//! Shelf categories live in kmrs.sqlite. Series rows stay in the main database.

use crate::error::Result;
use crate::pool::Database;
use komga_core::model::catalog::CatalogCategory;
use rusqlite::Row;

const COLUMNS: &str =
    "SLUG, PARENT_SLUG, NAME, SORT_ORDER, BOUND_LIBRARY_NAME, NOTE, DIRECTORY_GROUPS";

pub struct CatalogDao {
    db: Database,
}

fn row_to_category(row: &Row<'_>) -> rusqlite::Result<CatalogCategory> {
    let parent: Option<String> = row.get(1)?;
    Ok(CatalogCategory {
        slug: row.get(0)?,
        parent_slug: parent.filter(|value| !value.is_empty()),
        name: row.get(2)?,
        sort_order: row.get(3)?,
        bound_library_name: row.get(4)?,
        note: row.get(5)?,
        directory_groups: row.get::<_, i32>(6)? != 0,
    })
}

impl CatalogDao {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn list_all(&self) -> Result<Vec<CatalogCategory>> {
        let conn = self.db.ro()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM CATALOG_CATEGORY ORDER BY SORT_ORDER, NAME"
        ))?;
        let rows = stmt
            .query_map([], row_to_category)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn find_by_slug(&self, slug: &str) -> Result<Option<CatalogCategory>> {
        let conn = self.db.ro()?;
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM CATALOG_CATEGORY WHERE SLUG = ?"
        ))?;
        let rows = stmt
            .query_map([slug], row_to_category)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().next())
    }

    pub fn series_ids_for(&self, slug: &str) -> Result<Vec<String>> {
        let conn = self.db.ro()?;
        let mut stmt =
            conn.prepare("SELECT SERIES_ID FROM CATALOG_SERIES WHERE CATEGORY_SLUG = ?")?;
        let rows = stmt
            .query_map([slug], |row| row.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(rows)
    }

    /// One series belongs to one category. Reassigning moves it.
    pub fn assign_series(&self, slug: &str, series_id: &str) -> Result<()> {
        let conn = self.db.rw()?;
        conn.execute(
            "INSERT INTO CATALOG_SERIES (SERIES_ID, CATEGORY_SLUG) VALUES (?, ?) \
             ON CONFLICT(SERIES_ID) DO UPDATE SET CATEGORY_SLUG = excluded.CATEGORY_SLUG",
            [series_id, slug],
        )?;
        Ok(())
    }
}
