use super::{parse_tracker_url, search_info};
use crate::service::tracker::oauth::OAuthManager;
use crate::service::tracker::{Provider, TrackerError, TrackerService};
use komga_core::tracker::{RemoteState, TrackMediaType, TrackSearchItem, TrackStatus, TrackUpdate};
use reqwest::{Method, StatusCode, Url};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;

const API_ORIGIN: &str = "https://api.mangabaka.org";
const SITE_ORIGIN: &str = "https://mangabaka.org";
const DEFAULT_TITLE_LANGUAGE: &str = "en";
const USER_AGENT: &str = concat!("kmrs/", env!("CARGO_PKG_VERSION"));
const SEARCH_LIMIT: usize = 20;

pub struct MangaBakaTracker {
    http: reqwest::Client,
    oauth: Arc<OAuthManager>,
    api_origin: String,
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    data: T,
}

#[derive(Debug, Default, Deserialize)]
struct Series {
    id: i64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    titles: Vec<SeriesTitle>,
    #[serde(default)]
    cover: Cover,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    year: Option<i64>,
    #[serde(default)]
    authors: Option<Vec<String>>,
    #[serde(default)]
    status: String,
    #[serde(default, rename = "type")]
    series_type: Option<String>,
    #[serde(default)]
    total_chapters: Option<String>,
    #[serde(default)]
    final_volume: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct SeriesTitle {
    #[serde(default)]
    language: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    traits: Vec<String>,
    #[serde(default)]
    is_primary: Option<bool>,
}

#[derive(Debug, Default, Deserialize)]
struct Cover {
    #[serde(default)]
    raw: Option<CoverRaw>,
    #[serde(default)]
    x350: Option<CoverDpi>,
}

#[derive(Debug, Deserialize)]
struct CoverRaw {
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CoverDpi {
    x1: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LibraryBatchEntry {
    series_id: i64,
}

#[derive(Debug, Default, Deserialize)]
struct LibraryEntry {
    #[serde(default)]
    rating: Option<f64>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    progress_chapter: Option<i64>,
    #[serde(default)]
    progress_volume: Option<i32>,
    #[serde(default)]
    start_date: Option<String>,
    #[serde(default)]
    finish_date: Option<String>,
}

fn provider_error(message: impl Into<String>) -> TrackerError {
    TrackerError::Provider {
        provider: Provider::Mangabaka,
        message: message.into(),
    }
}

fn not_logged_in() -> TrackerError {
    TrackerError::NotLoggedIn {
        provider: Provider::Mangabaka,
    }
}

fn track_id(value: &str) -> Result<i64, TrackerError> {
    value
        .parse()
        .map_err(|_| provider_error(format!("invalid MangaBaka series id '{value}'")))
}

fn library_status(status: &str) -> Option<TrackStatus> {
    match status {
        "reading" => Some(TrackStatus::Reading),
        "considering" | "plan_to_read" => Some(TrackStatus::Planning),
        "completed" => Some(TrackStatus::Completed),
        "paused" => Some(TrackStatus::Paused),
        "dropped" => Some(TrackStatus::Dropped),
        "rereading" => Some(TrackStatus::Rereading),
        _ => None,
    }
}

fn library_status_name(status: TrackStatus) -> &'static str {
    match status {
        TrackStatus::Reading => "reading",
        TrackStatus::Planning => "plan_to_read",
        TrackStatus::Completed => "completed",
        TrackStatus::Paused => "paused",
        TrackStatus::Dropped => "dropped",
        TrackStatus::Rereading => "rereading",
    }
}

fn normalized_language(language: &str) -> String {
    language.replace("-Latn", "-ro")
}

fn primary_title(titles: &[SeriesTitle], preference: &str) -> String {
    let preference = normalized_language(preference);
    if !preference.is_empty() {
        if let Some(title) = titles
            .iter()
            .find(|title| normalized_language(&title.language) == preference)
        {
            return title.title.clone();
        }
        if let Some(title) = titles.iter().find(|title| {
            normalized_language(&title.language).starts_with(&preference)
                && !title.language.ends_with("-Latn")
        }) {
            return title.title.clone();
        }
    }
    if let Some(title) = titles
        .iter()
        .find(|title| title.traits.iter().any(|trait_name| trait_name == "native"))
    {
        return title.title.clone();
    }
    if let Some(title) = titles
        .iter()
        .find(|title| title.language == "en" && title.is_primary == Some(true))
    {
        return title.title.clone();
    }
    titles
        .first()
        .map(|title| title.title.clone())
        .unwrap_or_default()
}

fn supported_date(date: &str) -> bool {
    date != "1969-12-31" && date != "1970-01-01"
}

impl Series {
    fn totals(&self) -> (Option<i32>, Option<i32>) {
        if self.status == "releasing" {
            return (None, None);
        }
        (
            self.total_chapters
                .as_deref()
                .and_then(|value| value.parse().ok()),
            self.final_volume
                .as_deref()
                .and_then(|value| value.parse().ok()),
        )
    }
}

impl MangaBakaTracker {
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
            .access_token(Provider::Mangabaka, user_id)
            .await?
            .ok_or_else(not_logged_in)
    }

    async fn send_public(&self, url: Url, action: &str) -> Result<reqwest::Response, TrackerError> {
        self.http
            .get(url)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .send()
            .await
            .map_err(|error| provider_error(format!("{action} request failed: {error}")))
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
            let token = match self.oauth.refresh_now(Provider::Mangabaka, user_id).await? {
                Some(token) => token,
                None => {
                    self.oauth
                        .logout_if_current(Provider::Mangabaka, user_id, &token)?;
                    return Err(not_logged_in());
                }
            };
            response = build(&token)
                .send()
                .await
                .map_err(|error| provider_error(format!("{action} retry failed: {error}")))?;
            if response.status() == StatusCode::UNAUTHORIZED {
                self.oauth
                    .logout_if_current(Provider::Mangabaka, user_id, &token)?;
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

    async fn get_authed_json<T: DeserializeOwned>(
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

    async fn get_public_json<T: DeserializeOwned>(
        &self,
        url: Url,
        action: &str,
    ) -> Result<T, TrackerError> {
        let response = self.send_public(url, action).await?;
        Self::decode_json(response, action).await
    }

    fn series_url_id(query: &str) -> Option<i64> {
        let url = parse_tracker_url(query)?;
        let host = url.host_str()?.to_ascii_lowercase();
        if host != "mangabaka.org" && !host.ends_with(".mangabaka.org") {
            return None;
        }
        url.path_segments()?.next()?.parse().ok()
    }

    async fn detail(&self, id: i64) -> Result<Series, TrackerError> {
        let envelope: Envelope<Series> = self
            .get_public_json(
                self.api_url(&["v1", "series", &id.to_string()])?,
                "MangaBaka series detail",
            )
            .await?;
        Ok(envelope.data)
    }

    fn search_item(
        &self,
        series: &Series,
        tracked: bool,
        language: Option<&str>,
    ) -> TrackSearchItem {
        let title = if series.titles.is_empty() {
            series.title.clone()
        } else {
            primary_title(&series.titles, language.unwrap_or(DEFAULT_TITLE_LANGUAGE))
        };
        let media_type = match series.series_type.as_deref() {
            Some("novel") => Some(TrackMediaType::Novel),
            Some("manga" | "manhwa" | "manhua" | "oel") => Some(TrackMediaType::Manga),
            _ => None,
        };
        TrackSearchItem {
            id: series.id.to_string(),
            title,
            cover_url: series
                .cover
                .x350
                .as_ref()
                .and_then(|cover| cover.x1.clone())
                .or_else(|| {
                    series
                        .cover
                        .raw
                        .as_ref()
                        .and_then(|cover| cover.url.clone())
                }),
            info: search_info(series.year, series.authors.iter().flatten().cloned()),
            description: series.description.clone(),
            tracked,
            media_type,
            series: false,
            url: Some(format!("{SITE_ORIGIN}/{}", series.id)),
        }
    }

    async fn tracked_ids(&self, user_id: &str, ids: &[i64]) -> Result<HashSet<i64>, TrackerError> {
        if ids.is_empty() {
            return Ok(HashSet::new());
        }
        let mut url = self.api_url(&["v1", "my", "library", "batch"])?;
        {
            let mut query = url.query_pairs_mut();
            for id in ids {
                query.append_pair("series_id", &id.to_string());
            }
        }
        let envelope: Envelope<Vec<LibraryBatchEntry>> = self
            .get_authed_json(user_id, url, "MangaBaka library batch")
            .await?;
        Ok(envelope
            .data
            .into_iter()
            .map(|entry| entry.series_id)
            .collect())
    }

    async fn library_entry(
        &self,
        user_id: &str,
        id: i64,
    ) -> Result<Option<LibraryEntry>, TrackerError> {
        let response = self
            .send_authed(
                user_id,
                Method::GET,
                self.api_url(&["v1", "my", "library", &id.to_string()])?,
                None,
                "MangaBaka library entry",
            )
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let envelope: Envelope<Option<LibraryEntry>> =
            Self::decode_json(response, "MangaBaka library entry").await?;
        Ok(envelope.data)
    }
}

#[async_trait::async_trait]
impl TrackerService for MangaBakaTracker {
    fn provider(&self) -> Provider {
        Provider::Mangabaka
    }

    fn is_logged_in(&self, user_id: &str) -> Result<bool, TrackerError> {
        Ok(self.oauth.status(Provider::Mangabaka, user_id)?.logged_in)
    }

    async fn search(
        &self,
        user_id: &str,
        query: &str,
        nsfw: bool,
        language: Option<&str>,
    ) -> Result<Vec<TrackSearchItem>, TrackerError> {
        let series = if let Some(id) = Self::series_url_id(query) {
            vec![self.detail(id).await?]
        } else {
            let mut url = self.api_url(&["v1", "series", "search"])?;
            {
                let mut pairs = url.query_pairs_mut();
                pairs
                    .append_pair("q", query)
                    .append_pair("limit", &SEARCH_LIMIT.to_string());
                if !nsfw {
                    pairs
                        .append_pair("not_content_rating", "erotica")
                        .append_pair("not_content_rating", "pornographic");
                }
            }
            let envelope: Envelope<Vec<Series>> =
                self.get_public_json(url, "MangaBaka series search").await?;
            envelope.data
        };
        let ids = series.iter().map(|series| series.id).collect::<Vec<_>>();
        let tracked = self.tracked_ids(user_id, &ids).await?;
        Ok(series
            .iter()
            .map(|series| self.search_item(series, tracked.contains(&series.id), language))
            .collect())
    }

    async fn get_state(&self, user_id: &str, id: &str) -> Result<RemoteState, TrackerError> {
        let id = track_id(id)?;
        let (total_chapters, total_volumes) = self.detail(id).await?.totals();
        let Some(entry) = self.library_entry(user_id, id).await? else {
            return Ok(RemoteState {
                total_chapters,
                total_volumes,
                ..RemoteState::default()
            });
        };
        Ok(RemoteState {
            score: entry.rating.map(|rating| rating.round() as i32),
            status: entry.state.as_deref().and_then(library_status),
            last_read_chapter: entry.progress_chapter.map(|chapter| chapter as f32),
            last_read_volume: entry.progress_volume,
            total_chapters,
            total_volumes,
            start_read_date: entry.start_date,
            finish_read_date: entry.finish_date,
        })
    }

    async fn update(
        &self,
        user_id: &str,
        id: &str,
        update: &TrackUpdate,
    ) -> Result<(), TrackerError> {
        let id = track_id(id)?;
        let mut body = serde_json::Map::new();
        if let Some(status) = update.status {
            body.insert("state".into(), json!(library_status_name(status)));
        }
        if let Some(score) = update.score {
            body.insert("rating".into(), json!(score));
        }
        if let Some(chapter) = update.last_read_chapter {
            body.insert("progress_chapter".into(), json!(chapter.floor() as i64));
        }
        if let Some(volume) = update.last_read_volume {
            body.insert("progress_volume".into(), json!(volume));
        }
        if let Some(date) = update
            .start_read_date
            .as_deref()
            .filter(|date| supported_date(date))
        {
            body.insert("start_date".into(), json!(date));
        }
        if let Some(date) = update
            .finish_read_date
            .as_deref()
            .filter(|date| supported_date(date))
        {
            body.insert("finish_date".into(), json!(date));
        }
        if body.is_empty() {
            return Ok(());
        }

        let exists = self.library_entry(user_id, id).await?.is_some();
        let method = if exists { Method::PATCH } else { Method::POST };
        let response = self
            .send_authed(
                user_id,
                method,
                self.api_url(&["v1", "my", "library", &id.to_string()])?,
                Some(&Value::Object(body)),
                "MangaBaka library update",
            )
            .await?;
        Self::ensure_success(response, "MangaBaka library update").await
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

    fn query_values(request: &CapturedRequest, key: &str) -> Vec<String> {
        form_urlencoded::parse(request.query.as_bytes())
            .filter(|(name, _)| name == key)
            .map(|(_, value)| value.into_owned())
            .collect()
    }

    fn test_service(origin: &str) -> MangaBakaTracker {
        let db = Database::open_in_memory(false).unwrap();
        Migrator::new(&komga_db::kmrs_migrations(), Placeholders::default())
            .migrate(&db.rw().unwrap())
            .unwrap();
        let dao = TrackerOAuthDao::new(db.clone());
        dao.upsert_token(
            "mangabaka",
            "alice",
            &TrackerOAuthToken {
                access_token: "stale".into(),
                refresh_token: Some("refresh-old".into()),
                token_type: Some("Bearer".into()),
                expires_at: None,
                username: Some("alice-mb".into()),
            },
        )
        .unwrap();
        let mut app = OAuthApp::for_provider(Provider::Mangabaka);
        app.client_id = Some("test-id".into());
        app.client_secret = Some("test-secret".into());
        app.token_url = format!("{origin}/token");
        let manager = OAuthManager::with_apps(
            db,
            reqwest::Client::new(),
            HashMap::from([(Provider::Mangabaka, app)]),
        );
        MangaBakaTracker::with_api_origin(reqwest::Client::new(), manager, origin)
    }

    fn series(id: i64) -> Value {
        json!({
            "id": id,
            "title": "Fallback",
            "titles": [
                {"language": "en", "title": format!("English {id}"), "is_primary": true},
                {"language": "ja", "title": format!("日本語 {id}"), "traits": ["native"]},
                {"language": "ja-Latn", "title": format!("Romaji {id}")}
            ],
            "cover": {
                "raw": {"url": format!("https://img.test/{id}-raw.jpg")},
                "x350": {"x1": format!("https://img.test/{id}.jpg")}
            },
            "description": format!("Description {id}"),
            "year": 1989,
            "authors": if id == 8 { Value::Null } else { json!(["MIURA Kentaro", "MORI Kouji"]) },
            "status": "completed",
            "type": if id == 8 { "novel" } else { "manga" },
            "total_chapters": "20",
            "final_volume": "2"
        })
    }

    #[tokio::test]
    async fn public_search_uses_batch_tracking_and_language() {
        let (origin, mock) = serve(|request| match request.path.as_str() {
            "/v1/series/search" => (StatusCode::OK, json!({"data": [series(7), series(8)]})),
            "/v1/my/library/batch" => (StatusCode::OK, json!({"data": [{"series_id": 7}]})),
            _ => (StatusCode::NOT_FOUND, json!({"message": "unexpected"})),
        })
        .await;
        let service = test_service(&origin);
        assert_eq!(service.provider(), Provider::Mangabaka);
        assert!(service.is_logged_in("alice").unwrap());

        let results = service
            .search("alice", "A title / 検索", false, Some("ja"))
            .await
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "日本語 7");
        assert!(results[0].tracked);
        assert_eq!(
            results[0].cover_url.as_deref(),
            Some("https://img.test/7.jpg")
        );
        assert_eq!(results[0].media_type, Some(TrackMediaType::Manga));
        assert_eq!(results[0].url.as_deref(), Some("https://mangabaka.org/7"));
        assert_eq!(
            results[0].info.as_deref(),
            Some("1989 / MIURA Kentaro / MORI Kouji")
        );
        assert_eq!(results[1].info.as_deref(), Some("1989"));
        assert!(!results[1].tracked);
        assert_eq!(results[1].media_type, Some(TrackMediaType::Novel));
        let requests = mock.requests();
        let search = requests
            .iter()
            .find(|request| request.path == "/v1/series/search")
            .unwrap();
        assert_eq!(search.method, Method::GET);
        assert_eq!(search.authorization, None);
        assert_eq!(query_values(search, "q"), vec!["A title / 検索"]);
        assert_eq!(query_values(search, "limit"), vec!["20"]);
        assert_eq!(
            query_values(search, "not_content_rating"),
            vec!["erotica", "pornographic"]
        );
        let batch = requests
            .iter()
            .find(|request| request.path == "/v1/my/library/batch")
            .unwrap();
        assert_eq!(batch.authorization.as_deref(), Some("Bearer stale"));
        assert_eq!(query_values(batch, "series_id"), vec!["7", "8"]);

        let default_title: Series = serde_json::from_value(series(9)).unwrap();
        assert_eq!(
            test_service(&origin)
                .search_item(&default_title, false, None)
                .title,
            "English 9"
        );
        let romaji = test_service(&origin);
        assert_eq!(
            romaji
                .search_item(&default_title, false, Some("ja-ro"))
                .title,
            "Romaji 9"
        );

        let localized = Series {
            titles: vec![
                SeriesTitle {
                    language: "zh-Hans".into(),
                    title: "简体中文".into(),
                    ..SeriesTitle::default()
                },
                SeriesTitle {
                    language: "zh-Hant".into(),
                    title: "繁體中文".into(),
                    ..SeriesTitle::default()
                },
            ],
            ..Series::default()
        };
        for (language, expected) in [
            (Some("zh-Hant"), "繁體中文"),
            (Some("zh-Hans"), "简体中文"),
            (Some("fr"), "简体中文"),
        ] {
            assert_eq!(
                romaji.search_item(&localized, false, language).title,
                expected
            );
        }
    }

    #[tokio::test]
    async fn url_search_fetches_public_detail_without_search_request() {
        let (origin, mock) = serve(|request| match request.path.as_str() {
            "/v1/series/77" => (StatusCode::OK, json!({"data": series(77)})),
            "/v1/my/library/batch" => (StatusCode::OK, json!({"data": []})),
            _ => (StatusCode::NOT_FOUND, json!({"message": "unexpected"})),
        })
        .await;
        let service = test_service(&origin);

        let results = service
            .search("alice", "mangabaka.org/77", true, Some("ja"))
            .await
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "77");
        assert_eq!(results[0].title, "日本語 77");
        let requests = mock.requests();
        let detail = requests
            .iter()
            .find(|request| request.path == "/v1/series/77")
            .unwrap();
        assert_eq!(detail.authorization, None);
        assert!(!requests
            .iter()
            .any(|request| request.path == "/v1/series/search"));
    }

    #[tokio::test]
    async fn state_maps_library_dates_progress_and_releasing_totals() {
        let (origin, _) = serve(|request| match request.path.as_str() {
            "/v1/series/42" => (StatusCode::OK, json!({"data": series(42)})),
            "/v1/my/library/42" => (
                StatusCode::OK,
                json!({"data": {
                    "rating": 86.6,
                    "state": "considering",
                    "progress_chapter": 14,
                    "progress_volume": 2,
                    "start_date": "2026-10-01",
                    "finish_date": "2026-10-20"
                }}),
            ),
            "/v1/series/43" => (
                StatusCode::OK,
                json!({"data": {
                    "id": 43,
                    "title": "Releasing",
                    "status": "releasing",
                    "total_chapters": "999",
                    "final_volume": "99"
                }}),
            ),
            "/v1/my/library/43" => (
                StatusCode::OK,
                json!({"data": {"state": "reading", "progress_chapter": 1}}),
            ),
            "/v1/series/44" => (StatusCode::OK, json!({"data": series(44)})),
            "/v1/my/library/44" => (
                StatusCode::NOT_FOUND,
                json!({"message": "not in library", "data": null}),
            ),
            "/v1/series/45" => (StatusCode::OK, json!({"data": series(45)})),
            "/v1/my/library/45" => (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"message": "library unavailable"}),
            ),
            _ => (StatusCode::NOT_FOUND, json!({"message": "unexpected"})),
        })
        .await;
        let service = test_service(&origin);

        let state = service.get_state("alice", "42").await.unwrap();
        assert_eq!(state.score, Some(87));
        assert_eq!(state.status, Some(TrackStatus::Planning));
        assert_eq!(state.last_read_chapter, Some(14.0));
        assert_eq!(state.last_read_volume, Some(2));
        assert_eq!(state.total_chapters, Some(20));
        assert_eq!(state.total_volumes, Some(2));
        assert_eq!(state.start_read_date.as_deref(), Some("2026-10-01"));
        assert_eq!(state.finish_read_date.as_deref(), Some("2026-10-20"));

        let releasing = service.get_state("alice", "43").await.unwrap();
        assert_eq!(releasing.total_chapters, None);
        assert_eq!(releasing.total_volumes, None);
        assert_eq!(releasing.status, Some(TrackStatus::Reading));
        let uncollected = service.get_state("alice", "44").await.unwrap();
        assert_eq!(uncollected.total_chapters, Some(20));
        assert_eq!(uncollected.total_volumes, Some(2));
        assert_eq!(uncollected.status, None);
        assert!(matches!(
            service.get_state("alice", "45").await,
            Err(TrackerError::Provider {
                provider: Provider::Mangabaka,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn search_propagates_library_batch_failure() {
        let (origin, _) = serve(|request| match request.path.as_str() {
            "/v1/series/search" => (StatusCode::OK, json!({"data": [series(7)]})),
            "/v1/my/library/batch" => (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"message": "library unavailable"}),
            ),
            _ => (StatusCode::NOT_FOUND, json!({"message": "unexpected"})),
        })
        .await;
        let service = test_service(&origin);

        assert!(matches!(
            service.search("alice", "Book", false, None).await,
            Err(TrackerError::Provider {
                provider: Provider::Mangabaka,
                ..
            })
        ));
    }

    #[tokio::test]
    async fn state_propagates_series_detail_failure() {
        let (origin, mock) = serve(|request| match request.path.as_str() {
            "/v1/series/42" => (
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"message": "series unavailable"}),
            ),
            "/v1/my/library/42" => (
                StatusCode::OK,
                json!({"data": {"state": "reading", "progress_chapter": 1}}),
            ),
            _ => (StatusCode::NOT_FOUND, json!({"message": "unexpected"})),
        })
        .await;
        let service = test_service(&origin);

        assert!(matches!(
            service.get_state("alice", "42").await,
            Err(TrackerError::Provider {
                provider: Provider::Mangabaka,
                ..
            })
        ));
        assert!(!mock
            .requests()
            .iter()
            .any(|request| request.path == "/v1/my/library/42"));
    }

    #[tokio::test]
    async fn update_creates_missing_entry_and_maps_all_fields() {
        let (origin, mock) = serve(|request| match request.path.as_str() {
            "/v1/my/library/42" if request.method == Method::GET => (
                StatusCode::NOT_FOUND,
                json!({"message": "not in library", "data": null}),
            ),
            "/v1/my/library/42" => (StatusCode::OK, json!({"data": {}})),
            _ => (StatusCode::NOT_FOUND, json!({"message": "unexpected"})),
        })
        .await;
        let service = test_service(&origin);
        let update = TrackUpdate {
            score: Some(75),
            status: Some(TrackStatus::Planning),
            last_read_chapter: Some(10.9),
            last_read_volume: Some(3),
            start_read_date: Some("2026-10-01".into()),
            finish_read_date: Some("2026-10-20".into()),
        };

        service.update("alice", "42", &update).await.unwrap();
        let requests = mock.requests();
        let post = requests
            .iter()
            .find(|request| request.method == Method::POST)
            .unwrap();
        assert_eq!(post.path, "/v1/my/library/42");
        assert_eq!(post.authorization.as_deref(), Some("Bearer stale"));
        assert_eq!(
            post.json,
            Some(json!({
                "state": "plan_to_read",
                "rating": 75,
                "progress_chapter": 10,
                "progress_volume": 3,
                "start_date": "2026-10-01",
                "finish_date": "2026-10-20"
            }))
        );
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
                .status(Provider::Mangabaka, "alice")
                .unwrap()
                .logged_in
        );
    }

    #[tokio::test]
    async fn update_patches_existing_entry_and_refreshes_once() {
        let (origin, mock) = serve(|request| match request.path.as_str() {
            "/token" => (
                StatusCode::OK,
                json!({"access_token": "fresh", "refresh_token": "refresh-new", "expires_in": 3600}),
            ),
            "/v1/my/library/42" if request.method == Method::GET => (
                StatusCode::OK,
                json!({"data": {"state": "reading"}}),
            ),
            "/v1/my/library/42" if request.authorization.as_deref() == Some("Bearer stale") => {
                (StatusCode::UNAUTHORIZED, json!({"message": "expired"}))
            }
            "/v1/my/library/42" => (StatusCode::OK, json!({"data": {}})),
            _ => (StatusCode::NOT_FOUND, json!({"message": "unexpected"})),
        })
        .await;
        let service = test_service(&origin);
        let update = TrackUpdate {
            score: Some(91),
            status: Some(TrackStatus::Rereading),
            last_read_chapter: Some(5.9),
            start_read_date: Some("1970-01-01".into()),
            finish_read_date: Some("2026-10-20".into()),
            ..TrackUpdate::default()
        };

        service.update("alice", "42", &update).await.unwrap();
        let requests = mock.requests();
        let patches = requests
            .iter()
            .filter(|request| request.method == Method::PATCH)
            .collect::<Vec<_>>();
        assert_eq!(patches.len(), 2);
        assert_eq!(patches[0].authorization.as_deref(), Some("Bearer stale"));
        assert_eq!(patches[1].authorization.as_deref(), Some("Bearer fresh"));
        for request in patches {
            assert_eq!(request.path, "/v1/my/library/42");
            assert_eq!(
                request.json,
                Some(json!({
                    "state": "rereading",
                    "rating": 91,
                    "progress_chapter": 5,
                    "finish_date": "2026-10-20"
                }))
            );
        }
        assert!(!requests.iter().any(|request| {
            request.method == Method::POST && request.path == "/v1/my/library/42"
        }));
        let token = requests
            .iter()
            .find(|request| request.path == "/token")
            .unwrap();
        assert_eq!(token.form["grant_type"], "refresh_token");
        assert_eq!(token.form["refresh_token"], "refresh-old");
    }
}
