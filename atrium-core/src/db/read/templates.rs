// SPDX-License-Identifier: MIT
//! Quick Entry template read helpers (Phase 18.5 Tier-1, v0.18.0).
//! Extracted from `read.rs` in the v0.21.0 maintenance pass —
//! templates are a self-contained surface (their own table, their
//! own JSON-decode quirk on `default_tags`).

use rusqlite::{Connection, Row, params};

use crate::domain::QuickEntryTemplate;
use crate::error::DbError;

const QUICK_ENTRY_TEMPLATE_COLUMNS: &str = "id, name, shortcut_key, target_project_id, prefix, default_tags, position, \
     created_at, modified_at";

/// Fetch a single template by id.
pub fn quick_entry_template_by_id(
    conn: &Connection,
    id: i64,
) -> Result<Option<QuickEntryTemplate>, DbError> {
    let sql =
        format!("SELECT {QUICK_ENTRY_TEMPLATE_COLUMNS} FROM quick_entry_template WHERE id = ?1");
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query_map(params![id], quick_entry_template_from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// All templates ordered by `position` (display order in the
/// Quick Entry modal's picker bar). Empty when no templates are
/// configured — modal renders the standard Quick Entry shape
/// in that case.
pub fn list_quick_entry_templates(conn: &Connection) -> Result<Vec<QuickEntryTemplate>, DbError> {
    let sql = format!(
        "SELECT {QUICK_ENTRY_TEMPLATE_COLUMNS} FROM quick_entry_template \
         ORDER BY position, id"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map([], quick_entry_template_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn quick_entry_template_from_row(row: &Row<'_>) -> rusqlite::Result<QuickEntryTemplate> {
    let tags_json: String = row.get("default_tags")?;
    // Tolerant decode: malformed JSON falls back to empty Vec
    // rather than failing the read. The worker writes valid JSON
    // (it's the only writer), so a malformed value here means
    // hand-edit damage; degrade to "no tags" rather than poison
    // the whole query.
    let default_tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    Ok(QuickEntryTemplate {
        id: row.get("id")?,
        name: row.get("name")?,
        shortcut_key: row.get("shortcut_key")?,
        target_project_id: row.get("target_project_id")?,
        prefix: row.get("prefix")?,
        default_tags,
        position: row.get("position")?,
        created_at: row.get("created_at")?,
        modified_at: row.get("modified_at")?,
    })
}
