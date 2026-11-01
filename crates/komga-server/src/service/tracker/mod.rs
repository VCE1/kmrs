//! Native tracker contracts and per-user provider dispatch.

pub mod oauth;
pub mod providers;

use komga_core::tracker::{RemoteState, TrackSearchItem, TrackUpdate};
use komga_db::pool::Database;
use oauth::OAuthManager;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Anilist,
    Mal,
    Bangumi,
    Mangabaka,
}

impl Provider {
    pub const ALL: [Self; 4] = [Self::Anilist, Self::Mal, Self::Bangumi, Self::Mangabaka];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anilist => "anilist",
            Self::Mal => "mal",
            Self::Bangumi => "bangumi",
            Self::Mangabaka => "mangabaka",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "anilist" => Some(Self::Anilist),
            "mal" => Some(Self::Mal),
            "bangumi" => Some(Self::Bangumi),
            "mangabaka" => Some(Self::Mangabaka),
            _ => None,
        }
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TrackerError {
    #[error("{provider} tracker: not logged in")]
    NotLoggedIn { provider: Provider },
    #[error("{provider} tracker: {message}")]
    Provider { provider: Provider, message: String },
    #[error("{provider} OAuth endpoint returned {status}: {message}")]
    Upstream {
        provider: Provider,
        status: reqwest::StatusCode,
        message: String,
    },
    #[error(transparent)]
    Database(#[from] komga_db::Error),
    #[error("{provider} OAuth client id is not configured at build time ({variable})")]
    MissingClientId {
        provider: Provider,
        variable: &'static str,
    },
    #[error("{provider} OAuth client secret is not configured at build time ({variable})")]
    MissingClientSecret {
        provider: Provider,
        variable: &'static str,
    },
    #[error("invalid OAuth state")]
    InvalidState,
    #[error("OAuth authorization session is missing or expired")]
    PendingExpired,
}

#[async_trait::async_trait]
pub trait TrackerService: Send + Sync {
    fn provider(&self) -> Provider;
    fn is_logged_in(&self, user_id: &str) -> Result<bool, TrackerError>;

    async fn search(
        &self,
        user_id: &str,
        query: &str,
        nsfw: bool,
        language: Option<&str>,
    ) -> Result<Vec<TrackSearchItem>, TrackerError>;

    async fn get_state(&self, user_id: &str, track_id: &str) -> Result<RemoteState, TrackerError>;

    async fn update(
        &self,
        user_id: &str,
        track_id: &str,
        update: &TrackUpdate,
    ) -> Result<(), TrackerError>;
}

#[derive(Clone)]
pub struct TrackerServices {
    oauth: Arc<OAuthManager>,
    services: Arc<HashMap<Provider, Arc<dyn TrackerService>>>,
}

impl TrackerServices {
    pub fn new(oauth: Arc<OAuthManager>, services: Vec<Arc<dyn TrackerService>>) -> Self {
        let services = services
            .into_iter()
            .map(|service| (service.provider(), service))
            .collect();
        Self {
            oauth,
            services: Arc::new(services),
        }
    }

    pub fn from_db(db: Database) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(concat!("kmrs/", env!("CARGO_PKG_VERSION")))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("tracker HTTP client");
        let oauth = OAuthManager::new(db, http.clone());
        let services: Vec<Arc<dyn TrackerService>> = vec![
            Arc::new(providers::anilist::AniListTracker::new(
                http.clone(),
                oauth.clone(),
            )),
            Arc::new(providers::mal::MalTracker::new(http.clone(), oauth.clone())),
            Arc::new(providers::bangumi::BangumiTracker::new(
                http.clone(),
                oauth.clone(),
            )),
            Arc::new(providers::mangabaka::MangaBakaTracker::new(
                http,
                oauth.clone(),
            )),
        ];
        Self::new(oauth, services)
    }

    pub fn empty(db: Database) -> Self {
        Self::new(OAuthManager::new(db, reqwest::Client::new()), Vec::new())
    }

    pub fn oauth(&self) -> &Arc<OAuthManager> {
        &self.oauth
    }

    pub fn get(&self, provider: Provider) -> Option<Arc<dyn TrackerService>> {
        self.services.get(&provider).cloned()
    }

    pub fn all(&self) -> Vec<Arc<dyn TrackerService>> {
        Provider::ALL
            .into_iter()
            .filter_map(|provider| self.get(provider))
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.services.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubTracker;

    #[async_trait::async_trait]
    impl TrackerService for StubTracker {
        fn provider(&self) -> Provider {
            Provider::Anilist
        }

        fn is_logged_in(&self, _user_id: &str) -> Result<bool, TrackerError> {
            Ok(true)
        }

        async fn search(
            &self,
            _user_id: &str,
            _query: &str,
            _nsfw: bool,
            _language: Option<&str>,
        ) -> Result<Vec<TrackSearchItem>, TrackerError> {
            Ok(Vec::new())
        }

        async fn get_state(
            &self,
            _user_id: &str,
            _track_id: &str,
        ) -> Result<RemoteState, TrackerError> {
            Ok(RemoteState::default())
        }

        async fn update(
            &self,
            _user_id: &str,
            _track_id: &str,
            _update: &TrackUpdate,
        ) -> Result<(), TrackerError> {
            Ok(())
        }
    }

    #[test]
    fn provider_names_match_existing_routes() {
        for provider in Provider::ALL {
            assert_eq!(Provider::parse(provider.as_str()), Some(provider));
            assert_eq!(
                serde_json::to_value(provider).unwrap(),
                serde_json::Value::String(provider.as_str().into())
            );
        }
        for alias in ["myanimelist", "bgm", "mb", "AniList", "unknown"] {
            assert_eq!(Provider::parse(alias), None);
        }
        let error = TrackerError::NotLoggedIn {
            provider: Provider::Anilist,
        };
        assert_eq!(error.to_string(), "anilist tracker: not logged in");
    }

    #[tokio::test]
    async fn registry_is_clonable_and_dispatches_object_safely() {
        let empty = TrackerServices::empty(Database::open_in_memory(false).unwrap());
        assert!(empty.is_empty());
        assert!(empty.get(Provider::Anilist).is_none());

        let oauth = OAuthManager::new(
            Database::open_in_memory(false).unwrap(),
            reqwest::Client::new(),
        );
        let services = TrackerServices::new(oauth.clone(), vec![Arc::new(StubTracker)]);
        assert!(Arc::ptr_eq(services.oauth(), &oauth));
        let cloned = services.clone();
        let tracker = cloned.get(Provider::Anilist).unwrap();
        assert!(tracker.is_logged_in("alice").unwrap());
        assert!(tracker
            .search("alice", "title", true, None)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            tracker.get_state("alice", "42").await.unwrap(),
            RemoteState::default()
        );
        tracker
            .update("alice", "42", &TrackUpdate::default())
            .await
            .unwrap();
        assert_eq!(services.all().len(), 1);
        assert!(services.get(Provider::Mal).is_none());
    }
}
