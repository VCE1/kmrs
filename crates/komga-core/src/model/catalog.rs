//! Fork shelf taxonomy. Categories are not scan libraries and not filesystem folders.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogCategory {
    pub slug: String,
    pub parent_slug: Option<String>,
    pub name: String,
    pub sort_order: i32,
    pub bound_library_name: String,
    pub note: String,
    /// When set, the shelf splits into the bound library's folders.
    pub directory_groups: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogNodeDto {
    pub slug: String,
    pub parent_slug: Option<String>,
    pub name: String,
    pub sort_order: i32,
    pub bound_library_name: String,
    pub note: String,
    pub series_count: i64,
    /// Relative folder inside a directory-grouped shelf. Empty for a seeded category.
    pub directory: String,
    pub children: Vec<CatalogNodeDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogSeriesDto {
    pub id: String,
    pub name: String,
    pub book_count: i32,
    pub library_id: String,
    pub library_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPageDto {
    pub category: CatalogNodeDto,
    pub ancestors: Vec<CatalogNodeDto>,
    pub content: Vec<CatalogSeriesDto>,
    pub total_elements: i64,
    pub number: i64,
    pub size: i64,
    pub last: bool,
}
