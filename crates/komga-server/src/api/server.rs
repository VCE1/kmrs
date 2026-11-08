//! kmrs-native server endpoints (`/api/v1/server/**`): what the webui needs from the
//! server itself — version info, scheduled scans and the log file.
//! kmrs-private like `api::stats` (kept out of the OpenAPI spec). Unlike the actuator
//! parity surface, everything but `/info` is ADMIN-gated.

use crate::auth::RequireAuth;
use crate::error::ApiError;
use crate::state::AppState;
use axum::body::Body;
use axum::extract::State;
use axum::http::header::{CONTENT_LENGTH, CONTENT_TYPE};
use axum::response::{IntoResponse, Response};
use axum::{routing, Json, Router};
use komga_db::dao::komf_integration::{KomfIntegrationDao, KomfIntegrationState};
use serde::Serialize;
use tokio_util::io::ReaderStream;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/server/info", routing::get(get_info))
        .route(
            "/api/v1/server/scheduled-tasks",
            routing::get(get_scheduled_tasks),
        )
        .route("/api/v1/server/logfile", routing::get(get_logfile))
}

// region info

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerInfoDto {
    version: String,
    git_branch: String,
    git_commit_id: String,
    features: Vec<&'static str>,
}

/// No admin gate: the sidebar footer shows the version to every logged-in user,
/// and clients read `features` before any kmrs-private request.
async fn get_info(
    _auth: RequireAuth,
    State(state): State<AppState>,
) -> Result<Json<ServerInfoDto>, ApiError> {
    Ok(Json(ServerInfoDto {
        version: env!("CARGO_PKG_VERSION").to_string(),
        git_branch: env!("GIT_BRANCH").to_string(),
        git_commit_id: env!("GIT_COMMIT_ID").to_string(),
        features: features(&state)?,
    }))
}

/// The kmrs-private features this server can serve right now, so clients learn
/// from one request what to show instead of probing each endpoint group. A
/// feature that needs setup is listed only once it is set up.
fn features(state: &AppState) -> Result<Vec<&'static str>, ApiError> {
    // /api/v1/stats/libraries, /api/v1/stats/server, /api/v1/stats/reading/**,
    // /api/v1/smart-lists/**, /api/v2/users/me/sessions, /api/v2/users/sessions
    let mut features = vec!["serverStats", "readingStats", "smartLists", "sessions"];
    // /api/v1/komf/** proxies to komf only once the integration is connected
    let komf = KomfIntegrationDao::new(state.kmrs_db.clone()).get()?;
    if komf.is_some_and(|row| row.state == KomfIntegrationState::Connected) {
        features.push("komf");
    }
    // /api/v1/trackers/**
    if !state.tracker.all().is_empty() {
        features.push("trackers");
    }
    // /api/v1/update-check
    if state.config.update_check_enabled {
        features.push("updateCheck");
    }
    Ok(features)
}

// endregion

// region scheduled-tasks

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScheduledTasksDto {
    tasks: Vec<ScheduledTaskDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScheduledTaskDto {
    library_id: String,
    library_name: String,
    interval_seconds: u64,
}

async fn get_scheduled_tasks(auth: RequireAuth) -> Result<Json<ScheduledTasksDto>, ApiError> {
    auth.0.require_admin()?;
    let tasks = crate::service::scheduler::ScanScheduler::scheduled_tasks()
        .into_iter()
        .map(|registration| ScheduledTaskDto {
            library_id: registration.library_id,
            library_name: registration.library_name,
            interval_seconds: registration.period.as_secs(),
        })
        .collect();
    Ok(Json(ScheduledTasksDto { tasks }))
}

// endregion

// region logfile

async fn get_logfile(
    auth: RequireAuth,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    auth.0.require_admin()?;
    let logs_dir = state.config.config_dir.join("logs");
    let Some(path) = crate::api::actuator::latest_log_file(&logs_dir) else {
        return Ok(([(CONTENT_TYPE, "text/plain")], "").into_response());
    };
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|e| ApiError::Internal(format!("open {}: {e}", path.display())))?;
    let content_length = file.metadata().await.ok().map(|m| m.len().to_string());
    let body = Body::from_stream(ReaderStream::new(file));
    let mut response = ([(CONTENT_TYPE, "text/plain")], body).into_response();
    if let Some(len) = content_length {
        response
            .headers_mut()
            .insert(CONTENT_LENGTH, len.parse().unwrap());
    }
    Ok(response)
}

// endregion

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::collections::tests::{call, get, insert_user, test_state};
    use axum::http::{Request, StatusCode};
    use komga_core::model::user::UserRole;

    fn seed_admin(state: &AppState) -> String {
        insert_user(
            &state.db,
            "admin@x.c",
            &[UserRole::Admin],
            &[],
            Default::default(),
            "k-admin",
        );
        "k-admin".to_string()
    }

    fn seed_user(state: &AppState) -> String {
        insert_user(
            &state.db,
            "user@x.c",
            &[],
            &[],
            Default::default(),
            "k-user",
        );
        "k-user".to_string()
    }

    /// `get()` always sends an API key; anonymous and non-GET requests need a builder.
    fn request(method: &str, uri: &str, api_key: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(key) = api_key {
            builder = builder.header("X-API-Key", key);
        }
        builder.body(Body::empty()).unwrap()
    }

    fn json(bytes: &[u8]) -> serde_json::Value {
        serde_json::from_slice(bytes).unwrap()
    }

    #[tokio::test]
    async fn info_shape_and_any_authenticated_user() {
        let state = test_state();
        let user_key = seed_user(&state);

        let (status, _, body) = call(&state, router(), get("/api/v1/server/info", &user_key)).await;
        assert_eq!(status, StatusCode::OK);
        let body = json(&body);
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(body["gitBranch"], env!("GIT_BRANCH"));
        assert_eq!(body["gitCommitId"], env!("GIT_COMMIT_ID"));
        // test state: no komf integration, no tracker providers, update check on
        assert_eq!(
            body["features"],
            serde_json::json!([
                "serverStats",
                "readingStats",
                "smartLists",
                "sessions",
                "updateCheck"
            ])
        );

        let (status, _, _) = call(
            &state,
            router(),
            request("GET", "/api/v1/server/info", None),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    async fn info_features(state: &AppState, key: &str) -> serde_json::Value {
        let (status, _, body) = call(state, router(), get("/api/v1/server/info", key)).await;
        assert_eq!(status, StatusCode::OK);
        json(&body)["features"].clone()
    }

    #[tokio::test]
    async fn info_lists_komf_only_once_connected() {
        let state = test_state();
        let key = seed_user(&state);
        let dao = KomfIntegrationDao::new(state.kmrs_db.clone());

        dao.upsert("http://komf:8085", "http://kmrs:25600", None)
            .unwrap();
        let features = info_features(&state, &key).await;
        assert!(!features.as_array().unwrap().contains(&"komf".into()));

        dao.mark_connected("owner", "key").unwrap();
        let features = info_features(&state, &key).await;
        assert!(features.as_array().unwrap().contains(&"komf".into()));
    }

    #[tokio::test]
    async fn info_omits_update_check_when_disabled() {
        let state = test_state();
        let config = std::sync::Arc::new(crate::config::ServerConfig {
            update_check_enabled: false,
            ..(*state.config).clone()
        });
        let state = AppState { config, ..state };
        let key = seed_user(&state);
        let features = info_features(&state, &key).await;
        assert!(!features.as_array().unwrap().contains(&"updateCheck".into()));
    }

    #[tokio::test]
    async fn scheduled_tasks_empty_list_and_admin_gate() {
        let state = test_state();
        let admin_key = seed_admin(&state);
        let user_key = seed_user(&state);

        let (status, _, _) = call(
            &state,
            router(),
            get("/api/v1/server/scheduled-tasks", &user_key),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _, _) = call(
            &state,
            router(),
            request("GET", "/api/v1/server/scheduled-tasks", None),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, _, body) = call(
            &state,
            router(),
            get("/api/v1/server/scheduled-tasks", &admin_key),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json(&body), serde_json::json!({"tasks": []}));
    }

    #[tokio::test]
    async fn logfile_empty_200_without_logs_dir() {
        let state = test_state();
        let admin_key = seed_admin(&state);
        let user_key = seed_user(&state);
        // point at an empty config dir so the host's real logs can't leak into the test
        let dir = tempfile::tempdir().unwrap();
        let mut config = (*state.config).clone();
        config.config_dir = dir.path().to_path_buf();
        let state = AppState {
            config: std::sync::Arc::new(config),
            ..state
        };

        let (status, _, _) = call(&state, router(), get("/api/v1/server/logfile", &user_key)).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, _, _) = call(
            &state,
            router(),
            request("GET", "/api/v1/server/logfile", None),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);

        let (status, headers, body) =
            call(&state, router(), get("/api/v1/server/logfile", &admin_key)).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers.get(CONTENT_TYPE).unwrap(), "text/plain");
        assert!(body.is_empty());
    }
}
