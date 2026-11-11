//! `LibraryLifecycle.kt`: library creation/update/deletion with validity checks.

use crate::events::DomainEvent;
use crate::state::AppState;
use komga_core::model::library::Library;
use komga_core::task::{AnalyzeBook, Task, DEFAULT_PRIORITY, LOWEST_PRIORITY};
use komga_db::dao::library::LibraryDao;
use komga_db::dao::series::SeriesDao;
use komga_db::dao::sidecar::SidecarDao;
use std::path::PathBuf;

/// The four validation failures of `checkLibraryValidity`; each carries the exact message
/// the Kotlin exception produces (controllers map them to 400).
#[derive(Debug)]
pub enum LibraryError {
    FileNotFound(String),
    DirectoryNotFound(String),
    DuplicateName(String),
    PathContainedInPath(String),
    Db(komga_db::Error),
}

impl LibraryError {
    pub fn message(&self) -> String {
        match self {
            LibraryError::FileNotFound(m)
            | LibraryError::DirectoryNotFound(m)
            | LibraryError::DuplicateName(m)
            | LibraryError::PathContainedInPath(m) => m.clone(),
            LibraryError::Db(e) => e.to_string(),
        }
    }

    pub fn is_validation(&self) -> bool {
        !matches!(self, LibraryError::Db(_))
    }
}

impl From<komga_db::Error> for LibraryError {
    fn from(e: komga_db::Error) -> Self {
        LibraryError::Db(e)
    }
}

pub fn add_library(state: &AppState, library: &Library) -> Result<Library, LibraryError> {
    tracing::info!(
        "Adding new library: {} with root folder: {}",
        library.name,
        library.root
    );
    let dao = LibraryDao::new(state.db.clone());
    let existing = dao.find_all()?;
    check_library_validity(library, &existing)?;

    let id = dao.insert(library)?;
    let created = dao
        .find_by_id(&id)?
        .expect("library not found after insert");

    state
        .task_emitter
        .scan_library(&created.id, false, DEFAULT_PRIORITY)?;

    let _ = state
        .events
        .send(DomainEvent::LibraryAdded(created.clone()));
    Ok(created)
}

pub fn update_library(state: &AppState, to_update: &Library) -> Result<(), LibraryError> {
    tracing::info!("Updating library: {}", to_update.id);
    let dao = LibraryDao::new(state.db.clone());
    let libraries = dao.find_all()?;
    let current = libraries
        .iter()
        .find(|l| l.id == to_update.id)
        .cloned()
        .expect("library to update does not exist");
    let others: Vec<Library> = libraries
        .into_iter()
        .filter(|l| l.id != to_update.id)
        .collect();
    check_library_validity(to_update, &others)?;

    dao.update(to_update)?;

    if current.scan_interval != to_update.scan_interval {
        crate::service::scheduler::ScanScheduler::schedule_scan(state, to_update);
    }

    if check_library_should_rescan(&current, to_update) {
        state
            .task_emitter
            .scan_library(&to_update.id, false, DEFAULT_PRIORITY)?;
    }
    if to_update.hash_files && !current.hash_files {
        state.task_emitter.hash_books_without_hash(to_update)?;
    }
    if to_update.hash_koreader && !current.hash_koreader {
        state
            .task_emitter
            .hash_books_without_hash_koreader(to_update)?;
    }
    if to_update.hash_pages && !current.hash_pages {
        state
            .task_emitter
            .find_books_with_missing_page_hash(&to_update.id, LOWEST_PRIORITY)?;
    }
    if to_update.repair_extensions && !current.repair_extensions {
        repair_extensions(state, to_update)?;
    }
    if to_update.convert_to_cbz && !current.convert_to_cbz {
        state
            .task_emitter
            .find_books_to_convert(&to_update.id, LOWEST_PRIORITY)?;
    }

    let _ = state
        .events
        .send(DomainEvent::LibraryUpdated(to_update.clone()));
    Ok(())
}

fn check_library_should_rescan(existing: &Library, updated: &Library) -> bool {
    existing.root != updated.root
        || existing.oneshots_directory != updated.oneshots_directory
        || existing.scan_cbx != updated.scan_cbx
        || existing.scan_pdf != updated.scan_pdf
        || existing.scan_epub != updated.scan_epub
        || existing.scan_force_modified_time != updated.scan_force_modified_time
        || existing.scan_directory_exclusions != updated.scan_directory_exclusions
}

fn check_library_validity(library: &Library, existing: &[Library]) -> Result<(), LibraryError> {
    let path = PathBuf::from(komga_core::dto::url_to_file_path(&library.root));
    if !path.exists() {
        return Err(LibraryError::FileNotFound(format!(
            "Library root folder does not exist: {}",
            library.root
        )));
    }
    if !path.is_dir() {
        return Err(LibraryError::DirectoryNotFound(format!(
            "Library root folder is not a folder: {}",
            library.root
        )));
    }
    if existing.iter().any(|l| l.name == library.name) {
        return Err(LibraryError::DuplicateName(
            "Library name already exists".to_string(),
        ));
    }
    for other in existing {
        let other_path = PathBuf::from(komga_core::dto::url_to_file_path(&other.root));
        if path.starts_with(&other_path) {
            return Err(LibraryError::PathContainedInPath(format!(
                "Library path {} is a child of existing library {}: {}",
                path.display(),
                other.name,
                other_path.display()
            )));
        }
        if other_path.starts_with(&path) {
            return Err(LibraryError::PathContainedInPath(format!(
                "Library path {} is a parent of existing library {}: {}",
                path.display(),
                other.name,
                other_path.display()
            )));
        }
    }
    Ok(())
}

/// `TaskEmitter.repairExtensions`: one repair task per book whose extension does not match
/// its media type (BookConverter.mediaTypeToExtension). The handler side lands with the
/// converter in M8.
fn repair_extensions(state: &AppState, library: &Library) -> komga_db::Result<()> {
    const MEDIA_TYPE_TO_EXTENSION: [(&str, &str); 5] = [
        ("application/x-rar-compressed; version=4", "cbr"),
        ("application/x-rar-compressed; version=5", "cbr"),
        ("application/zip", "cbz"),
        ("application/pdf", "pdf"),
        ("application/epub+zip", "epub"),
    ];
    let conn = state.db.ro()?;
    let mut tasks = vec![];
    for (media_type, extension) in MEDIA_TYPE_TO_EXTENSION {
        let mut stmt = conn.prepare(
            "SELECT BOOK.ID, BOOK.SERIES_ID FROM BOOK \
             LEFT JOIN MEDIA ON BOOK.ID = MEDIA.BOOK_ID \
             WHERE BOOK.LIBRARY_ID = ? AND MEDIA.MEDIA_TYPE = ? AND BOOK.URL NOT LIKE ?",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![library.id, media_type, format!("%.{extension}")],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )?;
        for row in rows {
            let (book_id, series_id) = row?;
            tasks.push(Task::RepairExtension(AnalyzeBook {
                book_id,
                priority: LOWEST_PRIORITY,
                group_id: Some(series_id),
                unique_id: String::new(),
            }));
        }
    }
    state.task_emitter.submit_many(&tasks)
}

/// Create a missing library root only when its parent already exists.
/// Returns whether this call created the directory. Never creates more than one level,
/// and never accepts a symlink as the root.
pub fn prepare_library_root(root: &str) -> Result<bool, LibraryError> {
    let path = PathBuf::from(root.trim().trim_end_matches('/'));
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return Err(LibraryError::FileNotFound(
            "库路径必须是绝对路径".to_string(),
        ));
    }
    match std::fs::symlink_metadata(&path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(LibraryError::DirectoryNotFound(
                    "库根目录不能是软链接，请选择已经挂载的真实目录".to_string(),
                ));
            }
            if !meta.is_dir() {
                return Err(LibraryError::DirectoryNotFound(format!(
                    "库根目录不是文件夹: {}",
                    path.display()
                )));
            }
            Ok(false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty())
            else {
                return Err(LibraryError::FileNotFound(
                    "不能创建盘符根目录".to_string(),
                ));
            };
            let parent_meta = std::fs::symlink_metadata(parent).map_err(|_| {
                LibraryError::FileNotFound(format!(
                    "上级目录不存在，只创建最后一级，不会全盘扫描: {}",
                    parent.display()
                ))
            })?;
            if parent_meta.file_type().is_symlink() || !parent_meta.is_dir() {
                return Err(LibraryError::DirectoryNotFound(
                    "上级路径不是真实目录，没有创建文件夹".to_string(),
                ));
            }
            std::fs::create_dir(&path).map_err(|error| {
                LibraryError::FileNotFound(format!(
                    "无法创建书架目录 {}: {error}",
                    path.display()
                ))
            })?;
            Ok(true)
        }
        Err(error) => Err(LibraryError::FileNotFound(format!(
            "无法读取库路径 {}: {error}",
            path.display()
        ))),
    }
}

pub fn delete_library(state: &AppState, library: &Library) -> komga_db::Result<()> {
    tracing::info!("Deleting library: {library:?}");
    let series = SeriesDao::new(state.db.clone()).find_by_library_id(&library.id)?;
    crate::service::series::delete_many(state, &series)?;
    SidecarDao::new(state.db.clone()).delete_by_library_id(&library.id)?;
    LibraryDao::new(state.db.clone()).delete(&library.id)?;

    let _ = state
        .events
        .send(DomainEvent::LibraryDeleted(library.clone()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::libraries::test_support::TestApp;
    use komga_core::model::library::{ScanInterval, SeriesCover};
    use komga_core::time_codec::now_utc;
    use komga_db::dao::tasks::TasksDao;
    use std::path::Path;

    fn app() -> TestApp {
        TestApp::new(crate::api::libraries::router())
    }

    fn library(name: &str, root: &str) -> Library {
        Library {
            id: String::new(),
            name: name.into(),
            root: komga_media::scanner::path_to_url(Path::new(root)),
            import_comicinfo_book: true,
            import_comicinfo_series: true,
            import_comicinfo_collection: true,
            import_comicinfo_readlist: true,
            import_comicinfo_series_append_volume: true,
            import_epub_book: true,
            import_epub_series: true,
            import_mylar_series: true,
            import_local_artwork: true,
            import_barcode_isbn: true,
            scan_force_modified_time: false,
            scan_on_startup: false,
            scan_interval: ScanInterval::Every6H,
            scan_cbx: true,
            scan_pdf: true,
            scan_epub: true,
            scan_directory_exclusions: vec![],
            repair_extensions: false,
            convert_to_cbz: false,
            empty_trash_after_scan: false,
            series_cover: SeriesCover::First,
            hash_files: true,
            hash_pages: false,
            hash_koreader: false,
            analyze_dimensions: true,
            oneshots_directory: None,
            unavailable_date: None,
            created_date: now_utc(),
            last_modified_date: now_utc(),
        }
    }

    #[test]
    fn add_library_happy_path_submits_scan_and_event() {
        let app = app();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("manga");
        std::fs::create_dir(&root).unwrap();

        let mut rx = app.state.events.subscribe();
        let created = add_library(&app.state, &library("Manga", root.to_str().unwrap())).unwrap();
        assert_eq!(created.id.len(), 13);
        assert_eq!(created.name, "Manga");

        let tasks = TasksDao::new(app.state.tasks_db.clone())
            .find_all()
            .unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks[0].unique_id(),
            format!("SCAN_LIBRARY_{}_DEEP_false", created.id)
        );
        assert_eq!(tasks[0].priority(), DEFAULT_PRIORITY);

        let event = rx.try_recv().unwrap();
        assert!(matches!(event, DomainEvent::LibraryAdded(l) if l.id == created.id));
    }

    #[test]
    fn add_library_validation_errors() {
        let app = app();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("manga");
        std::fs::create_dir(&root).unwrap();
        let file = dir.path().join("file.txt");
        std::fs::write(&file, b"x").unwrap();

        // missing folder
        let missing = dir.path().join("nope");
        let err = add_library(&app.state, &library("A", missing.to_str().unwrap())).unwrap_err();
        assert!(matches!(err, LibraryError::FileNotFound(_)));
        assert_eq!(
            err.message(),
            format!(
                "Library root folder does not exist: {}",
                komga_media::scanner::path_to_url(&missing)
            )
        );

        // not a directory
        let err = add_library(&app.state, &library("A", file.to_str().unwrap())).unwrap_err();
        assert!(matches!(err, LibraryError::DirectoryNotFound(_)));

        // duplicate name
        add_library(&app.state, &library("Manga", root.to_str().unwrap())).unwrap();
        let err = add_library(&app.state, &library("Manga", root.to_str().unwrap())).unwrap_err();
        assert!(matches!(err, LibraryError::DuplicateName(_)));
        assert_eq!(err.message(), "Library name already exists");

        // child path
        let child = root.join("child");
        std::fs::create_dir(&child).unwrap();
        let err = add_library(&app.state, &library("Child", child.to_str().unwrap())).unwrap_err();
        assert!(matches!(err, LibraryError::PathContainedInPath(_)));
        assert!(err
            .message()
            .contains("is a child of existing library Manga"));

        // parent path
        let parent = root.parent().unwrap().to_path_buf();
        let err =
            add_library(&app.state, &library("Parent", parent.to_str().unwrap())).unwrap_err();
        assert!(matches!(err, LibraryError::PathContainedInPath(_)));
        assert!(err
            .message()
            .contains("is a parent of existing library Manga"));
    }

    #[test]
    fn update_library_rescan_and_toggle_derivations() {
        let app = app();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("manga");
        std::fs::create_dir(&root).unwrap();
        let root2 = dir.path().join("comics");
        std::fs::create_dir(&root2).unwrap();

        let created = add_library(&app.state, &library("Manga", root.to_str().unwrap())).unwrap();
        TasksDao::new(app.state.tasks_db.clone())
            .delete_all()
            .unwrap();

        // no change: nothing submitted
        update_library(&app.state, &created).unwrap();
        assert_eq!(
            TasksDao::new(app.state.tasks_db.clone()).count().unwrap(),
            0
        );

        // root changed: rescan
        let mut updated = created.clone();
        updated.root = komga_media::scanner::path_to_url(&root2);
        update_library(&app.state, &updated).unwrap();
        let tasks = TasksDao::new(app.state.tasks_db.clone())
            .find_all()
            .unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks[0].unique_id(),
            format!("SCAN_LIBRARY_{}_DEEP_false", created.id)
        );

        // toggles: hash koreader + hash pages + convert
        TasksDao::new(app.state.tasks_db.clone())
            .delete_all()
            .unwrap();
        let mut updated2 = updated.clone();
        updated2.hash_koreader = true;
        updated2.hash_pages = true;
        updated2.convert_to_cbz = true;
        update_library(&app.state, &updated2).unwrap();
        let ids: Vec<String> = TasksDao::new(app.state.tasks_db.clone())
            .find_all()
            .unwrap()
            .iter()
            .map(|t| t.unique_id())
            .collect();
        assert!(
            ids.contains(&format!("FIND_BOOKS_WITH_MISSING_PAGE_HASH_{}", created.id)),
            "{ids:?}"
        );
        assert!(
            ids.contains(&format!("FIND_BOOKS_TO_CONVERT_{}", created.id)),
            "{ids:?}"
        );

        // unrelated change only: no rescan
        TasksDao::new(app.state.tasks_db.clone())
            .delete_all()
            .unwrap();
        let mut updated3 = updated2.clone();
        updated3.name = "Renamed".into();
        update_library(&app.state, &updated3).unwrap();
        let ids: Vec<String> = TasksDao::new(app.state.tasks_db.clone())
            .find_all()
            .unwrap()
            .iter()
            .map(|t| t.unique_id())
            .collect();
        assert!(
            !ids.iter().any(|i| i.starts_with("SCAN_LIBRARY_")),
            "{ids:?}"
        );

        let stored = LibraryDao::new(app.state.db.clone())
            .find_by_id(&created.id)
            .unwrap()
            .unwrap();
        assert_eq!(stored.name, "Renamed");
        assert!(stored.hash_koreader);
    }

    #[tokio::test]
    async fn update_library_reschedules_periodic_scan() {
        let app = app();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("manga");
        std::fs::create_dir(&root).unwrap();
        let created = add_library(&app.state, &library("Manga", root.to_str().unwrap())).unwrap();
        crate::service::scheduler::ScanScheduler::schedule_scan(&app.state, &created);
        let period_of = |id: &str| {
            crate::service::scheduler::ScanScheduler::scheduled_tasks()
                .into_iter()
                .find(|t| t.library_id == id)
                .map(|t| t.period)
        };
        assert_eq!(
            period_of(&created.id),
            Some(std::time::Duration::from_secs(6 * 3600))
        );

        // interval changed: rescheduled with the new period
        let mut updated = created.clone();
        updated.scan_interval = ScanInterval::Daily;
        update_library(&app.state, &updated).unwrap();
        assert_eq!(
            period_of(&created.id),
            Some(std::time::Duration::from_secs(24 * 3600))
        );

        // disabled: the periodic scan is canceled
        let mut updated2 = updated.clone();
        updated2.scan_interval = ScanInterval::Disabled;
        update_library(&app.state, &updated2).unwrap();
        assert_eq!(period_of(&created.id), None);
    }

    #[test]
    fn delete_library_cascades() {
        let app = app();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("manga");
        std::fs::create_dir(&root).unwrap();
        let created = add_library(&app.state, &library("Manga", root.to_str().unwrap())).unwrap();

        let mut rx = app.state.events.subscribe();
        delete_library(&app.state, &created).unwrap();
        assert!(LibraryDao::new(app.state.db.clone())
            .find_by_id(&created.id)
            .unwrap()
            .is_none());
        let event = rx.try_recv().unwrap();
        assert!(matches!(event, DomainEvent::LibraryDeleted(l) if l.id == created.id));
    }

    #[test]
    fn prepare_library_root_creates_only_the_last_level() {
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().join("杂项");
        std::fs::create_dir(&parent).unwrap();
        let shelf = parent.join("新书架");
        assert!(prepare_library_root(shelf.to_str().unwrap()).unwrap());
        assert!(shelf.is_dir());

        assert!(!prepare_library_root(shelf.to_str().unwrap()).unwrap());

        let missing = dir.path().join("不存在").join("再下一层");
        let error = prepare_library_root(missing.to_str().unwrap()).unwrap_err();
        assert!(error.is_validation());
        assert!(!dir.path().join("不存在").exists());

        let link = dir.path().join("link-root");
        std::os::unix::fs::symlink(&parent, &link).unwrap();
        let error = prepare_library_root(link.to_str().unwrap()).unwrap_err();
        assert!(error.message().contains("软链接"));
    }
}
