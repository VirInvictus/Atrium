// SPDX-License-Identifier: MIT
//! Quick Entry template read helpers (Phase 18.5 Tier-1, v0.18.0).
//! Extracted from `read.rs` in the v0.21.0 maintenance pass —
//! templates are a self-contained surface (their own table, their
//! own JSON-decode quirk on `default_tags`).

use rusqlite::{Connection, Row, params};

use crate::domain::{QuickEntryTemplate, TaskTemplate, TaskTemplateItem};
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

// ── Task templates (v0.33.0) ─────────────────────────────────────

const TASK_TEMPLATE_COLUMNS: &str =
    "id, uuid, name, project_title_seed, note, tags_json, created_at, modified_at";

/// All task templates, ordered by name (case-insensitive).
pub fn list_task_templates(conn: &Connection) -> Result<Vec<TaskTemplate>, DbError> {
    let sql = format!(
        "SELECT {TASK_TEMPLATE_COLUMNS} FROM task_template ORDER BY name COLLATE NOCASE, id"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map([], task_template_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Single task template by id.
pub fn task_template_by_id(conn: &Connection, id: i64) -> Result<Option<TaskTemplate>, DbError> {
    let sql = format!("SELECT {TASK_TEMPLATE_COLUMNS} FROM task_template WHERE id = ?1");
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query_map(params![id], task_template_from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// Single task template by name (case-insensitive). Backs the CLI's
/// `task-template instantiate NAME` / `delete NAME`.
pub fn task_template_by_name(
    conn: &Connection,
    name: &str,
) -> Result<Option<TaskTemplate>, DbError> {
    let sql = format!(
        "SELECT {TASK_TEMPLATE_COLUMNS} FROM task_template WHERE name = ?1 COLLATE NOCASE LIMIT 1"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query_map(params![name], task_template_from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// The items of a template, ordered by `position` — the order they're
/// stamped out, and the order `parent_index` references.
pub fn task_template_items(
    conn: &Connection,
    template_id: i64,
) -> Result<Vec<TaskTemplateItem>, DbError> {
    let sql = "SELECT id, template_id, title, parent_index, position, estimated_minutes, \
               default_tags_json FROM task_template_item WHERE template_id = ?1 \
               ORDER BY position, id";
    let mut stmt = conn.prepare_cached(sql)?;
    let rows = stmt.query_map(params![template_id], task_template_item_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn task_template_from_row(row: &Row<'_>) -> rusqlite::Result<TaskTemplate> {
    let tags_json: String = row.get("tags_json")?;
    let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    Ok(TaskTemplate {
        id: row.get("id")?,
        uuid: row.get("uuid")?,
        name: row.get("name")?,
        project_title_seed: row.get("project_title_seed")?,
        note: row.get("note")?,
        tags,
        created_at: row.get("created_at")?,
        modified_at: row.get("modified_at")?,
    })
}

fn task_template_item_from_row(row: &Row<'_>) -> rusqlite::Result<TaskTemplateItem> {
    let tags_json: String = row.get("default_tags_json")?;
    let default_tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
    Ok(TaskTemplateItem {
        id: row.get("id")?,
        template_id: row.get("template_id")?,
        title: row.get("title")?,
        parent_index: row.get("parent_index")?,
        position: row.get("position")?,
        estimated_minutes: row.get("estimated_minutes")?,
        default_tags,
    })
}
