//! Tracker sync, a kmrs-private enhancement with no Java equivalent: pushes
//! reading progress to the current user's tracker accounts (AniList / MAL /
//! Bangumi / MangaBaka) whenever a book is read. Every decision rule lives in
//! `komga_core::tracker` (pure, unit-tested); this module only wires events,
//! database reads, and provider calls together.
//!
//! Progress only ever moves forward: unread/delete events never regress the
//! platform entry. Bindings belong to one user and only that user's reading
//! advances them.

use crate::events::DomainEvent;
use crate::service::tracker::{Provider, TrackerServices};
use crate::state::AppState;
use komga_core::tracker::{self, BookTrackInput};
use komga_db::dao::book::{BookDao, BookMetadataDao};
use komga_db::dao::read_progress::ReadProgressDao;
use komga_db::dao::tracker_link::{TrackerLink, TrackerLinkDao};

/// Serializes concurrent syncs of the same (series, user) pair. Pushing is a
/// GET → decide → POST sequence against komf, and overlapping sequences
/// reorder: two chapters completing in quick succession would both read the
/// same remote progress and the earlier chapter could POST last, regressing
/// the remote value. Holding one lock per key for the whole sync keeps every
/// sequence atomic; distinct pairs still run concurrently.
type SyncKey = (String, String);

fn sync_locks() -> &'static std::sync::Mutex<
    std::collections::HashMap<SyncKey, std::sync::Arc<tokio::sync::Mutex<()>>>,
> {
    static LOCKS: std::sync::OnceLock<
        std::sync::Mutex<
            std::collections::HashMap<SyncKey, std::sync::Arc<tokio::sync::Mutex<()>>>,
        >,
    > = std::sync::OnceLock::new();
    LOCKS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

async fn run_serialized<R>(key: &SyncKey, fut: impl std::future::Future<Output = R>) -> R {
    let slot = {
        let mut locks = sync_locks().lock().unwrap_or_else(|e| e.into_inner());
        locks.entry(key.clone()).or_default().clone()
    };
    let _guard = slot.lock().await;
    let result = fut.await;
    drop(_guard);
    // drop the slot when nobody else holds it so the registry can't grow with
    // one entry per synced series
    let mut locks = sync_locks().lock().unwrap_or_else(|e| e.into_inner());
    if std::sync::Arc::strong_count(&slot) == 2 {
        locks.remove(key);
    }
    result
}

/// Feeds on a lossless tap of the event bus and syncs series with tracker bindings
/// when their read progress changes. A drained burst is coalesced to one sync per
/// (series, user): a sync re-reads the current progress from the database anyway,
/// so intermediate events of the same pair would only repeat the same komf round
/// trip. `ReadProgressDeleted`/series-deleted events intentionally do nothing:
/// platform progress never regresses.
pub fn consume_events(
    state: AppState,
    mut events: tokio::sync::mpsc::UnboundedReceiver<DomainEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            // all senders are gone (shutdown)
            let Some(event) = events.recv().await else {
                break;
            };
            let mut batch = vec![event];
            while let Ok(event) = events.try_recv() {
                batch.push(event);
            }
            for (series_id, user_id) in sync_pairs(&state, batch).await {
                let state = state.clone();
                tokio::spawn(async move {
                    run_serialized(
                        &(series_id.clone(), user_id.clone()),
                        sync_series(&state, series_id, user_id),
                    )
                    .await;
                });
            }
        }
    })
}

/// A drained burst resolved to the distinct (series, user) pairs worth syncing.
async fn sync_pairs(state: &AppState, batch: Vec<DomainEvent>) -> Vec<(String, String)> {
    let state = state.clone();
    tokio::task::spawn_blocking(move || {
        let book_dao = BookDao::new(state.db.clone());
        let mut pairs = std::collections::BTreeSet::new();
        for event in batch {
            match event {
                DomainEvent::ReadProgressChanged(progress) if progress.completed => {
                    // a book already gone here (deleted between the progress write
                    // and the event) leaves nothing to sync
                    match book_dao.get_series_id_or_null(&progress.book_id) {
                        Ok(Some(series_id)) => {
                            pairs.insert((series_id, progress.user_id));
                        }
                        Ok(None) => {}
                        Err(e) => tracing::warn!(
                            "tracker sync: failed to resolve book {}: {e}",
                            progress.book_id
                        ),
                    }
                }
                DomainEvent::ReadProgressSeriesChanged { series_id, user_id } => {
                    pairs.insert((series_id, user_id));
                }
                _ => {}
            }
        }
        pairs.into_iter().collect()
    })
    .await
    .unwrap_or_else(|e| {
        tracing::error!("tracker sync: event task failed: {e}");
        vec![]
    })
}

/// Everything the decision needs, gathered in one blocking pass over the main
/// database. Book inputs own their names so the snapshot can outlive the DAO
/// rows; `BookTrackInput` views are built per decision call.
struct SeriesSnapshot {
    links: Vec<TrackerLink>,
    books: Vec<OwnedBookInput>,
}

struct OwnedBookInput {
    name: String,
    number_sort: f32,
    read: bool,
}

fn gather(
    state: &AppState,
    series_id: &str,
    user_id: &str,
) -> komga_db::Result<Option<SeriesSnapshot>> {
    let links =
        TrackerLinkDao::new(state.kmrs_db.clone()).list_by_series_and_user(series_id, user_id)?;
    if links.is_empty() {
        return Ok(None);
    }
    let book_dao = BookDao::new(state.db.clone());
    let books = book_dao.find_by_series_id(series_id)?;
    if books.is_empty() {
        return Ok(None);
    }
    let book_ids: Vec<String> = books.iter().map(|b| b.id.clone()).collect();
    let number_sorts: std::collections::HashMap<String, f32> =
        BookMetadataDao::new(state.db.clone())
            .find_number_sort_by_book_ids(&book_ids)?
            .into_iter()
            .collect();
    let read: std::collections::HashSet<String> = ReadProgressDao::new(state.db.clone())
        .find_by_books_and_user(&book_ids, user_id)?
        .into_iter()
        .filter(|p| p.completed)
        .map(|p| p.book_id)
        .collect();
    let books = books
        .into_iter()
        .map(|b| OwnedBookInput {
            name: b.name,
            number_sort: number_sorts.get(&b.id).copied().unwrap_or(0.0),
            read: read.contains(&b.id),
        })
        .collect();
    Ok(Some(SeriesSnapshot { links, books }))
}

/// Syncs every tracker binding of one (series, user) pair. Failures are logged
/// and never propagated: reading must never fail because a tracker is down.
async fn sync_series(state: &AppState, series_id: String, user_id: String) {
    let snapshot = match tokio::task::spawn_blocking({
        let state = state.clone();
        let series_id = series_id.clone();
        let user_id = user_id.clone();
        move || gather(&state, &series_id, &user_id)
    })
    .await
    {
        Ok(Ok(Some(snapshot))) => snapshot,
        Ok(Ok(None)) => return,
        Ok(Err(e)) => {
            tracing::warn!("tracker sync: failed to gather series {series_id}: {e}");
            return;
        }
        Err(e) => {
            tracing::warn!("tracker sync: gather task failed: {e}");
            return;
        }
    };

    let services = state.tracker.clone();
    let today = komga_core::time_codec::now_utc().date();
    let books = snapshot
        .books
        .iter()
        .map(|b| BookTrackInput {
            name: &b.name,
            number_sort: b.number_sort,
            read: b.read,
        })
        .collect::<Vec<_>>();
    for link in &snapshot.links {
        if let Err(e) = push_link(&services, link, &books, today).await {
            tracing::warn!(
                "tracker sync: {}:{} for user {} failed: {e:#}",
                link.provider,
                link.track_id,
                link.user_id
            );
        }
    }
}

/// Fetches the platform state, decides, and pushes when there is something to
/// push. "Nothing to push" is `Ok(())`.
async fn push_link(
    services: &TrackerServices,
    link: &TrackerLink,
    books: &[BookTrackInput<'_>],
    today: time::Date,
) -> anyhow::Result<()> {
    let provider = Provider::parse(&link.provider)
        .ok_or_else(|| anyhow::anyhow!("unsupported tracker provider {}", link.provider))?;
    let service = services
        .get(provider)
        .ok_or_else(|| anyhow::anyhow!("tracker provider {} is unavailable", link.provider))?;
    let remote = service.get_state(&link.user_id, &link.track_id).await?;
    let mode = tracker::effective_mode(link.track_mode, books);
    let Some(update) = tracker::decide(mode, link.chapter_offset, &remote, books, today) else {
        return Ok(());
    };
    service
        .update(&link.user_id, &link.track_id, &update)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::collections::tests as shared;
    use crate::service::tracker::{TrackerError, TrackerService};
    use komga_core::model::read_progress::ReadProgress;
    use komga_core::tracker::{RemoteState, TrackMode, TrackSearchItem, TrackStatus, TrackUpdate};
    use std::sync::{Arc, Mutex as StdMutex};

    /// Two pushes racing on the same (series, user) must stay ordered: each
    /// task's GET happens strictly before its POST, otherwise the earlier
    /// chapter could POST last and regress the remote value. The sleep stands
    /// in for the provider round trip and forces the interleaving.
    #[tokio::test]
    async fn same_key_sequences_never_interleave() {
        let events: Arc<StdMutex<Vec<(&'static str, &'static str)>>> =
            Arc::new(StdMutex::new(Vec::new()));
        let key = ("series-1".to_string(), "user-1".to_string());
        let run = |tag: &'static str| {
            let events = events.clone();
            let key = key.clone();
            async move {
                run_serialized(&key, async move {
                    events.lock().unwrap().push(("get", tag));
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    events.lock().unwrap().push(("post", tag));
                })
                .await
            }
        };
        tokio::join!(run("one"), run("two"));
        let order: Vec<_> = events.lock().unwrap().iter().map(|(op, _)| *op).collect();
        assert_eq!(order, vec!["get", "post", "get", "post"]);
    }

    /// Distinct (series, user) pairs must not block each other. The barrier
    /// forces both GETs before either POST; a global lock would deadlock
    /// them into get,post,get,post instead.
    #[tokio::test]
    async fn distinct_keys_run_concurrently() {
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        let events: Arc<StdMutex<Vec<&'static str>>> = Arc::new(StdMutex::new(Vec::new()));
        let run = |key: SyncKey, barrier: Arc<tokio::sync::Barrier>| {
            let events = events.clone();
            async move {
                run_serialized(&key, async move {
                    events.lock().unwrap().push("get");
                    barrier.wait().await;
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    events.lock().unwrap().push("post");
                })
                .await
            }
        };
        let key_a = ("series-1".to_string(), "user-1".to_string());
        let key_b = ("series-2".to_string(), "user-1".to_string());
        tokio::join!(run(key_a, barrier.clone()), run(key_b, barrier));
        assert_eq!(*events.lock().unwrap(), vec!["get", "get", "post", "post"]);
    }

    /// Finished slots are evicted so the registry can't grow with one entry
    /// per synced series.
    #[tokio::test]
    async fn finished_slots_are_evicted() {
        let key = ("evict-series".to_string(), "evict-user".to_string());
        run_serialized(&key, async {}).await;
        let locks = sync_locks().lock().unwrap_or_else(|e| e.into_inner());
        assert!(!locks.contains_key(&key));
    }

    struct StubTracker {
        remote: RemoteState,
        updates: Arc<StdMutex<Vec<(String, String, TrackUpdate)>>>,
    }

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
            _title_language: Option<&str>,
        ) -> Result<Vec<TrackSearchItem>, TrackerError> {
            Ok(vec![])
        }

        async fn get_state(
            &self,
            _user_id: &str,
            _track_id: &str,
        ) -> Result<RemoteState, TrackerError> {
            Ok(self.remote.clone())
        }

        async fn update(
            &self,
            user_id: &str,
            track_id: &str,
            update: &TrackUpdate,
        ) -> Result<(), TrackerError> {
            self.updates.lock().unwrap().push((
                user_id.to_string(),
                track_id.to_string(),
                update.clone(),
            ));
            Ok(())
        }
    }

    #[tokio::test]
    async fn push_link_uses_native_provider() {
        let updates = Arc::new(StdMutex::new(Vec::new()));
        let oauth = crate::service::tracker::oauth::OAuthManager::new(
            crate::state::test_kmrs_db(),
            reqwest::Client::new(),
        );
        let services = TrackerServices::new(
            oauth,
            vec![Arc::new(StubTracker {
                remote: RemoteState {
                    status: Some(TrackStatus::Reading),
                    last_read_chapter: Some(10.0),
                    total_chapters: Some(12),
                    ..RemoteState::default()
                },
                updates: updates.clone(),
            })],
        );
        let now = komga_core::time_codec::now_utc();
        let link = TrackerLink {
            series_id: "s1".into(),
            user_id: "u1".into(),
            provider: "anilist".into(),
            track_id: "42".into(),
            title: None,
            track_mode: TrackMode::Chapter,
            chapter_offset: 0,
            created_date: now,
            last_modified_date: now,
        };
        let books = [BookTrackInput {
            name: "ch.11",
            number_sort: 11.0,
            read: true,
        }];
        push_link(
            &services,
            &link,
            &books,
            time::Date::from_calendar_date(2026, time::Month::October, 25).unwrap(),
        )
        .await
        .unwrap();

        let updates = updates.lock().unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].0, "u1");
        assert_eq!(updates[0].1, "42");
        assert_eq!(updates[0].2.last_read_chapter, Some(11.0));
        assert_eq!(updates[0].2.status, None);
    }

    fn read_progress(book_id: &str, user_id: &str, completed: bool) -> ReadProgress {
        ReadProgress {
            book_id: book_id.into(),
            user_id: user_id.into(),
            page: 1,
            completed,
            read_date: komga_core::time_codec::now_utc(),
            device_id: String::new(),
            device_name: String::new(),
            locator: None,
            created_date: komga_core::time_codec::now_utc(),
            last_modified_date: komga_core::time_codec::now_utc(),
        }
    }

    fn progress_event(book_id: &str, user_id: &str, completed: bool) -> DomainEvent {
        DomainEvent::ReadProgressChanged(read_progress(book_id, user_id, completed))
    }

    #[tokio::test]
    async fn sync_series_works_without_metadata_integration() {
        let mut state = shared::test_state();
        shared::seed_base(&state.db);
        let user_id = shared::insert_user(
            &state.db,
            "reader@x.c",
            &[],
            &[],
            Default::default(),
            "reader-key",
        );
        let updates = Arc::new(StdMutex::new(Vec::new()));
        let oauth = state.tracker.oauth().clone();
        state.tracker = Arc::new(TrackerServices::new(
            oauth,
            vec![Arc::new(StubTracker {
                remote: RemoteState {
                    status: Some(TrackStatus::Reading),
                    last_read_chapter: Some(1.0),
                    total_chapters: Some(3),
                    ..RemoteState::default()
                },
                updates: updates.clone(),
            })],
        ));
        TrackerLinkDao::new(state.kmrs_db.clone())
            .upsert(&komga_db::dao::tracker_link::NewTrackerLink {
                series_id: "s1",
                user_id: &user_id,
                provider: "anilist",
                track_id: "42",
                title: Some("Series"),
                track_mode: TrackMode::Chapter,
                chapter_offset: 0,
            })
            .unwrap();
        ReadProgressDao::new(state.db.clone())
            .save_many(&[
                read_progress("b1", &user_id, true),
                read_progress("b2", &user_id, true),
            ])
            .unwrap();

        sync_series(&state, "s1".into(), user_id.clone()).await;

        let updates = updates.lock().unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].0, user_id);
        assert_eq!(updates[0].1, "42");
        assert_eq!(updates[0].2.last_read_chapter, Some(2.0));
    }

    #[tokio::test]
    async fn sync_pairs_coalesce_to_one_per_series_and_user() {
        let state = shared::test_state();
        shared::seed_base(&state.db);
        let batch = vec![
            progress_event("b1", "u1", true),
            progress_event("b1", "u1", true),
            // b2 belongs to the same series as b1
            progress_event("b2", "u1", true),
            progress_event("b3", "u2", true),
            progress_event("b1", "u3", false),
            // deleted between the progress write and the event
            progress_event("gone", "u4", true),
            DomainEvent::ReadProgressSeriesChanged {
                series_id: "s9".into(),
                user_id: "u1".into(),
            },
            DomainEvent::ReadProgressSeriesChanged {
                series_id: "s9".into(),
                user_id: "u1".into(),
            },
        ];
        let pairs = sync_pairs(&state, batch).await;
        assert_eq!(
            pairs,
            vec![
                ("s1".to_string(), "u1".to_string()),
                ("s2".to_string(), "u2".to_string()),
                ("s9".to_string(), "u1".to_string()),
            ]
        );
    }
}
