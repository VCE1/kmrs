//! Move a series into another library, or link its books into a category folder.
//!
//! A library move renames the series directory only when every entry under it is a symlink or a
//! real directory of symlinks. A category move does not rename anything. Each book symlink is
//! recreated inside the chosen folder, then the old link is removed. A regular file aborts the
//! move. Source file bytes are never opened for writing. The database change is one transaction;
//! a database failure puts the links back. Book titles are locked so a later scrape cannot undo
//! the placement.

use crate::events::DomainEvent;
use crate::state::AppState;
use komga_core::dto::url_to_file_path;
use komga_core::model::book::Book;
use komga_core::model::series::Series;
use komga_core::time_codec::{format_datetime, now_utc};
use komga_db::dao::library::LibraryDao;
use komga_db::dao::series::SeriesDao;
use komga_media::scanner::path_to_url;
use std::path::{Component, Path, PathBuf};

#[derive(Debug)]
pub enum PlacementError {
    NotFound,
    Rejected(String),
    Db(komga_db::Error),
    /// `rename` cannot cross bind mounts even when `st_dev` matches. Callers mirror symlinks instead.
    CrossMount,
}

impl std::fmt::Display for PlacementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "找不到这个系列"),
            Self::Rejected(message) => write!(f, "{message}"),
            Self::Db(error) => write!(f, "{error}"),
            Self::CrossMount => write!(f, "来源和目标不在同一个挂载点"),
        }
    }
}

impl From<komga_db::Error> for PlacementError {
    fn from(error: komga_db::Error) -> Self {
        Self::Db(error)
    }
}

impl From<rusqlite::Error> for PlacementError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Db(error.into())
    }
}

pub struct MoveOutcome {
    pub series: Series,
    /// Set when the old series row was emptied and marked deleted.
    pub removed: Option<Series>,
    pub book_ids: Vec<String>,
}

impl std::fmt::Debug for MoveOutcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MoveOutcome")
            .field("series", &self.series.id)
            .field("removed", &self.removed.as_ref().map(|series| &series.id))
            .field("book_ids", &self.book_ids)
            .finish()
    }
}

pub fn move_series_to_library(
    state: &AppState,
    series_id: &str,
    dest_library_id: &str,
    category_path: Option<&str>,
) -> Result<MoveOutcome, PlacementError> {
    let series = SeriesDao::new(state.db.clone())
        .find_by_id(series_id)?
        .ok_or(PlacementError::NotFound)?;
    let category = category_path.map(str::trim).filter(|path| !path.is_empty());

    let libraries = LibraryDao::new(state.db.clone());
    let source = libraries
        .find_by_id(&series.library_id)?
        .ok_or_else(|| PlacementError::Rejected("来源库不存在，没有移动".into()))?;
    let dest = libraries
        .find_by_id(dest_library_id)?
        .ok_or_else(|| PlacementError::Rejected("目标库不存在".into()))?;

    let series_path = PathBuf::from(url_to_file_path(&series.url));
    let source_root = PathBuf::from(url_to_file_path(&source.root));
    let dest_root = PathBuf::from(url_to_file_path(&dest.root));
    if let Some(path) = category {
        return place_into_category(state, &series, &series_path, &source_root, &dest, &dest_root, path);
    }
    if series.library_id == dest_library_id {
        return Err(PlacementError::Rejected("系列已经在这个库里".into()));
    }
    if library_has_nested_folders(state, &dest.id, &dest.root)? {
        return Err(PlacementError::Rejected(
            "这个库有子分类，要选到具体文件夹，不能整夹搬进去".into(),
        ));
    }
    if !strictly_inside(&series_path, &source_root) {
        return Err(PlacementError::Rejected(
            "系列目录不在来源库里面，没有移动".into(),
        ));
    }
    let on_disk = path_to_url(&series_path);
    if series.url != on_disk {
        return Err(PlacementError::Rejected(
            "数据库里的系列路径和磁盘不一致，没有移动".into(),
        ));
    }
    if !dest_root.is_dir() {
        return Err(PlacementError::Rejected("目标库目录不存在，没有移动".into()));
    }

    let Some(name) = series_path.file_name() else {
        return Err(PlacementError::Rejected("系列目录没有名称，没有移动".into()));
    };
    let dest_path = dest_root.join(name);
    if std::fs::symlink_metadata(&dest_path).is_ok() {
        return Err(PlacementError::Rejected(
            "目标库里已经有同名文件夹，没有移动".into(),
        ));
    }
    assert_link_only(&series_path)?;
    let try_rename = same_device(&series_path, &dest_root)?;
    let renamed = if try_rename {
        match rename_link(&series_path, &dest_path) {
            Ok(()) => true,
            Err(PlacementError::CrossMount) => false,
            Err(error) => return Err(error),
        }
    } else {
        false
    };
    if !renamed {
        mirror_symlink_tree(&series_path, &dest_path)?;
    }
    let new_url = path_to_url(&dest_path);
    let old_prefix = directory_prefix(&on_disk);
    let new_prefix = directory_prefix(&new_url);
    if let Err(error) = rewrite_placement(
        state,
        &series.id,
        &new_url,
        &old_prefix,
        &new_prefix,
        dest_library_id,
    ) {
        let back = if renamed {
            std::fs::rename(&dest_path, &series_path)
                .err()
                .map(|undo| undo.to_string())
        } else {
            remove_link_tree(&dest_path)
                .err()
                .map(|undo| undo.to_string())
        };
        return match back {
            None => Err(error),
            Some(back) => Err(PlacementError::Rejected(format!(
                "数据库已撤销，但文件夹留在 {}，移回 {} 失败：{back}。原因：{error}",
                dest_path.display(),
                series_path.display()
            ))),
        };
    }
    if !renamed {
        if let Err(error) = remove_link_tree(&series_path) {
            return Err(PlacementError::Rejected(format!(
                "书已经挂到目标库，但旧软链接没能删掉：{error}"
            )));
        }
    }

    let moved = SeriesDao::new(state.db.clone())
        .find_by_id(&series.id)?
        .ok_or(PlacementError::NotFound)?;
    let book_ids = book_ids_of(state, &series.id)?;
    tracing::info!(
        series_id = %series.id,
        from = %source.id,
        to = %dest.id,
        books = moved.book_count,
        renamed,
        "moved series folder"
    );
    Ok(MoveOutcome {
        series: moved,
        removed: None,
        book_ids,
    })
}

/// Notify search after the move. Callers that already hold no database connection may use this.
pub fn publish_moved(
    state: &AppState,
    series: &Series,
    books: Vec<Book>,
    removed: Option<&Series>,
) {
    let _ = state
        .events
        .send(DomainEvent::SeriesUpdated(series.clone()));
    if let Some(removed) = removed {
        if removed.id != series.id {
            let _ = state
                .events
                .send(DomainEvent::SeriesDeleted(removed.clone()));
        }
    }
    for book in books {
        let _ = state.events.send(DomainEvent::BookUpdated(book));
    }
}

/// A library has categories when one of its series lives more than one folder below the root.
/// Flat shelves, where each series folder sits directly in the library, stay a single destination.
pub(crate) fn library_has_nested_folders(
    state: &AppState,
    library_id: &str,
    root_url: &str,
) -> Result<bool, PlacementError> {
    let root = if root_url.ends_with('/') {
        root_url.to_string()
    } else {
        format!("{root_url}/")
    };
    let conn = state.db.ro()?;
    match conn.query_row(
        "SELECT 1 FROM SERIES \
         WHERE LIBRARY_ID = ?1 AND DELETED_DATE IS NULL \
           AND substr(URL, 1, length(?2)) = ?2 \
           AND instr(trim(substr(URL, length(?2) + 1), '/'), '/') > 0 \
         LIMIT 1",
        rusqlite::params![library_id, root],
        |row| row.get::<_, i64>(0),
    ) {
        Ok(_) => Ok(true),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

struct LinkedBook {
    id: String,
    old_path: PathBuf,
    old_url: String,
    target: PathBuf,
    new_path: PathBuf,
    new_url: String,
}

fn place_into_category(
    state: &AppState,
    series: &Series,
    series_path: &Path,
    source_root: &Path,
    dest: &komga_core::model::library::Library,
    dest_root: &Path,
    category_path: &str,
) -> Result<MoveOutcome, PlacementError> {
    if !strictly_inside(series_path, source_root) {
        return Err(PlacementError::Rejected(
            "系列目录不在来源库里面，没有移动".into(),
        ));
    }
    let series_meta = std::fs::symlink_metadata(series_path).map_err(|error| {
        PlacementError::Rejected(format!("读不到系列目录 {}：{error}", series_path.display()))
    })?;
    if series_meta.file_type().is_symlink() || !series_meta.is_dir() {
        return Err(PlacementError::Rejected(
            "系列目录本身不是普通文件夹，没有移动。源文件没有被改动。".into(),
        ));
    }
    let relative = category_relative(category_path)?;
    let dest_dir = real_category_dir(dest_root, &relative)?;
    if dest_dir == series_path || dest_dir.starts_with(series_path) {
        return Err(PlacementError::Rejected(
            "不能移动到这个系列自己的目录里".into(),
        ));
    }
    let on_disk = path_to_url(series_path);
    if series.url != on_disk {
        return Err(PlacementError::Rejected(
            "数据库里的系列路径和磁盘不一致，没有移动".into(),
        ));
    }
    let dest_url = path_to_url(&dest_dir);
    if series.url == dest_url && series.library_id == dest.id {
        return Err(PlacementError::Rejected("系列已经在这个分类里".into()));
    }

    let rows = book_rows(state, &series.id)?;
    if rows.is_empty() {
        return Err(PlacementError::Rejected("这个系列没有书，没有移动".into()));
    }
    let mut items = Vec::with_capacity(rows.len());
    let mut names = std::collections::BTreeSet::new();
    for (id, old_url) in rows {
        let old_path = PathBuf::from(url_to_file_path(&old_url));
        if !strictly_inside(&old_path, series_path) {
            return Err(PlacementError::Rejected(
                "有的书籍路径不在这个系列目录下，没有移动".into(),
            ));
        }
        let meta = std::fs::symlink_metadata(&old_path).map_err(|error| {
            PlacementError::Rejected(format!(
                "读不到书籍软链接 {}：{error}。源文件没有被改动。",
                old_path.display()
            ))
        })?;
        if !meta.file_type().is_symlink() {
            return Err(PlacementError::Rejected(format!(
                "系列里有普通文件，不能移动：{}。源文件没有被改动。",
                old_path.display()
            )));
        }
        let target = absolute_link_target(&old_path)?;
        let followed = std::fs::metadata(&target).map_err(|error| {
            PlacementError::Rejected(format!(
                "软链接指向的源文件读不到 {}：{error}。源文件没有被改动。",
                target.display()
            ))
        })?;
        if !followed.is_file() {
            return Err(PlacementError::Rejected(format!(
                "软链接指向的不是文件：{}。源文件没有被改动。",
                target.display()
            )));
        }
        let Some(file_name) = old_path.file_name().map(|name| name.to_os_string()) else {
            return Err(PlacementError::Rejected(
                "书籍文件没有名称，没有移动".into(),
            ));
        };
        if !names.insert(file_name.clone()) {
            return Err(PlacementError::Rejected(
                "系列里有同名文件，没有移动".into(),
            ));
        }
        let new_path = dest_dir.join(&file_name);
        if std::fs::symlink_metadata(&new_path).is_ok() {
            return Err(PlacementError::Rejected(format!(
                "目标分类里已经有同名文件 {}，没有移动。",
                new_path.display()
            )));
        }
        items.push(LinkedBook {
            id,
            old_path,
            old_url,
            target,
            new_url: String::new(),
            new_path,
        });
    }
    for item in &mut items {
        // The path does not exist yet. path_to_url still encodes it; the missing
        // symlink is not a directory, so the URL has no trailing slash.
        item.new_url = path_to_url(&item.new_path);
    }
    if url_taken(state, &items)? {
        return Err(PlacementError::Rejected(
            "目标分类里已经有这本书的记录，没有移动".into(),
        ));
    }

    let mut created = Vec::new();
    for item in &items {
        if let Err(error) = create_symlink(&item.target, &item.new_path) {
            return Err(undo_or(error, &created, &[]));
        }
        created.push(item.new_path.clone());
    }
    let mut removed = Vec::new();
    for item in &items {
        if let Err(error) = unlink_symlink(&item.old_path) {
            return Err(undo_or(error, &created, &removed));
        }
        removed.push((item.old_path.clone(), item.target.clone()));
    }

    let folder_name = dest_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "分类".to_string());
    match assign_books(state, &series.id, &dest.id, &dest_url, &folder_name, &items) {
        Ok((dest_series_id, source_deleted)) => {
            remove_empty_dirs(series_path, source_root);
            let moved = SeriesDao::new(state.db.clone())
                .find_by_id(&dest_series_id)?
                .ok_or(PlacementError::NotFound)?;
            let removed_series = if source_deleted {
                SeriesDao::new(state.db.clone()).find_by_id(&series.id)?
            } else {
                None
            };
            tracing::info!(
                series_id = %series.id,
                dest_series_id = %dest_series_id,
                library_id = %dest.id,
                category = %dest_dir.display(),
                books = items.len(),
                "linked series books into a category folder"
            );
            Ok(MoveOutcome {
                series: moved,
                removed: removed_series,
                book_ids: items.iter().map(|item| item.id.clone()).collect(),
            })
        }
        Err(error) => {
            if let Err(back) = undo_links(&created, &removed) {
                Err(PlacementError::Rejected(format!(
                    "数据库已撤销，但软链接没有完全恢复：{back}。原因：{error}"
                )))
            } else {
                Err(error)
            }
        }
    }
}

fn category_relative(path: &str) -> Result<PathBuf, PlacementError> {
    let trimmed = path.trim().trim_matches('/');
    if trimmed.is_empty() {
        return Err(PlacementError::Rejected("要选择分类文件夹".into()));
    }
    let mut out = PathBuf::new();
    for part in trimmed.split('/') {
        if part.is_empty() || part == "." || part == ".." || part.contains('\0') {
            return Err(PlacementError::Rejected("分类路径不对，没有移动".into()));
        }
        out.push(part);
    }
    Ok(out)
}

/// Walk each component with `symlink_metadata`, so a symlink directory cannot redirect
/// the link into another tree.
fn real_category_dir(root: &Path, relative: &Path) -> Result<PathBuf, PlacementError> {
    if !root.is_dir() {
        return Err(PlacementError::Rejected("目标库目录不存在，没有移动".into()));
    }
    let mut current = root.to_path_buf();
    for part in relative.components() {
        let Component::Normal(name) = part else {
            return Err(PlacementError::Rejected("分类路径不对，没有移动".into()));
        };
        current.push(name);
        let meta = match std::fs::symlink_metadata(&current) {
            Ok(meta) => meta,
            Err(_) => {
                return Err(PlacementError::Rejected(
                    "这个分类文件夹不存在，没有移动".into(),
                ))
            }
        };
        if meta.file_type().is_symlink() {
            return Err(PlacementError::Rejected(
                "分类路径上有软链接，没有移动".into(),
            ));
        }
        if !meta.is_dir() {
            return Err(PlacementError::Rejected(
                "这个分类不是文件夹，没有移动".into(),
            ));
        }
    }
    if !strictly_inside(&current, root) {
        return Err(PlacementError::Rejected(
            "分类不在目标库里面，没有移动".into(),
        ));
    }
    Ok(current)
}

fn absolute_link_target(link: &Path) -> Result<PathBuf, PlacementError> {
    let raw = std::fs::read_link(link).map_err(|error| {
        PlacementError::Rejected(format!("读不到软链接指向 {}：{error}", link.display()))
    })?;
    if raw.is_absolute() {
        Ok(raw)
    } else {
        Ok(link.parent().unwrap_or(Path::new("/")).join(raw))
    }
}

fn book_rows(state: &AppState, series_id: &str) -> Result<Vec<(String, String)>, PlacementError> {
    let conn = state.db.ro()?;
    let mut stmt = conn.prepare(
        "SELECT ID, URL FROM BOOK WHERE SERIES_ID = ?1 AND DELETED_DATE IS NULL ORDER BY NUMBER, ID",
    )?;
    let rows = stmt.query_map([series_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

fn book_ids_of(state: &AppState, series_id: &str) -> Result<Vec<String>, PlacementError> {
    Ok(book_rows(state, series_id)?
        .into_iter()
        .map(|(id, _)| id)
        .collect())
}

fn url_taken(state: &AppState, items: &[LinkedBook]) -> Result<bool, PlacementError> {
    let conn = state.db.ro()?;
    for item in items {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM BOOK WHERE URL = ?1 AND ID != ?2 AND DELETED_DATE IS NULL",
            rusqlite::params![item.new_url, item.id],
            |row| row.get(0),
        )?;
        if count > 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

fn create_symlink(target: &Path, link: &Path) -> Result<(), PlacementError> {
    #[cfg(unix)]
    {
        return std::os::unix::fs::symlink(target, link).map_err(|error| {
            if error.kind() == std::io::ErrorKind::PermissionDenied {
                PlacementError::Rejected("目标分类不可写，源文件没有被改动".into())
            } else {
                PlacementError::Rejected(format!(
                    "没能在分类里加上软链接，源文件没有被改动：{error}"
                ))
            }
        });
    }
    #[cfg(not(unix))]
    {
        let _ = (target, link);
        Err(PlacementError::Rejected(
            "只支持在 Linux 上移动系列".into(),
        ))
    }
}

fn unlink_symlink(path: &Path) -> Result<(), PlacementError> {
    let meta = std::fs::symlink_metadata(path).map_err(|error| {
        PlacementError::Rejected(format!("读不到要移走的软链接 {}：{error}", path.display()))
    })?;
    if !meta.file_type().is_symlink() {
        return Err(PlacementError::Rejected(format!(
            "要移走的不是软链接，已停止：{}。源文件没有被改动。",
            path.display()
        )));
    }
    std::fs::remove_file(path).map_err(|error| {
        PlacementError::Rejected(format!(
            "移走旧软链接失败 {}：{error}。源文件没有被改动。",
            path.display()
        ))
    })
}

fn undo_or(
    error: PlacementError,
    created: &[PathBuf],
    removed: &[(PathBuf, PathBuf)],
) -> PlacementError {
    match undo_links(created, removed) {
        Ok(()) => error,
        Err(back) => PlacementError::Rejected(format!(
            "{error}。恢复软链接也失败：{back}"
        )),
    }
}

fn undo_links(created: &[PathBuf], removed: &[(PathBuf, PathBuf)]) -> Result<(), PlacementError> {
    let mut errors = Vec::new();
    for path in created {
        match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                if let Err(error) = std::fs::remove_file(path) {
                    errors.push(format!("删掉新软链接 {} 失败：{error}", path.display()));
                }
            }
            Ok(_) => errors.push(format!("新路径不是软链接，没有删：{}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => errors.push(format!("读不到新软链接 {}：{error}", path.display())),
        }
    }
    for (old, target) in removed {
        if std::fs::symlink_metadata(old).is_ok() {
            continue;
        }
        if let Err(error) = create_symlink(target, old) {
            errors.push(error.to_string());
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(PlacementError::Rejected(errors.join("；")))
    }
}

fn remove_empty_dirs(dir: &Path, root: &Path) {
    if !strictly_inside(dir, root) {
        return;
    }
    let Ok(meta) = std::fs::symlink_metadata(dir) else {
        return;
    };
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(child) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if child.file_type().is_dir() && !child.file_type().is_symlink() {
            remove_empty_dirs(&path, root);
        }
    }
    if std::fs::read_dir(dir)
        .ok()
        .and_then(|mut read| read.next())
        .is_none()
    {
        let _ = std::fs::remove_dir(dir);
    }
}

fn assign_books(
    state: &AppState,
    source_series_id: &str,
    dest_library_id: &str,
    dest_url: &str,
    folder_name: &str,
    items: &[LinkedBook],
) -> Result<(String, bool), PlacementError> {
    let mut conn = state.db.rw()?;
    let tx = conn.transaction()?;
    let now = format_datetime(now_utc());
    let existing: Option<(String, bool)> = {
        let mut stmt = tx.prepare(
            "SELECT ID, DELETED_DATE IS NOT NULL FROM SERIES WHERE LIBRARY_ID = ?1 AND URL = ?2",
        )?;
        let rows = stmt
            .query_map(rusqlite::params![dest_library_id, dest_url], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? != 0))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows.iter()
            .find(|(_, deleted)| !deleted)
            .cloned()
            .or_else(|| rows.first().cloned())
    };
    if existing.as_ref().map(|(id, _)| id.as_str()) == Some(source_series_id) {
        return Err(PlacementError::Rejected("系列已经在这个分类里".into()));
    }
    let dest_series_id = if let Some((id, deleted)) = existing {
        if deleted {
            tx.execute(
                "UPDATE SERIES SET DELETED_DATE = NULL, NAME = ?1, LAST_MODIFIED_DATE = ?2 WHERE ID = ?3",
                rusqlite::params![folder_name, now, id],
            )?;
        }
        id
    } else {
        let id = state.tsid.create_string();
        tx.execute(
            "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID, BOOK_COUNT, ONESHOT) \
             VALUES (?1, ?2, ?3, ?4, ?5, 0, 0)",
            rusqlite::params![id, folder_name, dest_url, now, dest_library_id],
        )?;
        tx.execute(
            "INSERT INTO SERIES_METADATA (SERIES_ID, STATUS, TITLE, TITLE_SORT, TITLE_LOCK, TITLE_SORT_LOCK) \
             VALUES (?1, 'ONGOING', ?2, ?2, 1, 1)",
            rusqlite::params![id, folder_name],
        )?;
        id
    };

    for item in items {
        let book_rows = tx.execute(
            "UPDATE BOOK SET URL = ?1, SERIES_ID = ?2, LIBRARY_ID = ?3, LAST_MODIFIED_DATE = ?4 \
             WHERE ID = ?5 AND SERIES_ID = ?6",
            rusqlite::params![
                item.new_url,
                dest_series_id,
                dest_library_id,
                now,
                item.id,
                source_series_id
            ],
        )?;
        if book_rows != 1 {
            return Err(PlacementError::Rejected(
                "有的书籍记录没有更新，已撤销移动".into(),
            ));
        }
        let meta_rows = tx.execute(
            "UPDATE BOOK_METADATA SET TITLE_LOCK = 1, LAST_MODIFIED_DATE = ?1 WHERE BOOK_ID = ?2",
            rusqlite::params![now, item.id],
        )?;
        if meta_rows != 1 {
            return Err(PlacementError::Rejected(
                "有的书籍没有元数据，已撤销移动".into(),
            ));
        }
        tx.execute(
            "UPDATE THUMBNAIL_BOOK SET URL = ?1, LAST_MODIFIED_DATE = ?2 \
             WHERE BOOK_ID = ?3 AND URL = ?4",
            rusqlite::params![item.new_url, now, item.id, item.old_url],
        )?;
    }

    for series_id in [dest_series_id.as_str(), source_series_id] {
        tx.execute(
            "UPDATE SERIES SET BOOK_COUNT = ( \
                 SELECT COUNT(*) FROM BOOK WHERE SERIES_ID = ?1 AND DELETED_DATE IS NULL \
             ), LAST_MODIFIED_DATE = ?2 WHERE ID = ?1",
            rusqlite::params![series_id, now],
        )?;
    }
    let source_left: i64 = tx.query_row(
        "SELECT COUNT(*) FROM BOOK WHERE SERIES_ID = ?1 AND DELETED_DATE IS NULL",
        [source_series_id],
        |row| row.get(0),
    )?;
    let source_deleted = source_left == 0;
    if source_deleted {
        let rows = tx.execute(
            "UPDATE SERIES SET DELETED_DATE = ?1, LAST_MODIFIED_DATE = ?1 WHERE ID = ?2",
            rusqlite::params![now, source_series_id],
        )?;
        if rows != 1 {
            return Err(PlacementError::Rejected(
                "原来的系列记录没有更新，已撤销移动".into(),
            ));
        }
    }
    tx.commit()?;
    Ok((dest_series_id, source_deleted))
}

fn directory_prefix(url: &str) -> String {
    if url.ends_with('/') {
        url.to_string()
    } else {
        format!("{url}/")
    }
}

fn strictly_inside(child: &Path, root: &Path) -> bool {
    let Ok(relative) = child.strip_prefix(root) else {
        return false;
    };
    let mut parts = relative.components();
    match parts.next() {
        Some(Component::Normal(_)) => parts.all(|part| matches!(part, Component::Normal(_))),
        _ => false,
    }
}

fn assert_link_only(path: &Path) -> Result<(), PlacementError> {
    let meta = std::fs::symlink_metadata(path).map_err(|error| {
        PlacementError::Rejected(format!("读不到系列目录 {}：{error}", path.display()))
    })?;
    let file_type = meta.file_type();
    if file_type.is_symlink() {
        return Ok(());
    }
    if file_type.is_dir() {
        let entries = std::fs::read_dir(path).map_err(|error| {
            PlacementError::Rejected(format!("读不到系列目录 {}：{error}", path.display()))
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                PlacementError::Rejected(format!("读不到系列目录 {}：{error}", path.display()))
            })?;
            assert_link_only(&entry.path())?;
        }
        return Ok(());
    }
    Err(PlacementError::Rejected(format!(
        "系列里有普通文件，不能移动：{}。源文件没有被改动。",
        path.display()
    )))
}

fn same_device(series_path: &Path, dest_root: &Path) -> Result<bool, PlacementError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let src = std::fs::symlink_metadata(series_path).map_err(|error| {
            PlacementError::Rejected(format!("读不到系列目录：{error}"))
        })?;
        let dst = std::fs::metadata(dest_root).map_err(|error| {
            PlacementError::Rejected(format!("读不到目标库：{error}"))
        })?;
        return Ok(src.dev() == dst.dev());
    }
    #[cfg(not(unix))]
    {
        let _ = (series_path, dest_root);
        Err(PlacementError::Rejected(
            "只支持在 Linux 上移动系列".into(),
        ))
    }
}

fn rename_link(from: &Path, to: &Path) -> Result<(), PlacementError> {
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            Err(PlacementError::Rejected(
                "目标库目录不可写，源文件没有被移动".into(),
            ))
        }
        Err(error)
            if error.kind() == std::io::ErrorKind::CrossesDevices
                || error.raw_os_error() == Some(18) =>
        {
            Err(PlacementError::CrossMount)
        }
        Err(error) => Err(PlacementError::Rejected(format!(
            "移动系列文件夹失败，源文件没有被复制：{error}"
        ))),
    }
}

/// Recreate a symlink-only series directory at `dst`. File bytes are not copied.
fn mirror_symlink_tree(src: &Path, dst: &Path) -> Result<(), PlacementError> {
    if std::fs::symlink_metadata(dst).is_ok() {
        return Err(PlacementError::Rejected(
            "目标库里已经有同名文件夹，没有移动".into(),
        ));
    }
    std::fs::create_dir(dst).map_err(|error| {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            PlacementError::Rejected("目标库目录不可写，源文件没有被移动".into())
        } else {
            PlacementError::Rejected(format!(
                "没能在目标库建文件夹，源文件没有被复制：{error}"
            ))
        }
    })?;
    if let Err(error) = mirror_children(src, dst) {
        let _ = remove_link_tree(dst);
        return Err(error);
    }
    Ok(())
}

fn mirror_children(src: &Path, dst: &Path) -> Result<(), PlacementError> {
    let entries = std::fs::read_dir(src).map_err(|error| {
        PlacementError::Rejected(format!("读不到系列目录 {}：{error}", src.display()))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            PlacementError::Rejected(format!("读不到系列目录 {}：{error}", src.display()))
        })?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let meta = std::fs::symlink_metadata(&from).map_err(|error| {
            PlacementError::Rejected(format!("读不到 {}：{error}", from.display()))
        })?;
        if meta.file_type().is_symlink() {
            let raw = std::fs::read_link(&from).map_err(|error| {
                PlacementError::Rejected(format!("读不到软链接 {}：{error}", from.display()))
            })?;
            let target = if raw.is_absolute() {
                raw
            } else {
                from.parent().unwrap_or(Path::new("/")).join(raw)
            };
            create_symlink(&target, &to)?;
        } else if meta.is_dir() {
            std::fs::create_dir(&to).map_err(|error| {
                PlacementError::Rejected(format!("没能建立子文件夹 {}：{error}", to.display()))
            })?;
            mirror_children(&from, &to)?;
        } else {
            return Err(PlacementError::Rejected(format!(
                "系列里有普通文件，不能移动：{}。源文件没有被改动。",
                from.display()
            )));
        }
    }
    Ok(())
}

fn remove_link_tree(path: &Path) -> Result<(), PlacementError> {
    let meta = std::fs::symlink_metadata(path).map_err(|error| {
        PlacementError::Rejected(format!("读不到 {}：{error}", path.display()))
    })?;
    if meta.file_type().is_symlink() {
        return std::fs::remove_file(path).map_err(|error| {
            PlacementError::Rejected(format!("删不掉软链接 {}：{error}", path.display()))
        });
    }
    if meta.is_dir() {
        let entries = std::fs::read_dir(path).map_err(|error| {
            PlacementError::Rejected(format!("读不到 {}：{error}", path.display()))
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                PlacementError::Rejected(format!("读不到 {}：{error}", path.display()))
            })?;
            remove_link_tree(&entry.path())?;
        }
        return std::fs::remove_dir(path).map_err(|error| {
            PlacementError::Rejected(format!("删不掉空文件夹 {}：{error}", path.display()))
        });
    }
    Err(PlacementError::Rejected(format!(
        "遇到普通文件，没有删除：{}。源文件没有被改动。",
        path.display()
    )))
}

fn rewrite_placement(
    state: &AppState,
    series_id: &str,
    new_series_url: &str,
    old_prefix: &str,
    new_prefix: &str,
    dest_library_id: &str,
) -> Result<(), PlacementError> {
    let mut conn = state.db.rw()?;
    let tx = conn.transaction()?;
    let now = format_datetime(now_utc());
    let old_len = old_prefix.chars().count() as i64;
    let start = old_len + 1;

    let series_rows = tx.execute(
        "UPDATE SERIES SET URL = ?1, LIBRARY_ID = ?2, LAST_MODIFIED_DATE = ?3 WHERE ID = ?4",
        rusqlite::params![new_series_url, dest_library_id, now, series_id],
    )?;
    if series_rows != 1 {
        return Err(PlacementError::Rejected(
            "系列记录没有更新，已撤销移动".into(),
        ));
    }

    let book_count: i64 = tx.query_row(
        "SELECT COUNT(*) FROM BOOK WHERE SERIES_ID = ?1",
        [series_id],
        |row| row.get(0),
    )?;
    let book_rows = tx.execute(
        "UPDATE BOOK SET URL = ?1 || substr(URL, ?2), LIBRARY_ID = ?3, LAST_MODIFIED_DATE = ?4 \
         WHERE SERIES_ID = ?5 AND substr(URL, 1, ?6) = ?7",
        rusqlite::params![
            new_prefix,
            start,
            dest_library_id,
            now,
            series_id,
            old_len,
            old_prefix
        ],
    )?;
    if book_rows as i64 != book_count {
        return Err(PlacementError::Rejected(
            "有的书籍路径不在这个系列目录下，已撤销移动".into(),
        ));
    }

    tx.execute(
        "UPDATE THUMBNAIL_BOOK SET URL = ?1 || substr(URL, ?2), LAST_MODIFIED_DATE = ?3 \
         WHERE BOOK_ID IN (SELECT ID FROM BOOK WHERE SERIES_ID = ?4) \
           AND URL IS NOT NULL AND substr(URL, 1, ?5) = ?6",
        rusqlite::params![new_prefix, start, now, series_id, old_len, old_prefix],
    )?;
    tx.execute(
        "UPDATE THUMBNAIL_SERIES SET URL = ?1 || substr(URL, ?2), LAST_MODIFIED_DATE = ?3 \
         WHERE SERIES_ID = ?4 AND URL IS NOT NULL AND substr(URL, 1, ?5) = ?6",
        rusqlite::params![new_prefix, start, now, series_id, old_len, old_prefix],
    )?;

    let sidecars = {
        let mut stmt = tx.prepare(
            "SELECT URL, PARENT_URL FROM SIDECAR \
             WHERE substr(URL, 1, ?1) = ?2 OR substr(PARENT_URL, 1, ?1) = ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![old_len, old_prefix], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    for (url, parent) in sidecars {
        tx.execute(
            "UPDATE SIDECAR SET URL = ?1, PARENT_URL = ?2, LIBRARY_ID = ?3 WHERE URL = ?4",
            rusqlite::params![
                rewrite_prefix(&url, old_prefix, new_prefix),
                rewrite_prefix(&parent, old_prefix, new_prefix),
                dest_library_id,
                url
            ],
        )?;
    }

    let meta_rows = tx.execute(
        "UPDATE SERIES_METADATA SET TITLE_LOCK = 1, TITLE_SORT_LOCK = 1, LAST_MODIFIED_DATE = ?1 \
         WHERE SERIES_ID = ?2",
        rusqlite::params![now, series_id],
    )?;
    if meta_rows != 1 {
        return Err(PlacementError::Rejected(
            "系列没有元数据行，已撤销移动".into(),
        ));
    }
    let book_meta_rows = tx.execute(
        "UPDATE BOOK_METADATA SET TITLE_LOCK = 1, LAST_MODIFIED_DATE = ?1 \
         WHERE BOOK_ID IN (SELECT ID FROM BOOK WHERE SERIES_ID = ?2)",
        rusqlite::params![now, series_id],
    )?;
    if book_meta_rows as i64 != book_count {
        return Err(PlacementError::Rejected(
            "有的书籍没有元数据，已撤销移动".into(),
        ));
    }

    tx.commit()?;
    Ok(())
}

fn rewrite_prefix(value: &str, old_prefix: &str, new_prefix: &str) -> String {
    match value.strip_prefix(old_prefix) {
        Some(rest) => format!("{new_prefix}{rest}"),
        None => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, MetadataExt};

    fn scratch() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn insert_library(state: &AppState, id: &str, name: &str, root: &Path) {
        state
            .db
            .rw()
            .unwrap()
            .execute(
                "INSERT INTO LIBRARY (ID, NAME, ROOT) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, name, path_to_url(root)],
            )
            .unwrap();
    }

    fn insert_series(state: &AppState, id: &str, library_id: &str, path: &Path) {
        let url = path_to_url(path);
        state
            .db
            .rw()
            .unwrap()
            .execute(
                "INSERT INTO SERIES (ID, NAME, URL, FILE_LAST_MODIFIED, LIBRARY_ID, BOOK_COUNT) \
                 VALUES (?1, 'Series', ?2, '2020-01-01 00:00:00.0', ?3, 1)",
                rusqlite::params![id, url, library_id],
            )
            .unwrap();
        state
            .db
            .rw()
            .unwrap()
            .execute(
                "INSERT INTO SERIES_METADATA (SERIES_ID, STATUS, TITLE, TITLE_SORT) \
                 VALUES (?1, 'ONGOING', 'Series', 'Series')",
                [id],
            )
            .unwrap();
    }

    fn insert_book(state: &AppState, id: &str, series_id: &str, library_id: &str, path: &Path) {
        let url = path_to_url(path);
        state
            .db
            .rw()
            .unwrap()
            .execute(
                "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID, NUMBER) \
                 VALUES (?1, 'book.epub', ?2, '2020-01-01 00:00:00.0', ?3, ?4, 1)",
                rusqlite::params![id, url, series_id, library_id],
            )
            .unwrap();
        state
            .db
            .rw()
            .unwrap()
            .execute(
                "INSERT INTO BOOK_METADATA (BOOK_ID, NUMBER, NUMBER_SORT, TITLE) \
                 VALUES (?1, '1', 1, 'Book')",
                [id],
            )
            .unwrap();
        state
            .db
            .rw()
            .unwrap()
            .execute(
                "INSERT INTO THUMBNAIL_BOOK (ID, THUMBNAIL, URL, SELECTED, TYPE, BOOK_ID) \
                 VALUES (?1, X'0102', ?2, 1, 'USER_UPLOADED', ?3)",
                rusqlite::params![format!("t-{id}"), url, id],
            )
            .unwrap();
    }

    #[test]
    fn sibling_library_names_are_not_prefixes() {
        let root = Path::new("/novels/日文轻小说");
        let child = Path::new("/novels/日文轻小说/某系列");
        let sibling = Path::new("/novels/日文轻小说2/某系列");
        assert!(strictly_inside(child, root));
        assert!(!strictly_inside(sibling, root));
        assert!(!strictly_inside(root, root));
    }

    #[test]
    fn regular_file_is_refused_and_left_in_place() {
        let dir = scratch();
        let series = dir.path().join("src").join("Series");
        std::fs::create_dir_all(&series).unwrap();
        let book = series.join("book.epub");
        std::fs::write(&book, b"source-bytes").unwrap();

        let error = assert_link_only(&series).unwrap_err();
        assert!(error.to_string().contains("普通文件"), "{error}");
        assert_eq!(std::fs::read(&book).unwrap(), b"source-bytes");
    }

    #[test]
    fn symlink_directory_is_not_followed() {
        let dir = scratch();
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("real.epub"), b"keep").unwrap();
        let series = dir.path().join("Series");
        std::fs::create_dir_all(&series).unwrap();
        symlink(&outside, series.join("linked")).unwrap();

        assert_link_only(&series).unwrap();
        assert_eq!(std::fs::read(outside.join("real.epub")).unwrap(), b"keep");
    }

    #[test]
    fn move_renames_symlink_and_locks_titles() {
        let dir = scratch();
        let source_file = dir.path().join("original.epub");
        std::fs::write(&source_file, b"original-bytes").unwrap();
        let inode = std::fs::metadata(&source_file).unwrap().ino();

        let src_root = dir.path().join("src-lib");
        let dst_root = dir.path().join("dst-lib");
        let series = src_root.join("某系列 1");
        std::fs::create_dir_all(&series).unwrap();
        std::fs::create_dir_all(&dst_root).unwrap();
        let link = series.join("book.epub");
        symlink(&source_file, &link).unwrap();
        let sidecar = series.join("cover.jpg");
        symlink(&source_file, &sidecar).unwrap();

        let state = crate::service::series::tests::test_state();
        insert_library(&state, "src", "来源", &src_root);
        insert_library(&state, "dst", "目标", &dst_root);
        insert_series(&state, "s1", "src", &series);
        insert_book(&state, "b1", "s1", "src", &link);
        let series_url = path_to_url(&series);
        state
            .db
            .rw()
            .unwrap()
            .execute(
                "INSERT INTO SIDECAR (URL, PARENT_URL, LAST_MODIFIED_TIME, LIBRARY_ID) \
                 VALUES (?1, ?2, '2020-01-01 00:00:00.0', 'src')",
                rusqlite::params![path_to_url(&sidecar), series_url],
            )
            .unwrap();

        let moved = move_series_to_library(&state, "s1", "dst", None).unwrap();
        let dest = dst_root.join("某系列 1");
        assert_eq!(moved.series.library_id, "dst");
        assert_eq!(moved.series.url, path_to_url(&dest));
        assert!(!series.exists());
        assert!(dest.join("book.epub").is_symlink());
        assert_eq!(std::fs::read(&source_file).unwrap(), b"original-bytes");
        assert_eq!(std::fs::metadata(&source_file).unwrap().ino(), inode);
        assert_eq!(std::fs::metadata(&source_file).unwrap().nlink(), 1);

        let conn = state.db.ro().unwrap();
        let (book_url, book_lib, book_lock): (String, String, i64) = conn
            .query_row(
                "SELECT BOOK.URL, BOOK.LIBRARY_ID, BOOK_METADATA.TITLE_LOCK \
                 FROM BOOK JOIN BOOK_METADATA ON BOOK.ID = BOOK_METADATA.BOOK_ID WHERE BOOK.ID = 'b1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(book_lib, "dst");
        assert_eq!(book_lock, 1);
        assert_eq!(book_url, path_to_url(&dest.join("book.epub")));
        let (title_lock, sort_lock): (i64, i64) = conn
            .query_row(
                "SELECT TITLE_LOCK, TITLE_SORT_LOCK FROM SERIES_METADATA WHERE SERIES_ID = 's1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!((title_lock, sort_lock), (1, 1));
        let (thumb_url, blob): (String, Vec<u8>) = conn
            .query_row(
                "SELECT URL, THUMBNAIL FROM THUMBNAIL_BOOK WHERE ID = 't-b1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(thumb_url, book_url);
        assert_eq!(blob, vec![1, 2]);
        let sidecar_url: String = conn
            .query_row("SELECT URL FROM SIDECAR", [], |row| row.get(0))
            .unwrap();
        assert_eq!(sidecar_url, path_to_url(&dest.join("cover.jpg")));
    }

    #[test]
    fn database_mismatch_puts_the_folder_back() {
        let dir = scratch();
        let source_file = dir.path().join("original.epub");
        std::fs::write(&source_file, b"original-bytes").unwrap();
        let src_root = dir.path().join("src-lib");
        let dst_root = dir.path().join("dst-lib");
        let series = src_root.join("Series");
        std::fs::create_dir_all(&series).unwrap();
        std::fs::create_dir_all(&dst_root).unwrap();
        symlink(&source_file, series.join("book.epub")).unwrap();

        let state = crate::service::series::tests::test_state();
        insert_library(&state, "src", "来源", &src_root);
        insert_library(&state, "dst", "目标", &dst_root);
        insert_series(&state, "s1", "src", &series);
        state
            .db
            .rw()
            .unwrap()
            .execute(
                "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID, NUMBER) \
                 VALUES ('b1', 'book.epub', 'file:/elsewhere/book.epub', '2020-01-01 00:00:00.0', 's1', 'src', 1)",
                [],
            )
            .unwrap();
        state
            .db
            .rw()
            .unwrap()
            .execute(
                "INSERT INTO BOOK_METADATA (BOOK_ID, NUMBER, NUMBER_SORT, TITLE) \
                 VALUES ('b1', '1', 1, 'Book')",
                [],
            )
            .unwrap();

        let error = move_series_to_library(&state, "s1", "dst", None).unwrap_err();
        assert!(error.to_string().contains("已撤销"), "{error}");
        assert!(series.join("book.epub").is_symlink());
        assert!(!dst_root.join("Series").exists());
        assert_eq!(std::fs::read(&source_file).unwrap(), b"original-bytes");
        let library_id: String = state
            .db
            .ro()
            .unwrap()
            .query_row("SELECT LIBRARY_ID FROM SERIES WHERE ID = 's1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(library_id, "src");
    }

    #[test]
    fn destination_collision_does_not_rename() {
        let dir = scratch();
        let src_root = dir.path().join("src-lib");
        let dst_root = dir.path().join("dst-lib");
        let series = src_root.join("Series");
        std::fs::create_dir_all(&series).unwrap();
        std::fs::create_dir_all(dst_root.join("Series")).unwrap();
        symlink(dir.path(), series.join("book")).unwrap();

        let state = crate::service::series::tests::test_state();
        insert_library(&state, "src", "来源", &src_root);
        insert_library(&state, "dst", "目标", &dst_root);
        insert_series(&state, "s1", "src", &series);
        state
            .db
            .rw()
            .unwrap()
            .execute("UPDATE SERIES SET BOOK_COUNT = 0 WHERE ID = 's1'", [])
            .unwrap();

        let error = move_series_to_library(&state, "s1", "dst", None).unwrap_err();
        assert!(error.to_string().contains("同名"), "{error}");
        assert!(series.is_dir());
    }

    #[test]
    fn mirror_recreates_symlinks_without_copying_bytes() {
        let dir = scratch();
        let source_file = dir.path().join("original.epub");
        std::fs::write(&source_file, b"original-bytes").unwrap();
        let inode = std::fs::metadata(&source_file).unwrap().ino();
        let series = dir.path().join("src").join("某系列");
        let nested = series.join("extra");
        std::fs::create_dir_all(&nested).unwrap();
        symlink(&source_file, series.join("book.epub")).unwrap();
        symlink(&source_file, nested.join("side.epub")).unwrap();
        let dest = dir.path().join("dst").join("某系列");
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();

        mirror_symlink_tree(&series, &dest).unwrap();
        assert_eq!(std::fs::read_link(dest.join("book.epub")).unwrap(), source_file);
        assert_eq!(
            std::fs::read_link(dest.join("extra").join("side.epub")).unwrap(),
            source_file
        );
        assert_eq!(std::fs::read(&source_file).unwrap(), b"original-bytes");
        assert_eq!(std::fs::metadata(&source_file).unwrap().ino(), inode);
        assert_eq!(std::fs::metadata(&source_file).unwrap().nlink(), 1);
        remove_link_tree(&series).unwrap();
        assert!(!series.exists());
        assert!(dest.join("book.epub").is_symlink());
        assert_eq!(std::fs::read(&source_file).unwrap(), b"original-bytes");
    }

    #[test]
    fn category_library_refuses_a_root_rename() {
        let dir = scratch();
        let source_file = dir.path().join("original.epub");
        std::fs::write(&source_file, b"original-bytes").unwrap();
        let src_root = dir.path().join("src-lib");
        let dst_root = dir.path().join("think");
        let series = src_root.join("Series");
        std::fs::create_dir_all(&series).unwrap();
        std::fs::create_dir_all(&dst_root).unwrap();
        symlink(&source_file, series.join("book.epub")).unwrap();

        let state = crate::service::series::tests::test_state();
        insert_library(&state, "src", "来源", &src_root);
        insert_library(&state, "dst", "分类书架", &dst_root);
        insert_series(&state, "s1", "src", &series);
        insert_book(&state, "b1", "s1", "src", &series.join("book.epub"));
        let nested = dst_root.join("甲类").join("子类");
        std::fs::create_dir_all(&nested).unwrap();
        insert_series(&state, "nested", "dst", &nested);
        assert!(library_has_nested_folders(&state, "dst", &path_to_url(&dst_root)).unwrap());
        assert!(!library_has_nested_folders(&state, "src", &path_to_url(&src_root)).unwrap());

        let error = move_series_to_library(&state, "s1", "dst", None).unwrap_err();
        assert!(error.to_string().contains("具体文件夹"), "{error}");
        assert!(series.join("book.epub").is_symlink());
        assert_eq!(std::fs::read(&source_file).unwrap(), b"original-bytes");
    }

    #[test]
    fn category_link_keeps_the_source_file_and_joins_an_existing_folder() {
        let dir = scratch();
        let source_file = dir.path().join("original.epub");
        std::fs::write(&source_file, b"original-bytes").unwrap();
        let inode = std::fs::metadata(&source_file).unwrap().ino();
        let src_root = dir.path().join("src-lib");
        let dst_root = dir.path().join("think");
        let series = src_root.join("某系列");
        let category = dst_root.join("哲学").join("伦理学");
        std::fs::create_dir_all(&series).unwrap();
        std::fs::create_dir_all(&category).unwrap();
        let link = series.join("book.epub");
        symlink(&source_file, &link).unwrap();
        let resident = category.join("already.epub");
        symlink(&source_file, &resident).unwrap();

        let state = crate::service::series::tests::test_state();
        insert_library(&state, "src", "来源", &src_root);
        insert_library(&state, "dst", "编程随想推荐书库", &dst_root);
        insert_series(&state, "s1", "src", &series);
        insert_book(&state, "b1", "s1", "src", &link);
        insert_series(&state, "ethics", "dst", &category);
        state
            .db
            .rw()
            .unwrap()
            .execute("UPDATE SERIES SET NAME = '伦理学' WHERE ID = 'ethics'", [])
            .unwrap();
        insert_book(&state, "b0", "ethics", "dst", &resident);

        let moved = move_series_to_library(&state, "s1", "dst", Some("哲学/伦理学"))
            .unwrap();
        assert_eq!(moved.series.id, "ethics");
        assert_eq!(moved.book_ids, vec!["b1".to_string()]);
        assert!(moved.removed.unwrap().deleted_date.is_some());
        assert!(!link.exists());
        assert!(category.join("book.epub").is_symlink());
        assert_eq!(
            std::fs::read_link(category.join("book.epub")).unwrap(),
            source_file
        );
        assert_eq!(std::fs::read(&source_file).unwrap(), b"original-bytes");
        assert_eq!(std::fs::metadata(&source_file).unwrap().ino(), inode);
        assert_eq!(std::fs::metadata(&source_file).unwrap().nlink(), 1);
        assert!(!series.exists());

        let conn = state.db.ro().unwrap();
        let (book_url, book_series, book_lock): (String, String, i64) = conn
            .query_row(
                "SELECT BOOK.URL, BOOK.SERIES_ID, BOOK_METADATA.TITLE_LOCK \
                 FROM BOOK JOIN BOOK_METADATA ON BOOK.ID = BOOK_METADATA.BOOK_ID WHERE BOOK.ID = 'b1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(book_series, "ethics");
        assert_eq!(book_lock, 1);
        assert_eq!(book_url, path_to_url(&category.join("book.epub")));
        let count: i64 = conn
            .query_row(
                "SELECT BOOK_COUNT FROM SERIES WHERE ID = 'ethics'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn category_link_creates_a_folder_series_when_the_folder_has_none() {
        let dir = scratch();
        let source_file = dir.path().join("original.epub");
        std::fs::write(&source_file, b"original-bytes").unwrap();
        let src_root = dir.path().join("src-lib");
        let dst_root = dir.path().join("think");
        let series = src_root.join("某系列");
        let category = dst_root.join("哲学");
        std::fs::create_dir_all(&series).unwrap();
        std::fs::create_dir_all(category.join("伦理学")).unwrap();
        symlink(&source_file, series.join("book.epub")).unwrap();

        let state = crate::service::series::tests::test_state();
        insert_library(&state, "src", "来源", &src_root);
        insert_library(&state, "dst", "编程随想推荐书库", &dst_root);
        insert_series(&state, "s1", "src", &series);
        insert_book(&state, "b1", "s1", "src", &series.join("book.epub"));

        let moved = move_series_to_library(&state, "s1", "dst", Some("哲学")).unwrap();
        assert_ne!(moved.series.id, "s1");
        assert_eq!(moved.series.url, path_to_url(&category));
        assert_eq!(moved.series.name, "哲学");
        assert!(category.join("book.epub").is_symlink());
        assert!(!series.join("book.epub").exists());
        assert_eq!(std::fs::read(&source_file).unwrap(), b"original-bytes");
        let (title, lock): (String, i64) = state
            .db
            .ro()
            .unwrap()
            .query_row(
                "SELECT TITLE, TITLE_LOCK FROM SERIES_METADATA WHERE SERIES_ID = ?1",
                [&moved.series.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(title, "哲学");
        assert_eq!(lock, 1);
    }

    #[test]
    fn regular_file_is_not_linked_into_a_category() {
        let dir = scratch();
        let src_root = dir.path().join("src-lib");
        let dst_root = dir.path().join("think");
        let series = src_root.join("Series");
        let category = dst_root.join("哲学");
        std::fs::create_dir_all(&series).unwrap();
        std::fs::create_dir_all(&category).unwrap();
        let book = series.join("book.epub");
        std::fs::write(&book, b"source-bytes").unwrap();

        let state = crate::service::series::tests::test_state();
        insert_library(&state, "src", "来源", &src_root);
        insert_library(&state, "dst", "编程随想推荐书库", &dst_root);
        insert_series(&state, "s1", "src", &series);
        insert_book(&state, "b1", "s1", "src", &book);

        let error = move_series_to_library(&state, "s1", "dst", Some("哲学")).unwrap_err();
        assert!(error.to_string().contains("普通文件"), "{error}");
        assert_eq!(std::fs::read(&book).unwrap(), b"source-bytes");
        assert!(!category.join("book.epub").exists());
        let library_id: String = state
            .db
            .ro()
            .unwrap()
            .query_row("SELECT LIBRARY_ID FROM SERIES WHERE ID = 's1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(library_id, "src");
    }

    #[test]
    fn category_database_failure_restores_the_old_link() {
        let dir = scratch();
        let source_file = dir.path().join("original.epub");
        std::fs::write(&source_file, b"original-bytes").unwrap();
        let src_root = dir.path().join("src-lib");
        let dst_root = dir.path().join("think");
        let series = src_root.join("Series");
        let category = dst_root.join("哲学");
        std::fs::create_dir_all(&series).unwrap();
        std::fs::create_dir_all(&category).unwrap();
        symlink(&source_file, series.join("book.epub")).unwrap();

        let state = crate::service::series::tests::test_state();
        insert_library(&state, "src", "来源", &src_root);
        insert_library(&state, "dst", "编程随想推荐书库", &dst_root);
        insert_series(&state, "s1", "src", &series);
        state
            .db
            .rw()
            .unwrap()
            .execute(
                "INSERT INTO BOOK (ID, NAME, URL, FILE_LAST_MODIFIED, SERIES_ID, LIBRARY_ID, NUMBER) \
                 VALUES ('b1', 'book.epub', ?1, '2020-01-01 00:00:00.0', 's1', 'src', 1)",
                [path_to_url(&series.join("book.epub"))],
            )
            .unwrap();

        let error = move_series_to_library(&state, "s1", "dst", Some("哲学")).unwrap_err();
        assert!(error.to_string().contains("已撤销"), "{error}");
        assert!(series.join("book.epub").is_symlink());
        assert!(!category.join("book.epub").exists());
        assert_eq!(std::fs::read(&source_file).unwrap(), b"original-bytes");
        let library_id: String = state
            .db
            .ro()
            .unwrap()
            .query_row("SELECT LIBRARY_ID FROM BOOK WHERE ID = 'b1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(library_id, "src");
    }
}
