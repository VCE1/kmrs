//! Session store: aligned with Spring Session (caffeine when
//! `jdbc-session-enabled=false`, JDBC over `sessions.sqlite` when true).
//! - Cookie name `KOMGA-SESSION`; the `X-Auth-Token` header is also accepted for passing the session id both ways.
//! - Sliding expiration (default 7 days, idle-based): every access refreshes
//!   `LAST_ACCESS_TIME`/`EXPIRY_TIME`, like the `SessionRepositoryFilter` saving
//!   the session at the end of each request.
//! - Session data stores the user id; user info is fetched from the DB each time
//!   (immediate effect of permission changes is handled by explicit invalidation).
//!
//! JDBC mapping notes (see `komga_db::dao::session`): the row mirrors
//! `JdbcIndexedSessionRepository` — distinct `PRIMARY_ID`/`SESSION_ID` UUIDs,
//! millis timestamps, interval in seconds, `EXPIRY_TIME = last access + interval`.
//! `PRINCIPAL_NAME` holds the kmrs user id rather than Java's login email: kmrs
//! invalidation and the actuator `?username=` filter both key off the user id and
//! resolve the email lazily, so observable behavior is identical across backends.
//! A session row without a kmrs payload (e.g. written by Java komga) reads as an
//! unknown session and the user re-authenticates.

use komga_db::dao::session::SessionDao;
use komga_db::pool::Database;
use komga_db::Result;
use moka::sync::Cache;
use std::time::Duration;

pub const SESSION_COOKIE_NAME: &str = "KOMGA-SESSION";
pub const SESSION_HEADER_NAME: &str = "X-Auth-Token";

/// Stored sessions kept per user by the JDBC store; creating one more drops the least
/// recently used. Far above what a user's own devices and browsers hold, it only bounds
/// clients that log in on every request without sending the session back.
const MAX_SESSIONS_PER_USER: usize = 50;

/// Sessions the in-memory store holds in total (a few MB); beyond that the least
/// recently used is evicted and its client logs in again.
const MAX_MEMORY_SESSIONS: u64 = 10_000;

#[derive(Clone)]
pub struct SessionData {
    pub user_id: String,
    /// Epoch millis; tracked for the actuator `/actuator/sessions` descriptors.
    pub created_at: u64,
    pub last_accessed_at: u64,
}

/// The security-context payload persisted in `SPRING_SESSION_ATTRIBUTES`.
#[derive(serde::Serialize, serde::Deserialize)]
struct SessionPayload {
    user_id: String,
}

impl SessionPayload {
    fn encode(user_id: &str) -> Vec<u8> {
        serde_json::to_vec(&SessionPayload {
            user_id: user_id.to_string(),
        })
        .expect("session payload serializes")
    }

    fn decode(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn timeout_secs(timeout: Duration) -> i64 {
    timeout.as_secs().min(i64::MAX as u64) as i64
}

enum StoreInner {
    Memory {
        cache: Cache<String, SessionData>,
    },
    Jdbc {
        dao: SessionDao,
        timeout: Duration,
        max_per_user: usize,
    },
}

#[derive(Clone)]
pub struct SessionStore {
    inner: std::sync::Arc<StoreInner>,
}

impl SessionStore {
    pub fn new(timeout: Duration) -> Self {
        Self::memory(timeout, MAX_MEMORY_SESSIONS)
    }

    fn memory(timeout: Duration, capacity: u64) -> Self {
        Self {
            inner: std::sync::Arc::new(StoreInner::Memory {
                // sliding expiration (time_to_idle = expire after access). LRU rather than
                // moka's default TinyLFU: its admission filter can turn away a brand-new
                // key when the cache is full, which would drop a fresh login's session.
                cache: Cache::builder()
                    .time_to_idle(timeout)
                    .max_capacity(capacity)
                    .eviction_policy(moka::policy::EvictionPolicy::lru())
                    .build(),
            }),
        }
    }

    /// JDBC backend over `sessions.sqlite`, used when `jdbc-session-enabled=true`.
    pub fn jdbc(db: Database, timeout: Duration) -> Self {
        Self::jdbc_with_limit(db, timeout, MAX_SESSIONS_PER_USER)
    }

    fn jdbc_with_limit(db: Database, timeout: Duration, max_per_user: usize) -> Self {
        Self {
            inner: std::sync::Arc::new(StoreInner::Jdbc {
                dao: SessionDao::new(db),
                timeout,
                max_per_user,
            }),
        }
    }

    pub fn create(&self, user_id: &str) -> Result<String> {
        match &*self.inner {
            StoreInner::Memory { cache } => {
                let id = uuid::Uuid::new_v4().to_string();
                let now = now_millis();
                cache.insert(
                    id.clone(),
                    SessionData {
                        user_id: user_id.to_string(),
                        created_at: now,
                        last_accessed_at: now,
                    },
                );
                Ok(id)
            }
            StoreInner::Jdbc {
                dao,
                timeout,
                max_per_user,
            } => {
                let id = uuid::Uuid::new_v4().to_string();
                let primary_id = uuid::Uuid::new_v4().to_string();
                let now = now_millis() as i64;
                dao.insert(
                    &primary_id,
                    &id,
                    now,
                    timeout_secs(*timeout),
                    user_id,
                    &SessionPayload::encode(user_id),
                )?;
                // the new session is the most recently used, so it is always kept; a failed
                // trim only leaves extra rows for the expiry cleanup
                if let Err(e) = dao.delete_least_recently_used(user_id, *max_per_user) {
                    tracing::warn!("failed to trim the sessions of user {user_id}: {e}");
                }
                Ok(id)
            }
        }
    }

    pub fn get(&self, id: &str) -> Result<Option<SessionData>> {
        match &*self.inner {
            StoreInner::Memory { cache } => {
                let Some(data) = cache.get(id) else {
                    return Ok(None);
                };
                let mut data = data;
                data.last_accessed_at = now_millis();
                cache.insert(id.to_string(), data.clone());
                Ok(Some(data))
            }
            // `findById`: an expired row is deleted and reads as missing, a live
            // one has its sliding expiration refreshed.
            StoreInner::Jdbc { dao, timeout, .. } => {
                let Some(row) = dao.find_by_id(id)? else {
                    return Ok(None);
                };
                let now = now_millis() as i64;
                if row.is_expired(now) {
                    dao.delete_by_id(id)?;
                    return Ok(None);
                }
                let Some(payload) = row.payload.as_deref().and_then(SessionPayload::decode) else {
                    return Ok(None);
                };
                dao.touch(id, now, timeout_secs(*timeout))?;
                Ok(Some(SessionData {
                    user_id: payload.user_id,
                    created_at: row.created_at as u64,
                    last_accessed_at: now as u64,
                }))
            }
        }
    }

    /// All live sessions, for the actuator `/actuator/sessions` listing.
    pub fn all(&self) -> Result<Vec<(String, SessionData)>> {
        match &*self.inner {
            StoreInner::Memory { cache } => {
                Ok(cache.iter().map(|(k, v)| ((*k).clone(), v)).collect())
            }
            StoreInner::Jdbc { dao, .. } => Ok(dao
                .list_live(now_millis() as i64)?
                .into_iter()
                .filter_map(|row| {
                    let payload = row.payload.as_deref().and_then(SessionPayload::decode)?;
                    Some((
                        row.id,
                        SessionData {
                            user_id: payload.user_id,
                            created_at: row.created_at as u64,
                            last_accessed_at: row.last_accessed_at as u64,
                        },
                    ))
                })
                .collect()),
        }
    }

    pub fn invalidate(&self, id: &str) -> Result<()> {
        match &*self.inner {
            StoreInner::Memory { cache } => cache.invalidate(id),
            StoreInner::Jdbc { dao, .. } => dao.delete_by_id(id)?,
        }
        Ok(())
    }

    /// Invalidates all sessions of a user (on password change / permission change / user deletion).
    pub fn invalidate_user(&self, user_id: &str) -> Result<()> {
        match &*self.inner {
            StoreInner::Memory { cache } => {
                let ids: Vec<String> = cache
                    .iter()
                    .filter(|(_, v)| v.user_id == user_id)
                    .map(|(k, _)| (*k).clone())
                    .collect();
                for id in ids {
                    cache.invalidate(&id);
                }
            }
            StoreInner::Jdbc { dao, .. } => {
                // like the memory backend, invalidation keys off the stored payload's
                // user id: rows written by Java komga have an undecodable payload
                // and are left alone
                let ids: Vec<String> = dao
                    .list_live(now_millis() as i64)?
                    .into_iter()
                    .filter(|row| {
                        row.payload
                            .as_deref()
                            .and_then(SessionPayload::decode)
                            .is_some_and(|p| p.user_id == user_id)
                    })
                    .map(|row| row.id)
                    .collect();
                for id in ids {
                    dao.delete_by_id(&id)?;
                }
            }
        }
        Ok(())
    }

    /// Hourly expired-session cleanup (Java's `cleanup-cron: "0 0 * * * *"`); a
    /// no-op for the memory backend, whose cache evicts idle entries itself.
    /// Returns the number of rows removed.
    pub fn cleanup_expired(&self) -> Result<u64> {
        match &*self.inner {
            StoreInner::Memory { .. } => Ok(0),
            StoreInner::Jdbc { dao, .. } => dao.delete_expired(now_millis() as i64),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_store() -> SessionStore {
        SessionStore::new(Duration::from_secs(60))
    }

    fn jdbc_store() -> SessionStore {
        let db = Database::open_in_memory(false).unwrap();
        let migrations = komga_db::sessions_migrations();
        komga_db::Migrator::new(&migrations, komga_db::Placeholders::default())
            .migrate(&db.rw().unwrap())
            .unwrap();
        SessionStore::jdbc(db, Duration::from_secs(60))
    }

    fn lifecycle_round_trip(store: &SessionStore) {
        // create + get refreshes the sliding expiration
        let id = store.create("u1").unwrap();
        let first = store.get(&id).unwrap().unwrap();
        assert_eq!(first.user_id, "u1");
        assert!(first.created_at > 0);
        assert!(first.last_accessed_at >= first.created_at);
        assert_eq!(store.get(&id).unwrap().unwrap().user_id, "u1");

        // invalidate kills the session
        store.invalidate(&id).unwrap();
        assert!(store.get(&id).unwrap().is_none());
        store.invalidate(&id).unwrap(); // idempotent

        // all() lists live sessions
        let a = store.create("u1").unwrap();
        let b = store.create("u2").unwrap();
        let mut ids: Vec<String> = store
            .all()
            .unwrap()
            .iter()
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        let mut expected = vec![a, b];
        expected.sort();
        assert_eq!(ids, expected);
    }

    #[test]
    fn memory_lifecycle_round_trip() {
        lifecycle_round_trip(&memory_store());
    }

    #[test]
    fn jdbc_lifecycle_round_trip() {
        lifecycle_round_trip(&jdbc_store());
    }

    #[test]
    fn jdbc_sessions_survive_store_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sessions.sqlite");
        let config = komga_db::pool::DatabaseConfig {
            file: file.clone(),
            ..Default::default()
        };
        let open = || {
            let db = Database::open(&config).unwrap();
            let migrations = komga_db::sessions_migrations();
            komga_db::Migrator::new(&migrations, komga_db::Placeholders::default())
                .migrate(&db.rw().unwrap())
                .unwrap();
            SessionStore::jdbc(db, Duration::from_secs(60))
        };

        let store = open();
        let id = store.create("u1").unwrap();
        drop(store);

        // a new store over the same file sees the session
        let store = open();
        let session = store.get(&id).unwrap().unwrap();
        assert_eq!(session.user_id, "u1");
    }

    #[test]
    fn jdbc_expired_sessions_read_as_missing_and_are_cleaned_up() {
        let db = Database::open_in_memory(false).unwrap();
        let migrations = komga_db::sessions_migrations();
        komga_db::Migrator::new(&migrations, komga_db::Placeholders::default())
            .migrate(&db.rw().unwrap())
            .unwrap();
        // a 1ns timeout expires the session before the next read
        let store = SessionStore::jdbc(db.clone(), Duration::from_nanos(1));
        let id = store.create("u1").unwrap();
        assert!(store.get(&id).unwrap().is_none());
        // the expired row is gone (deleted on read); cleanup finds nothing
        assert_eq!(store.cleanup_expired().unwrap(), 0);
        assert!(store.all().unwrap().is_empty());

        // an expired row never read is removed by the hourly cleanup instead
        let dao = komga_db::dao::session::SessionDao::new(db);
        let past = now_millis() as i64 - 60_000;
        dao.insert(
            &uuid::Uuid::new_v4().to_string(),
            "stale",
            past,
            30,
            "u1",
            &SessionPayload::encode("u1"),
        )
        .unwrap();
        assert_eq!(store.cleanup_expired().unwrap(), 1);
        assert!(store.all().unwrap().is_empty());
        assert_eq!(store.cleanup_expired().unwrap(), 0);
    }

    /// Timestamps are millis: step past one between operations so the order is real.
    fn tick() {
        std::thread::sleep(Duration::from_millis(3));
    }

    #[test]
    fn jdbc_keeps_the_most_recently_used_sessions_per_user() {
        let db = Database::open_in_memory(false).unwrap();
        let migrations = komga_db::sessions_migrations();
        komga_db::Migrator::new(&migrations, komga_db::Placeholders::default())
            .migrate(&db.rw().unwrap())
            .unwrap();
        let store = SessionStore::jdbc_with_limit(db, Duration::from_secs(60), 3);

        let other = store.create("u2").unwrap();
        let s1 = store.create("u1").unwrap();
        tick();
        let s2 = store.create("u1").unwrap();
        tick();
        let s3 = store.create("u1").unwrap();
        tick();
        // using s1 makes s2 the least recently used
        store.get(&s1).unwrap().unwrap();
        tick();
        let s4 = store.create("u1").unwrap();

        assert!(store.get(&s2).unwrap().is_none());
        for id in [&s1, &s3, &s4, &other] {
            assert!(store.get(id).unwrap().is_some(), "{id} should be kept");
        }
        assert_eq!(store.all().unwrap().len(), 4);
    }

    #[test]
    fn memory_store_is_bounded_and_admits_new_sessions() {
        let store = SessionStore::memory(Duration::from_secs(60), 2);
        let s1 = store.create("u1").unwrap();
        let s2 = store.create("u1").unwrap();
        // using s1 makes s2 the least recently used
        store.get(&s1).unwrap().unwrap();
        let s3 = store.create("u2").unwrap();

        let StoreInner::Memory { cache } = &*store.inner else {
            unreachable!()
        };
        cache.run_pending_tasks();
        assert_eq!(cache.entry_count(), 2);
        // a fresh login is never turned away, and the least recently used one goes
        assert!(store.get(&s3).unwrap().is_some());
        assert!(store.get(&s1).unwrap().is_some());
        assert!(store.get(&s2).unwrap().is_none());
    }
}
