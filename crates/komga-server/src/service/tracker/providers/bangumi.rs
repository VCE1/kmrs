use super::parse_tracker_url;
use crate::service::tracker::oauth::OAuthManager;
use crate::service::tracker::{Provider, TrackerError, TrackerService};
use komga_core::tracker::{RemoteState, TrackMediaType, TrackSearchItem, TrackStatus, TrackUpdate};
use reqwest::{Method, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

const API_ORIGIN: &str = "https://next.bgm.tv";
const SITE_ORIGIN: &str = "https://bgm.tv";
const USER_AGENT: &str = concat!("kmrs/", env!("CARGO_PKG_VERSION"));

pub struct BangumiTracker {
    http: reqwest::Client,
    oauth: Arc<OAuthManager>,
    api_origin: String,
}

#[derive(Debug, Default, Deserialize)]
struct Subject {
    id: i64,
    #[serde(default)]
    name: String,
    #[serde(default, rename = "nameCN")]
    name_cn: Option<String>,
    #[serde(default)]
    info: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    images: Option<Images>,
    #[serde(default)]
    platform: Option<SubjectPlatform>,
    #[serde(default)]
    series: bool,
    #[serde(default)]
    eps: Option<i32>,
    #[serde(default)]
    volumes: Option<i32>,
    #[serde(default)]
    interest: Option<SubjectInterest>,
}

#[derive(Debug, Deserialize)]
struct SubjectPlatform {
    #[serde(default, rename = "typeCN")]
    type_cn: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SubjectInterest {
    #[serde(default, rename = "type")]
    collection_type: Option<i64>,
    #[serde(default)]
    rate: Option<i32>,
    #[serde(default, rename = "epStatus")]
    ep_status: Option<i64>,
    #[serde(default, rename = "volStatus")]
    vol_status: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct Images {
    #[serde(default)]
    large: Option<String>,
    #[serde(default)]
    common: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SearchPage {
    #[serde(default)]
    data: Vec<Subject>,
}

fn provider_error(message: impl Into<String>) -> TrackerError {
    TrackerError::Provider {
        provider: Provider::Bangumi,
        message: message.into(),
    }
}

fn not_logged_in() -> TrackerError {
    TrackerError::NotLoggedIn {
        provider: Provider::Bangumi,
    }
}

fn track_id(value: &str) -> Result<i64, TrackerError> {
    value
        .parse()
        .map_err(|_| provider_error(format!("invalid Bangumi subject id '{value}'")))
}

fn collection_status(collection_type: Option<i64>) -> Option<TrackStatus> {
    match collection_type {
        Some(1) => Some(TrackStatus::Planning),
        Some(2) => Some(TrackStatus::Completed),
        Some(3) => Some(TrackStatus::Reading),
        Some(4) => Some(TrackStatus::Paused),
        Some(5) => Some(TrackStatus::Dropped),
        _ => None,
    }
}

fn collection_type(status: TrackStatus) -> i64 {
    match status {
        TrackStatus::Reading | TrackStatus::Rereading => 3,
        TrackStatus::Planning => 1,
        TrackStatus::Completed => 2,
        TrackStatus::Paused => 4,
        TrackStatus::Dropped => 5,
    }
}

impl BangumiTracker {
    pub fn new(http: reqwest::Client, oauth: Arc<OAuthManager>) -> Self {
        Self::with_api_origin(http, oauth, API_ORIGIN)
    }

    pub fn with_api_origin(
        http: reqwest::Client,
        oauth: Arc<OAuthManager>,
        api_origin: impl Into<String>,
    ) -> Self {
        Self {
            http,
            oauth,
            api_origin: api_origin.into().trim_end_matches('/').to_string(),
        }
    }

    fn api_url(&self, segments: &[&str]) -> Result<Url, TrackerError> {
        let mut url = Url::parse(&format!("{}/", self.api_origin))
            .map_err(|error| provider_error(format!("invalid API origin: {error}")))?;
        url.path_segments_mut()
            .map_err(|_| provider_error("API origin cannot contain path segments"))?
            .extend(segments);
        Ok(url)
    }

    async fn access_token(&self, user_id: &str) -> Result<String, TrackerError> {
        self.oauth
            .access_token(Provider::Bangumi, user_id)
            .await?
            .ok_or_else(not_logged_in)
    }

    async fn send_authed(
        &self,
        user_id: &str,
        method: Method,
        url: Url,
        body: Option<&Value>,
        action: &str,
    ) -> Result<reqwest::Response, TrackerError> {
        let token = self.access_token(user_id).await?;
        let build = |token: &str| {
            let mut request = self
                .http
                .request(method.clone(), url.clone())
                .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
                .header(reqwest::header::USER_AGENT, USER_AGENT);
            if let Some(body) = body {
                request = request.json(body);
            }
            request
        };
        let mut response = build(&token)
            .send()
            .await
            .map_err(|error| provider_error(format!("{action} request failed: {error}")))?;
        if response.status() == StatusCode::UNAUTHORIZED {
            let token = match self.oauth.refresh_now(Provider::Bangumi, user_id).await? {
                Some(token) => token,
                None => {
                    self.oauth
                        .logout_if_current(Provider::Bangumi, user_id, &token)?;
                    return Err(not_logged_in());
                }
            };
            response = build(&token)
                .send()
                .await
                .map_err(|error| provider_error(format!("{action} retry failed: {error}")))?;
            if response.status() == StatusCode::UNAUTHORIZED {
                self.oauth
                    .logout_if_current(Provider::Bangumi, user_id, &token)?;
                return Err(not_logged_in());
            }
        }
        Ok(response)
    }

    async fn decode_json<T: DeserializeOwned>(
        response: reqwest::Response,
        action: &str,
    ) -> Result<T, TrackerError> {
        let status = response.status();
        let body = response.text().await.map_err(|error| {
            provider_error(format!("{action} response could not be read: {error}"))
        })?;
        if !status.is_success() {
            return Err(provider_error(format!(
                "{action} returned {status}: {body}"
            )));
        }
        serde_json::from_str(&body)
            .map_err(|error| provider_error(format!("{action} response is invalid: {error}")))
    }

    async fn ensure_success(response: reqwest::Response, action: &str) -> Result<(), TrackerError> {
        if response.status().is_success() {
            return Ok(());
        }
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(provider_error(format!(
            "{action} returned {status}: {body}"
        )))
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        user_id: &str,
        url: Url,
        action: &str,
    ) -> Result<T, TrackerError> {
        let response = self
            .send_authed(user_id, Method::GET, url, None, action)
            .await?;
        Self::decode_json(response, action).await
    }

    fn subject_url_id(query: &str) -> Option<i64> {
        let url = parse_tracker_url(query)?;
        let host = url.host_str()?.to_ascii_lowercase();
        if host != "bgm.tv" && host != "bangumi.tv" && !host.ends_with(".bgm.tv") {
            return None;
        }
        let mut segments = url.path_segments()?;
        if segments.next()? != "subject" {
            return None;
        }
        segments.next()?.parse().ok()
    }

    fn search_item(subject: Subject) -> TrackSearchItem {
        let tracked = subject.interest.is_some();
        let title = subject
            .name_cn
            .as_deref()
            .filter(|title| !title.is_empty())
            .unwrap_or(&subject.name);
        let media_type = match subject
            .platform
            .as_ref()
            .and_then(|platform| platform.type_cn.as_deref())
        {
            Some(platform) if platform.contains("小说") => Some(TrackMediaType::Novel),
            Some(platform) if platform.contains("漫画") => Some(TrackMediaType::Manga),
            _ => None,
        };
        let cover_url = subject
            .images
            .as_ref()
            .and_then(|images| images.large.clone().or_else(|| images.common.clone()));
        TrackSearchItem {
            id: subject.id.to_string(),
            title: if title.is_empty() {
                "Unknown Title"
            } else {
                title
            }
            .into(),
            cover_url,
            info: subject.info.filter(|info| !info.is_empty()),
            description: subject.summary.filter(|summary| !summary.is_empty()),
            tracked,
            media_type,
            series: subject.series,
            url: Some(format!("{SITE_ORIGIN}/subject/{}", subject.id)),
        }
    }

    async fn search_next(
        &self,
        user_id: &str,
        query: &str,
        nsfw: bool,
    ) -> Result<Vec<Subject>, TrackerError> {
        let mut url = self.api_url(&["p1", "search", "subjects"])?;
        url.query_pairs_mut().append_pair("limit", "20");
        let body = json!({
            "keyword": query,
            "sort": "match",
            "filter": { "type": [1], "nsfw": nsfw }
        });
        let response = self
            .send_authed(user_id, Method::POST, url, Some(&body), "Bangumi search")
            .await?;
        let page: SearchPage = Self::decode_json(response, "Bangumi search").await?;
        Ok(page.data)
    }

    async fn subject(&self, user_id: &str, id: i64) -> Result<Subject, TrackerError> {
        self.get_json(
            user_id,
            self.api_url(&["p1", "subjects", &id.to_string()])?,
            "Bangumi subject detail",
        )
        .await
    }
}

#[async_trait::async_trait]
impl TrackerService for BangumiTracker {
    fn provider(&self) -> Provider {
        Provider::Bangumi
    }

    fn is_logged_in(&self, user_id: &str) -> Result<bool, TrackerError> {
        Ok(self.oauth.status(Provider::Bangumi, user_id)?.logged_in)
    }

    async fn search(
        &self,
        user_id: &str,
        query: &str,
        nsfw: bool,
        _language: Option<&str>,
    ) -> Result<Vec<TrackSearchItem>, TrackerError> {
        if let Some(id) = Self::subject_url_id(query) {
            let subject = self.subject(user_id, id).await?;
            return Ok(vec![Self::search_item(subject)]);
        }
        let subjects = self.search_next(user_id, query, nsfw).await?;
        Ok(subjects.into_iter().map(Self::search_item).collect())
    }

    async fn get_state(&self, user_id: &str, id: &str) -> Result<RemoteState, TrackerError> {
        let id = track_id(id)?;
        let subject = self.subject(user_id, id).await?;
        let interest = subject.interest.unwrap_or_default();
        Ok(RemoteState {
            score: interest.rate,
            status: collection_status(interest.collection_type),
            last_read_chapter: interest.ep_status.map(|value| value as f32),
            last_read_volume: interest.vol_status,
            total_chapters: subject.eps,
            total_volumes: subject.volumes,
            start_read_date: None,
            finish_read_date: None,
        })
    }

    async fn update(
        &self,
        user_id: &str,
        id: &str,
        update: &TrackUpdate,
    ) -> Result<(), TrackerError> {
        let id = track_id(id)?;
        let mut collection_body = serde_json::Map::new();
        if let Some(status) = update.status {
            collection_body.insert("type".into(), json!(collection_type(status)));
        }
        if let Some(score) = update.score {
            collection_body.insert("rate".into(), json!(score));
        }
        let mut progress_body = serde_json::Map::new();
        if let Some(chapter) = update.last_read_chapter {
            progress_body.insert("epStatus".into(), json!(chapter.floor() as i64));
        }
        if let Some(volume) = update.last_read_volume {
            progress_body.insert("volStatus".into(), json!(volume));
        }
        if collection_body.is_empty() && progress_body.is_empty() {
            return Ok(());
        }

        let has_collection_update = !collection_body.is_empty();
        if has_collection_update && (update.status.is_none() || update.score.is_none()) {
            let interest = self
                .subject(user_id, id)
                .await?
                .interest
                .unwrap_or_default();
            if update.status.is_none() {
                collection_body.insert("type".into(), json!(interest.collection_type.unwrap_or(3)));
            }
            if update.score.is_none() {
                collection_body.insert("rate".into(), json!(interest.rate.unwrap_or(0)));
            }
        }

        let url = self.api_url(&["p1", "collections", "subjects", &id.to_string()])?;
        if has_collection_update {
            let response = self
                .send_authed(
                    user_id,
                    Method::PUT,
                    url.clone(),
                    Some(&Value::Object(collection_body)),
                    "Bangumi collection update",
                )
                .await?;
            Self::ensure_success(response, "Bangumi collection update").await?;
        }
        if !progress_body.is_empty() {
            let response = self
                .send_authed(
                    user_id,
                    Method::PATCH,
                    url.clone(),
                    Some(&Value::Object(progress_body.clone())),
                    "Bangumi progress update",
                )
                .await?;
            if response.status() == StatusCode::NOT_FOUND && !has_collection_update {
                let response = self
                    .send_authed(
                        user_id,
                        Method::PUT,
                        url.clone(),
                        Some(&json!({"type": 3, "rate": 0})),
                        "Bangumi collection update",
                    )
                    .await?;
                Self::ensure_success(response, "Bangumi collection update").await?;
                let response = self
                    .send_authed(
                        user_id,
                        Method::PATCH,
                        url,
                        Some(&Value::Object(progress_body)),
                        "Bangumi progress update",
                    )
                    .await?;
                Self::ensure_success(response, "Bangumi progress update").await?;
            } else {
                Self::ensure_success(response, "Bangumi progress update").await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::tracker::oauth::OAuthApp;
    use axum::extract::{Request, State};
    use axum::http::{HeaderMap, Method, StatusCode};
    use axum::response::{IntoResponse, Response};
    use axum::{Json, Router};
    use komga_core::tracker::{BookTrackInput, TrackMode};
    use komga_db::dao::tracker_oauth::{TrackerOAuthDao, TrackerOAuthToken};
    use komga_db::pool::Database;
    use komga_db::{Migrator, Placeholders};
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Debug, Clone)]
    struct CapturedRequest {
        method: Method,
        path: String,
        query: String,
        authorization: Option<String>,
        json: Option<Value>,
        form: HashMap<String, String>,
    }

    type Responder = dyn Fn(&CapturedRequest) -> (StatusCode, Value) + Send + Sync;

    #[derive(Clone)]
    struct Mock {
        requests: Arc<Mutex<Vec<CapturedRequest>>>,
        responder: Arc<Responder>,
    }

    impl Mock {
        fn requests(&self) -> Vec<CapturedRequest> {
            self.requests.lock().unwrap().clone()
        }
    }

    fn authorization(headers: &HeaderMap) -> Option<String> {
        headers
            .get(reqwest::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    }

    async fn handle(State(mock): State<Mock>, request: Request) -> Response {
        let (parts, body) = request.into_parts();
        let bytes = axum::body::to_bytes(body, 1024 * 1024).await.unwrap();
        let captured = CapturedRequest {
            method: parts.method,
            path: parts.uri.path().to_string(),
            query: parts.uri.query().unwrap_or_default().to_string(),
            authorization: authorization(&parts.headers),
            json: serde_json::from_slice(&bytes).ok(),
            form: form_urlencoded::parse(&bytes).into_owned().collect(),
        };
        let (status, body) = (mock.responder)(&captured);
        mock.requests.lock().unwrap().push(captured);
        (status, Json(body)).into_response()
    }

    async fn serve<F>(responder: F) -> (String, Mock)
    where
        F: Fn(&CapturedRequest) -> (StatusCode, Value) + Send + Sync + 'static,
    {
        let mock = Mock {
            requests: Arc::new(Mutex::new(Vec::new())),
            responder: Arc::new(responder),
        };
        let app = Router::new()
            .fallback(axum::routing::any(handle))
            .with_state(mock.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}"), mock)
    }

    fn query(request: &CapturedRequest, key: &str) -> Option<String> {
        form_urlencoded::parse(request.query.as_bytes())
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.into_owned())
    }

    fn test_service(origin: &str) -> BangumiTracker {
        let db = Database::open_in_memory(false).unwrap();
        Migrator::new(&komga_db::kmrs_migrations(), Placeholders::default())
            .migrate(&db.rw().unwrap())
            .unwrap();
        let dao = TrackerOAuthDao::new(db.clone());
        dao.upsert_token(
            "bangumi",
            "alice",
            &TrackerOAuthToken {
                access_token: "stale".into(),
                refresh_token: Some("refresh-old".into()),
                token_type: Some("Bearer".into()),
                expires_at: None,
                username: Some("alice-bg".into()),
            },
        )
        .unwrap();
        let mut app = OAuthApp::for_provider(Provider::Bangumi);
        app.client_id = Some("test-id".into());
        app.client_secret = Some("test-secret".into());
        app.token_url = format!("{origin}/token");
        let manager = OAuthManager::with_apps(
            db,
            reqwest::Client::new(),
            HashMap::from([(Provider::Bangumi, app)]),
        );
        BangumiTracker::with_api_origin(reqwest::Client::new(), manager, origin)
    }

    fn subject(id: i64, name: &str) -> Value {
        json!({
            "id": id,
            "name": name,
            "nameCN": "",
            "images": {"large": format!("https://img.test/{id}.jpg")},
            "info": format!("Info {id}"),
            "summary": format!("Summary {id}"),
            "platform": {"typeCN": "漫画"},
            "eps": 30,
            "volumes": 6,
            "interest": null
        })
    }

    fn slim_subject(id: i64, name: &str) -> Value {
        json!({
            "id": id,
            "name": name,
            "nameCN": "",
            "images": {"large": format!("https://img.test/{id}.jpg")},
            "info": format!("Info {id}"),
            "type": 1,
            "platform": {"typeCN": "漫画"},
            "interest": null
        })
    }

    #[tokio::test]
    async fn next_search_uses_embedded_platform_and_interest() {
        let (origin, mock) = serve(move |request| match request.path.as_str() {
            "/p1/search/subjects" => {
                let mut manga = slim_subject(101, "Book");
                manga["nameCN"] = json!("书籍");
                manga["interest"] = json!({"type": 3, "rate": 8});
                manga["series"] = json!(true);
                let mut novel = slim_subject(102, "Novel");
                novel["platform"]["typeCN"] = json!("轻小说");
                (StatusCode::OK, json!({"data": [manga, novel], "total": 2}))
            }
            _ => (StatusCode::NOT_FOUND, json!({"message": "unexpected"})),
        })
        .await;
        let service = test_service(&origin);
        assert_eq!(service.provider(), Provider::Bangumi);
        assert!(service.is_logged_in("alice").unwrap());

        let results = service.search("alice", "Book", false, None).await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id, "101");
        assert_eq!(results[0].title, "书籍");
        assert!(results[0].tracked);
        assert_eq!(results[0].media_type, Some(TrackMediaType::Manga));
        assert!(results[0].series);
        assert_eq!(results[0].info.as_deref(), Some("Info 101"));
        assert_eq!(results[0].description, None);
        assert_eq!(results[1].id, "102");
        assert!(!results[1].tracked);
        assert!(!results[1].series);
        assert_eq!(results[1].media_type, Some(TrackMediaType::Novel));
        assert_eq!(results[1].info.as_deref(), Some("Info 102"));
        assert_eq!(results[1].description, None);
        let requests = mock.requests();
        assert_eq!(requests.len(), 1);
        let search = &requests[0];
        assert_eq!(search.path, "/p1/search/subjects");
        assert_eq!(search.method, Method::POST);
        assert_eq!(query(search, "limit").as_deref(), Some("20"));
        assert_eq!(search.authorization.as_deref(), Some("Bearer stale"));
        assert_eq!(
            search.json,
            Some(json!({
                "keyword": "Book",
                "sort": "match",
                "filter": {"type": [1], "nsfw": false}
            }))
        );
    }

    #[tokio::test]
    async fn failed_next_search_returns_error_without_detail_requests() {
        let (origin, mock) = serve(|request| match request.path.as_str() {
            "/p1/search/subjects" => (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"message": "search unavailable"}),
            ),
            _ => (StatusCode::NOT_FOUND, json!({"message": "unexpected"})),
        })
        .await;
        let service = test_service(&origin);

        assert!(matches!(
            service.search("alice", "标题 / A", true, None).await,
            Err(TrackerError::Provider {
                provider: Provider::Bangumi,
                ..
            })
        ));
        let requests = mock.requests();
        let search = requests
            .iter()
            .find(|request| request.path == "/p1/search/subjects")
            .unwrap();
        assert_eq!(search.json.as_ref().unwrap()["filter"]["nsfw"], true);
        assert!(!requests
            .iter()
            .any(|request| request.path.starts_with("/p1/subjects/")));
    }

    #[tokio::test]
    async fn state_propagates_subject_detail_failure() {
        let (origin, _) = serve(|request| match request.path.as_str() {
            "/p1/subjects/42" => (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"message": "subject unavailable"}),
            ),
            _ => (StatusCode::NOT_FOUND, json!({"message": "unexpected"})),
        })
        .await;

        assert!(matches!(
            test_service(&origin).get_state("alice", "42").await,
            Err(TrackerError::Provider {
                provider: Provider::Bangumi,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn state_reads_interest_and_totals_from_subject() {
        let (origin, mock) = serve(|request| match request.path.as_str() {
            "/p1/subjects/42" => {
                let mut value = subject(42, "Book");
                value["interest"] = json!({"type": 3, "rate": 8, "epStatus": 12, "volStatus": 2});
                (StatusCode::OK, value)
            }
            "/p1/subjects/43" => (StatusCode::OK, subject(43, "Book 43")),
            _ => (StatusCode::NOT_FOUND, json!({"message": "unexpected"})),
        })
        .await;
        let service = test_service(&origin);

        let state = service.get_state("alice", "42").await.unwrap();
        assert_eq!(state.score, Some(8));
        assert_eq!(state.status, Some(TrackStatus::Reading));
        assert_eq!(state.last_read_chapter, Some(12.0));
        assert_eq!(state.last_read_volume, Some(2));
        assert_eq!(state.total_chapters, Some(30));
        assert_eq!(state.total_volumes, Some(6));
        assert_eq!(state.start_read_date, None);
        assert_eq!(state.finish_read_date, None);

        let uncollected = service.get_state("alice", "43").await.unwrap();
        assert_eq!(uncollected.score, None);
        assert_eq!(uncollected.status, None);
        assert_eq!(uncollected.last_read_chapter, None);
        assert_eq!(uncollected.last_read_volume, None);
        assert_eq!(uncollected.total_chapters, Some(30));
        assert_eq!(uncollected.total_volumes, Some(6));
        let books = [BookTrackInput {
            name: "ch.30",
            number_sort: 30.0,
            read: true,
        }];
        let update = komga_core::tracker::decide(
            TrackMode::Chapter,
            0,
            &uncollected,
            &books,
            time::Date::from_calendar_date(2026, time::Month::October, 25).unwrap(),
        )
        .unwrap();
        assert_eq!(update.status, Some(TrackStatus::Completed));

        let requests = mock.requests();
        assert_eq!(requests.len(), 2);
        assert!(requests
            .iter()
            .all(|request| request.path.starts_with("/p1/subjects/")));
    }

    #[tokio::test]
    async fn final_unauthorized_logs_out() {
        let (origin, _) = serve(|request| match request.path.as_str() {
            "/token" => (
                StatusCode::OK,
                json!({"access_token": "fresh", "expires_in": 3600}),
            ),
            _ => (StatusCode::UNAUTHORIZED, json!({"message": "expired"})),
        })
        .await;
        let service = test_service(&origin);
        let update = TrackUpdate {
            last_read_chapter: Some(1.0),
            ..TrackUpdate::default()
        };
        assert!(matches!(
            service.update("alice", "42", &update).await,
            Err(TrackerError::NotLoggedIn { .. })
        ));
        assert!(
            !service
                .oauth
                .status(Provider::Bangumi, "alice")
                .unwrap()
                .logged_in
        );
    }

    #[tokio::test]
    async fn update_splits_collection_and_progress_and_refreshes_once() {
        let (origin, mock) = serve(|request| match request.path.as_str() {
            "/token" => (
                StatusCode::OK,
                json!({"access_token": "fresh", "refresh_token": "refresh-new", "expires_in": 3600}),
            ),
            "/p1/collections/subjects/42"
                if request.authorization.as_deref() == Some("Bearer stale") =>
            {
                (StatusCode::UNAUTHORIZED, json!({"message": "expired"}))
            }
            "/p1/collections/subjects/42" => (StatusCode::OK, json!({})),
            _ => (StatusCode::NOT_FOUND, json!({"message": "unexpected"})),
        })
        .await;
        let service = test_service(&origin);
        let update = TrackUpdate {
            score: Some(8),
            status: Some(TrackStatus::Rereading),
            last_read_chapter: Some(12.9),
            last_read_volume: Some(3),
            start_read_date: Some("2026-10-01".into()),
            finish_read_date: Some("2026-10-20".into()),
        };

        service.update("alice", "42", &update).await.unwrap();
        let requests = mock.requests();
        let updates = requests
            .iter()
            .filter(|request| request.path == "/p1/collections/subjects/42")
            .collect::<Vec<_>>();
        assert_eq!(updates.len(), 3);
        assert_eq!(updates[0].method, Method::PUT);
        assert_eq!(updates[1].method, Method::PUT);
        assert_eq!(updates[2].method, Method::PATCH);
        assert_eq!(updates[0].authorization.as_deref(), Some("Bearer stale"));
        assert_eq!(updates[1].authorization.as_deref(), Some("Bearer fresh"));
        assert_eq!(updates[2].authorization.as_deref(), Some("Bearer fresh"));
        for request in &updates[..2] {
            assert_eq!(request.json, Some(json!({"type": 3, "rate": 8})));
        }
        assert_eq!(
            updates[2].json,
            Some(json!({"epStatus": 12, "volStatus": 3}))
        );
        let tokens = requests
            .iter()
            .filter(|request| request.path == "/token")
            .collect::<Vec<_>>();
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].form["grant_type"], "refresh_token");
        assert_eq!(tokens[0].form["refresh_token"], "refresh-old");
        assert!(!requests
            .iter()
            .any(|request| request.path == "/p1/subjects/42"));
    }

    #[tokio::test]
    async fn status_only_update_preserves_existing_rating() {
        let (origin, mock) = serve(|request| match request.path.as_str() {
            "/p1/subjects/42" => {
                let mut value = subject(42, "Book");
                value["interest"] = json!({"type": 1, "rate": 8, "epStatus": 4, "volStatus": 1});
                (StatusCode::OK, value)
            }
            "/p1/collections/subjects/42" => (StatusCode::OK, json!({})),
            _ => (StatusCode::NOT_FOUND, json!({"message": "unexpected"})),
        })
        .await;
        let update = TrackUpdate {
            status: Some(TrackStatus::Reading),
            ..TrackUpdate::default()
        };

        test_service(&origin)
            .update("alice", "42", &update)
            .await
            .unwrap();
        let requests = mock.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].path, "/p1/subjects/42");
        assert_eq!(requests[1].method, Method::PUT);
        assert_eq!(requests[1].json, Some(json!({"type": 3, "rate": 8})));
    }

    #[tokio::test]
    async fn score_only_update_creates_reading_collection_when_missing() {
        let (origin, mock) = serve(|request| match request.path.as_str() {
            "/p1/subjects/42" => (StatusCode::OK, subject(42, "Book")),
            "/p1/collections/subjects/42" => (StatusCode::OK, json!({})),
            _ => (StatusCode::NOT_FOUND, json!({"message": "unexpected"})),
        })
        .await;
        let update = TrackUpdate {
            score: Some(8),
            ..TrackUpdate::default()
        };

        test_service(&origin)
            .update("alice", "42", &update)
            .await
            .unwrap();
        let requests = mock.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[1].method, Method::PUT);
        assert_eq!(requests[1].json, Some(json!({"type": 3, "rate": 8})));
    }

    #[tokio::test]
    async fn progress_only_update_sends_one_patch() {
        let (origin, mock) = serve(|request| match request.path.as_str() {
            "/p1/collections/subjects/42" => (StatusCode::OK, json!({})),
            _ => (StatusCode::NOT_FOUND, json!({"message": "unexpected"})),
        })
        .await;
        let update = TrackUpdate {
            last_read_chapter: Some(12.9),
            ..TrackUpdate::default()
        };

        test_service(&origin)
            .update("alice", "42", &update)
            .await
            .unwrap();
        let requests = mock.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, Method::PATCH);
        assert_eq!(requests[0].json, Some(json!({"epStatus": 12})));
    }

    #[tokio::test]
    async fn progress_only_update_creates_missing_collection_and_retries() {
        let collected = Arc::new(Mutex::new(false));
        let collected_by_put = collected.clone();
        let (origin, mock) = serve(move |request| match request.path.as_str() {
            "/p1/collections/subjects/42" if request.method == Method::PUT => {
                *collected_by_put.lock().unwrap() = true;
                (StatusCode::OK, json!({}))
            }
            "/p1/collections/subjects/42" if !*collected.lock().unwrap() => (
                StatusCode::NOT_FOUND,
                json!({"message": "subject not collected"}),
            ),
            "/p1/collections/subjects/42" => (StatusCode::OK, json!({})),
            _ => (StatusCode::NOT_FOUND, json!({"message": "unexpected"})),
        })
        .await;
        let update = TrackUpdate {
            last_read_chapter: Some(12.9),
            last_read_volume: Some(3),
            ..TrackUpdate::default()
        };

        test_service(&origin)
            .update("alice", "42", &update)
            .await
            .unwrap();
        let requests = mock.requests();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].method, Method::PATCH);
        assert_eq!(requests[1].method, Method::PUT);
        assert_eq!(requests[2].method, Method::PATCH);
        assert_eq!(requests[1].json, Some(json!({"type": 3, "rate": 0})));
        for request in [&requests[0], &requests[2]] {
            assert_eq!(request.json, Some(json!({"epStatus": 12, "volStatus": 3})));
        }
    }

    #[tokio::test]
    async fn empty_or_date_only_update_skips_requests() {
        let (origin, mock) = serve(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"message": "unexpected"}),
            )
        })
        .await;
        let service = test_service(&origin);
        service
            .update("alice", "42", &TrackUpdate::default())
            .await
            .unwrap();
        let update = TrackUpdate {
            start_read_date: Some("2026-10-01".into()),
            finish_read_date: Some("2026-10-20".into()),
            ..TrackUpdate::default()
        };
        service.update("alice", "42", &update).await.unwrap();
        assert!(mock.requests().is_empty());
    }

    #[tokio::test]
    async fn subject_url_uses_detail_and_official_result_url() {
        let (origin, mock) = serve(|request| match request.path.as_str() {
            "/p1/subjects/1902" => {
                let mut value = subject(1902, "Book");
                value["interest"] = json!({"type": 3, "rate": 8});
                (StatusCode::OK, value)
            }
            _ => (StatusCode::NOT_FOUND, json!({"message": "unexpected"})),
        })
        .await;
        let service = test_service(&origin);

        let results = service
            .search("alice", "bgm.tv/subject/1902", false, None)
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "1902");
        assert!(results[0].tracked);
        assert_eq!(results[0].media_type, Some(TrackMediaType::Manga));
        assert_eq!(results[0].info.as_deref(), Some("Info 1902"));
        assert_eq!(results[0].description.as_deref(), Some("Summary 1902"));
        assert_eq!(
            results[0].url.as_deref(),
            Some("https://bgm.tv/subject/1902")
        );
        let requests = mock.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/p1/subjects/1902");
    }

    #[test]
    fn status_mapping_has_no_rereading_collection_type() {
        assert_eq!(collection_status(Some(1)), Some(TrackStatus::Planning));
        assert_eq!(collection_status(Some(2)), Some(TrackStatus::Completed));
        assert_eq!(collection_status(Some(3)), Some(TrackStatus::Reading));
        assert_eq!(collection_status(Some(4)), Some(TrackStatus::Paused));
        assert_eq!(collection_status(Some(5)), Some(TrackStatus::Dropped));
        assert_eq!(collection_type(TrackStatus::Rereading), 3);
    }
}
