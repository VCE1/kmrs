//! Shelf taxonomy. Categories group existing scan libraries. They do not move files.

use crate::auth::RequireAuth;
use crate::error::ApiError;
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::{routing, Json, Router};
use komga_core::model::catalog::{
    CatalogCategory, CatalogNodeDto, CatalogPageDto, CatalogSeriesDto,
};
use komga_db::dao::catalog::CatalogDao;
use komga_db::dao::library::LibraryDao;
use komga_db::dao::series::SeriesDao;
use rusqlite::params;
use serde::Deserialize;
use std::collections::{BTreeSet, HashMap};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/catalog", routing::get(list_roots))
        .route("/api/v1/catalog/{slug}", routing::get(get_category))
        .route(
            "/api/v1/catalog/{slug}/series/{series_id}",
            routing::put(assign_series),
        )
}

#[derive(Deserialize)]
struct PageQuery {
    page: Option<i64>,
    size: Option<i64>,
    directory: Option<String>,
}

#[derive(Clone)]
struct ShelfSeries {
    id: String,
    name: String,
    book_count: i32,
    library_id: String,
    library_name: String,
    url: String,
}

async fn list_roots(
    State(state): State<AppState>,
    auth: RequireAuth,
) -> Result<Json<Vec<CatalogNodeDto>>, ApiError> {
    let categories = CatalogDao::new(state.kmrs_db.clone()).list_all()?;
    let counts = series_counts(&state, &auth.0.user, &categories)?;
    Ok(Json(tree(&categories, &counts, None)))
}

async fn get_category(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path(slug): Path<String>,
    Query(query): Query<PageQuery>,
) -> Result<Json<CatalogPageDto>, ApiError> {
    let dao = CatalogDao::new(state.kmrs_db.clone());
    let categories = dao.list_all()?;
    let current = categories
        .iter()
        .find(|category| category.slug == slug)
        .cloned()
        .ok_or_else(|| ApiError::not_found("Category not found"))?;
    let counts = series_counts(&state, &auth.0.user, &categories)?;
    let page = query.page.unwrap_or(0).max(0);
    let size = query.size.unwrap_or(48).clamp(1, 200);
    let directory = normalize_directory(query.directory.as_deref())?;
    if !directory.is_empty() && !current.directory_groups {
        return Err(ApiError::bad_request(
            "This category does not group directories",
        ));
    }
    let series = visible_series(&state, &auth.0.user, &current)?;
    let (category_node, ancestor_nodes, series) = if current.directory_groups {
        let libraries = LibraryDao::new(state.db.clone()).find_all()?;
        let root = libraries
            .iter()
            .find(|library| {
                library.name == current.bound_library_name && !current.bound_library_name.is_empty()
            })
            .map(|library| library.root.clone())
            .unwrap_or_default();
        let view = group_directories(&current, &series, &root, &directory);
        let mut category_node = node(&current, &counts, &categories);
        category_node.children = view.children;
        let mut ancestor_nodes = ancestors(&categories, &counts, &current);
        if !directory.is_empty() {
            category_node.name = directory
                .rsplit('/')
                .next()
                .unwrap_or(category_node.name.as_str())
                .to_string();
            category_node.directory = directory.clone();
            let self_crumb = bare(&current, &counts);
            ancestor_nodes.push(self_crumb);
            let parts: Vec<&str> = directory.split('/').collect();
            let mut acc = String::new();
            for (index, part) in parts.iter().enumerate() {
                if index + 1 == parts.len() {
                    break;
                }
                acc = if acc.is_empty() {
                    (*part).to_string()
                } else {
                    format!("{acc}/{part}")
                };
                let mut crumb = bare(&current, &counts);
                crumb.name = (*part).to_string();
                crumb.directory = acc.clone();
                ancestor_nodes.push(crumb);
            }
        }
        (category_node, ancestor_nodes, view.series)
    } else {
        (
            node(&current, &counts, &categories),
            ancestors(&categories, &counts, &current),
            series,
        )
    };
    let total = series.len() as i64;
    let start = (page as usize).saturating_mul(size as usize);
    let content = series
        .into_iter()
        .skip(start)
        .take(size as usize)
        .map(into_series_dto)
        .collect();
    let shown = (start as i64 + size).min(total);
    Ok(Json(CatalogPageDto {
        category: category_node,
        ancestors: ancestor_nodes,
        content,
        total_elements: total,
        number: page,
        size,
        last: shown >= total,
    }))
}

async fn assign_series(
    State(state): State<AppState>,
    auth: RequireAuth,
    Path((slug, series_id)): Path<(String, String)>,
) -> Result<Json<CatalogSeriesDto>, ApiError> {
    auth.0.require_admin()?;
    let dao = CatalogDao::new(state.kmrs_db.clone());
    if dao.find_by_slug(&slug)?.is_none() {
        return Err(ApiError::not_found("Category not found"));
    }
    let series = SeriesDao::new(state.db.clone())
        .find_by_id(&series_id)?
        .ok_or_else(|| ApiError::not_found("Series not found"))?;
    if series.deleted_date.is_some() {
        return Err(ApiError::not_found("Series not found"));
    }
    let library = LibraryDao::new(state.db.clone())
        .find_by_id(&series.library_id)?
        .ok_or_else(|| ApiError::not_found("Library not found"))?;
    dao.assign_series(&slug, &series.id)?;
    Ok(Json(CatalogSeriesDto {
        id: series.id,
        name: series.name,
        book_count: series.book_count,
        library_id: series.library_id,
        library_name: library.name,
    }))
}

fn into_series_dto(series: ShelfSeries) -> CatalogSeriesDto {
    CatalogSeriesDto {
        id: series.id,
        name: series.name,
        book_count: series.book_count,
        library_id: series.library_id,
        library_name: series.library_name,
    }
}

fn node(
    category: &CatalogCategory,
    counts: &HashMap<String, i64>,
    categories: &[CatalogCategory],
) -> CatalogNodeDto {
    let mut dto = bare(category, counts);
    dto.children = categories
        .iter()
        .filter(|child| child.parent_slug.as_deref() == Some(category.slug.as_str()))
        .map(|child| bare(child, counts))
        .collect();
    dto
}

fn bare(category: &CatalogCategory, counts: &HashMap<String, i64>) -> CatalogNodeDto {
    CatalogNodeDto {
        slug: category.slug.clone(),
        parent_slug: category.parent_slug.clone(),
        name: category.name.clone(),
        sort_order: category.sort_order,
        bound_library_name: category.bound_library_name.clone(),
        note: category.note.clone(),
        series_count: counts.get(&category.slug).copied().unwrap_or(0),
        directory: String::new(),
        children: Vec::new(),
    }
}

fn tree(
    categories: &[CatalogCategory],
    counts: &HashMap<String, i64>,
    parent: Option<&str>,
) -> Vec<CatalogNodeDto> {
    categories
        .iter()
        .filter(|category| category.parent_slug.as_deref() == parent)
        .map(|category| {
            let mut dto = bare(category, counts);
            dto.children = tree(categories, counts, Some(category.slug.as_str()));
            dto
        })
        .collect()
}

fn ancestors(
    categories: &[CatalogCategory],
    counts: &HashMap<String, i64>,
    current: &CatalogCategory,
) -> Vec<CatalogNodeDto> {
    let mut chain = Vec::new();
    let mut parent = current.parent_slug.clone();
    while let Some(slug) = parent {
        let Some(category) = categories.iter().find(|item| item.slug == slug) else {
            break;
        };
        chain.push(bare(category, counts));
        parent = category.parent_slug.clone();
    }
    chain.reverse();
    chain
}

fn series_counts(
    state: &AppState,
    user: &komga_core::model::user::KomgaUser,
    categories: &[CatalogCategory],
) -> Result<HashMap<String, i64>, ApiError> {
    let mut direct = HashMap::new();
    for category in categories {
        direct.insert(
            category.slug.clone(),
            visible_series(state, user, category)?.len() as i64,
        );
    }
    let mut totals = HashMap::new();
    for category in categories {
        totals.insert(
            category.slug.clone(),
            rollup(&direct, categories, &category.slug),
        );
    }
    Ok(totals)
}

fn rollup(direct: &HashMap<String, i64>, categories: &[CatalogCategory], slug: &str) -> i64 {
    let own = direct.get(slug).copied().unwrap_or(0);
    let children: i64 = categories
        .iter()
        .filter(|category| category.parent_slug.as_deref() == Some(slug))
        .map(|category| rollup(direct, categories, &category.slug))
        .sum();
    own + children
}

fn visible_series(
    state: &AppState,
    user: &komga_core::model::user::KomgaUser,
    category: &CatalogCategory,
) -> Result<Vec<ShelfSeries>, ApiError> {
    let libraries = LibraryDao::new(state.db.clone()).find_all()?;
    let allowed: BTreeSet<String> = libraries
        .iter()
        .filter(|library| user.can_access_library(&library.id))
        .map(|library| library.id.clone())
        .collect();
    let names: HashMap<String, String> = libraries
        .iter()
        .map(|library| (library.id.clone(), library.name.clone()))
        .collect();
    let bound_id = libraries
        .iter()
        .find(|library| library.name == category.bound_library_name && !category.bound_library_name.is_empty())
        .map(|library| library.id.clone());
    let explicit = CatalogDao::new(state.kmrs_db.clone()).series_ids_for(&category.slug)?;
    let mut series = Vec::new();
    let conn = state.db.ro()?;
    if let Some(library_id) = bound_id.filter(|id| allowed.contains(id)) {
        let mut stmt = conn.prepare(
            "SELECT ID, NAME, BOOK_COUNT, LIBRARY_ID, URL FROM SERIES \
             WHERE LIBRARY_ID = ? AND DELETED_DATE IS NULL",
        )
        .map_err(|err| ApiError::Internal(err.to_string()))?;
        let rows = stmt.query_map(params![library_id], |row| {
            Ok(ShelfSeries {
                id: row.get(0)?,
                name: row.get(1)?,
                book_count: row.get(2)?,
                library_id: row.get(3)?,
                library_name: String::new(),
                url: row.get(4)?,
            })
        })
        .map_err(|err| ApiError::Internal(err.to_string()))?;
        for row in rows {
            series.push(row.map_err(|err| ApiError::Internal(err.to_string()))?);
        }
    }
    if !explicit.is_empty() {
        let mut have: BTreeSet<String> = series.iter().map(|item| item.id.clone()).collect();
        for chunk in explicit.chunks(400) {
            let placeholders = chunk.iter().map(|_| "?").collect::<Vec<_>>().join(", ");
            let sql = format!(
                "SELECT ID, NAME, BOOK_COUNT, LIBRARY_ID, URL FROM SERIES \
                 WHERE DELETED_DATE IS NULL AND ID IN ({placeholders})"
            );
            let mut stmt = conn
                .prepare(&sql)
                .map_err(|err| ApiError::Internal(err.to_string()))?;
            let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                Ok(ShelfSeries {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    book_count: row.get(2)?,
                    library_id: row.get(3)?,
                    library_name: String::new(),
                    url: row.get(4)?,
                })
            })
            .map_err(|err| ApiError::Internal(err.to_string()))?;
            for row in rows {
                let item = row.map_err(|err| ApiError::Internal(err.to_string()))?;
                if have.contains(&item.id) || !allowed.contains(&item.library_id) {
                    continue;
                }
                have.insert(item.id.clone());
                series.push(item);
            }
        }
    }
    for item in &mut series {
        item.library_name = names.get(&item.library_id).cloned().unwrap_or_default();
    }
    series.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    Ok(series)
}

struct FolderView {
    children: Vec<CatalogNodeDto>,
    series: Vec<ShelfSeries>,
}

/// Folders come from the series directory, not from a hardcoded genre list.
fn group_directories(
    category: &CatalogCategory,
    series: &[ShelfSeries],
    library_root: &str,
    directory: &str,
) -> FolderView {
    if library_root.is_empty() {
        return FolderView {
            children: Vec::new(),
            series: series.to_vec(),
        };
    }
    let mut counts: HashMap<String, i64> = HashMap::new();
    let mut direct = Vec::new();
    for item in series {
        let Some(rel) = relative_directory(&item.url, library_root) else {
            // The series is on this shelf, but its path is not under the library root.
            // Keep it on the top page so a bad URL does not hide the book.
            if directory.is_empty() {
                direct.push(item.clone());
            }
            continue;
        };
        let parent = match rel.rfind('/') {
            Some(index) => &rel[..index],
            None => "",
        };
        if parent == directory {
            direct.push(item.clone());
            continue;
        }
        let child = if directory.is_empty() {
            parent.split('/').next().unwrap_or("")
        } else {
            let prefix = format!("{directory}/");
            match parent.strip_prefix(&prefix) {
                Some(rest) => rest.split('/').next().unwrap_or(""),
                None => continue,
            }
        };
        if child.is_empty() {
            continue;
        }
        let path = if directory.is_empty() {
            child.to_string()
        } else {
            format!("{directory}/{child}")
        };
        *counts.entry(path).or_insert(0) += 1;
    }
    let mut paths: Vec<String> = counts.keys().cloned().collect();
    paths.sort_by(|left, right| {
        let left_leaf = left.rsplit('/').next().unwrap_or(left);
        let right_leaf = right.rsplit('/').next().unwrap_or(right);
        left_leaf.to_lowercase().cmp(&right_leaf.to_lowercase())
    });
    let children = paths
        .into_iter()
        .map(|path| {
            let name = path.rsplit('/').next().unwrap_or(&path).to_string();
            let series_count = counts[&path];
            CatalogNodeDto {
                slug: category.slug.clone(),
                parent_slug: Some(category.slug.clone()),
                name,
                sort_order: 0,
                bound_library_name: String::new(),
                note: String::new(),
                series_count,
                directory: path,
                children: Vec::new(),
            }
        })
        .collect();
    FolderView {
        children,
        series: direct,
    }
}

fn relative_directory(series_url: &str, library_root: &str) -> Option<String> {
    let series = trim_path(&percent_decode(series_url));
    let root = trim_path(&percent_decode(library_root));
    if series == root {
        return None;
    }
    let rest = series.strip_prefix(&format!("{root}/"))?;
    if rest.is_empty() {
        None
    } else {
        Some(rest.to_string())
    }
}

fn trim_path(url: &str) -> String {
    url.strip_prefix("file:")
        .unwrap_or(url)
        .trim_matches('/')
        .to_string()
}

fn normalize_directory(raw: Option<&str>) -> Result<String, ApiError> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(String::new());
    };
    let decoded = percent_decode(raw);
    let mut parts = Vec::new();
    for part in decoded.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." || part.contains('\\') || part.contains('\0') {
            return Err(ApiError::bad_request("Invalid directory"));
        }
        parts.push(part.to_string());
    }
    Ok(parts.join("/"))
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).ok();
            if let Some(value) = hex.and_then(|text| u8::from_str_radix(text, 16).ok()) {
                out.push(value);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::libraries::test_support::{insert_api_key, insert_library, insert_user, TestApp};
    use axum::http::{Request, StatusCode};
    use axum::body::Body;

    fn app() -> TestApp {
        TestApp::new(router())
    }

    fn insert_series(db: &komga_db::pool::Database, id: &str, name: &str, library_id: &str) {
        insert_series_at(db, id, name, library_id, &format!("file:/data/{id}/"));
    }

    fn insert_series_at(
        db: &komga_db::pool::Database,
        id: &str,
        name: &str,
        library_id: &str,
        url: &str,
    ) {
        let now = now_utc();
        SeriesDao::new(db.clone())
            .insert(&Series {
                id: id.into(),
                name: name.into(),
                url: url.into(),
                file_last_modified: now,
                library_id: library_id.into(),
                book_count: 2,
                deleted_date: None,
                oneshot: false,
                created_date: now,
                last_modified_date: now,
            })
            .unwrap();
    }

    #[tokio::test]
    async fn seeded_tree_keeps_programthink_independent() {
        let app = app();
        let admin = insert_user(&app.state.db, "admin@x.c", true, true, &[]);
        insert_api_key(&app.state.db, &admin, "k-admin");
        let (status, body) = app.get_json("/api/v1/catalog", "k-admin").await;
        assert_eq!(status, StatusCode::OK);
        let slugs: Vec<&str> = body
            .as_array()
            .unwrap()
            .iter()
            .map(|node| node["slug"].as_str().unwrap())
            .collect();
        assert_eq!(
            slugs,
            vec!["light-novel", "manga", "programthink", "pending", "misc"]
        );
        let light = &body[0];
        assert_eq!(light["boundLibraryName"], "");
        assert_eq!(light["children"][0]["slug"], "light-novel-refined");
        assert_eq!(light["children"][0]["boundLibraryName"], "精排日轻");
        assert_eq!(light["children"][1]["boundLibraryName"], "日文轻小说");
        assert_eq!(body[2]["parentSlug"], serde_json::Value::Null);
        assert_eq!(body[2]["boundLibraryName"], "编程随想推荐书库");
        assert!(body[2]["note"].as_str().unwrap().contains("独立分类"));
        assert_eq!(body[3]["boundLibraryName"], "");
        assert!(body[4]["note"].as_str().unwrap().contains("不自动归入"));
    }

    #[tokio::test]
    async fn bound_library_series_are_listed_and_hidden_without_access() {
        let app = app();
        insert_library(&app.state.db, "lib-pt", "编程随想推荐书库");
        insert_series(&app.state.db, "s-pt", "编程随想文集", "lib-pt");
        let admin = insert_user(&app.state.db, "admin@x.c", true, true, &[]);
        insert_api_key(&app.state.db, &admin, "k-admin");
        let stranger = insert_user(&app.state.db, "user@x.c", false, false, &[]);
        insert_api_key(&app.state.db, &stranger, "k-user");

        let (status, body) = app
            .get_json("/api/v1/catalog/programthink", "k-admin")
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["totalElements"], 1);
        assert_eq!(body["content"][0]["name"], "编程随想文集");
        assert_eq!(body["category"]["seriesCount"], 1);

        let (status, body) = app.get_json("/api/v1/catalog/programthink", "k-user").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["totalElements"], 0);
    }

    #[tokio::test]
    async fn assign_moves_a_series_onto_pending_without_using_old_genres() {
        let app = app();
        insert_library(&app.state.db, "lib-misc", "文学名作");
        insert_series(&app.state.db, "s-misc", "未判定作品", "lib-misc");
        let admin = insert_user(&app.state.db, "admin@x.c", true, true, &[]);
        insert_api_key(&app.state.db, &admin, "k-admin");

        let (status, _) = app.get_json("/api/v1/catalog/misc", "k-admin").await;
        assert_eq!(status, StatusCode::OK);

        let request = Request::put("/api/v1/catalog/pending/series/s-misc")
            .header("X-API-Key", "k-admin")
            .body(Body::empty())
            .unwrap();
        let (status, _, bytes) = app.request(request).await;
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&bytes));

        let (status, body) = app.get_json("/api/v1/catalog/pending", "k-admin").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["totalElements"], 1);
        assert_eq!(body["content"][0]["id"], "s-misc");
        let (status, body) = app.get_json("/api/v1/catalog/misc", "k-admin").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["totalElements"], 0);
    }

    #[tokio::test]
    async fn programthink_keeps_same_folder_names_under_their_own_parent() {
        let app = app();
        insert_library(&app.state.db, "lib-pt", "编程随想推荐书库");
        let root = "file:/data/编程随想推荐书库";
        insert_series_at(
            &app.state.db,
            "s-py",
            "Python",
            "lib-pt",
            &format!("{root}/IT/Python/"),
        );
        insert_series_at(
            &app.state.db,
            "s-sec",
            "渗透",
            "lib-pt",
            &format!("{root}/IT/%E4%BF%A1%E6%81%AF%E5%AE%89%E5%85%A8/渗透/"),
        );
        insert_series_at(
            &app.state.db,
            "s-cn",
            "中国",
            "lib-pt",
            &format!("{root}/政治/中国/"),
        );
        insert_series_at(
            &app.state.db,
            "s-out",
            "别处的Python",
            "lib-pt",
            "file:/elsewhere/IT/Python/",
        );
        let admin = insert_user(&app.state.db, "admin@x.c", true, true, &[]);
        insert_api_key(&app.state.db, &admin, "k-admin");

        let (status, body) = app.get_json("/api/v1/catalog/programthink", "k-admin").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["totalElements"], 1);
        assert_eq!(body["content"][0]["id"], "s-out");
        let names: Vec<&str> = body["category"]["children"]
            .as_array()
            .unwrap()
            .iter()
            .map(|node| node["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["IT", "政治"]);
        assert_eq!(body["category"]["children"][0]["directory"], "IT");
        assert_eq!(body["category"]["children"][0]["seriesCount"], 2);
        assert_eq!(body["category"]["children"][1]["seriesCount"], 1);

        let (status, body) = app
            .get_json("/api/v1/catalog/programthink?directory=IT", "k-admin")
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["category"]["name"], "IT");
        assert_eq!(body["content"][0]["name"], "Python");
        assert_eq!(body["totalElements"], 1);
        assert_eq!(body["category"]["children"][0]["name"], "信息安全");
        assert_eq!(body["category"]["children"][0]["directory"], "IT/信息安全");
        assert_eq!(body["ancestors"][0]["name"], "编程随想");
        assert_eq!(body["ancestors"][0]["directory"], "");

        let (status, body) = app
            .get_json(
                "/api/v1/catalog/programthink?directory=IT/%E4%BF%A1%E6%81%AF%E5%AE%89%E5%85%A8",
                "k-admin",
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["category"]["name"], "信息安全");
        assert_eq!(body["content"][0]["id"], "s-sec");
        assert_eq!(body["category"]["children"].as_array().unwrap().len(), 0);
        assert_eq!(body["ancestors"][1]["directory"], "IT");

        let (status, _) = app
            .get_json("/api/v1/catalog/programthink?directory=../IT", "k-admin")
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        let (status, _) = app
            .get_json("/api/v1/catalog/light-novel?directory=IT", "k-admin")
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
