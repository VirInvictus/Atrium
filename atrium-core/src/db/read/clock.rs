// SPDX-License-Identifier: MIT
//! Clock-entry read helpers (Phase 18.5 Tier-1, v0.17.0; timestamps
//! added v0.21.0). Extracted from `read.rs` in the v0.21.0
//! maintenance pass — the clock surface is self-contained
//! (its own table, its own row helper) and benefits from living
//! in its own file.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::domain::TaskClockEntry;
use crate::error::DbError;

const CLOCK_COLUMNS: &str = "id, task_id, started_at, ended_at, note, created_at, modified_at";

/// Fetch a single clock entry by id.
pub fn clock_entry_by_id(conn: &Connection, id: i64) -> Result<Option<TaskClockEntry>, DbError> {
    let sql = format!("SELECT {CLOCK_COLUMNS} FROM task_clock_entry WHERE id = ?1");
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query_map(params![id], clock_entry_from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// Resolve the task_id of a clock entry without loading the
/// full row. Used by the worker's delete path so it can look
/// up which task to refresh + notify after the row is gone.
pub fn clock_entry_task_id(conn: &Connection, id: i64) -> Result<Option<i64>, DbError> {
    let mut stmt = conn.prepare_cached("SELECT task_id FROM task_clock_entry WHERE id = ?1")?;
    let mut rows = stmt.query_map(params![id], |r| r.get::<_, i64>(0))?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// All clock entries on a task, newest-first by started_at
/// (Inspector log convention; Emacs's `org-clock` also lists
/// recent on top).
pub fn list_clock_entries(conn: &Connection, task_id: i64) -> Result<Vec<TaskClockEntry>, DbError> {
    let sql = format!(
        "SELECT {CLOCK_COLUMNS} FROM task_clock_entry \
         WHERE task_id = ?1 ORDER BY started_at DESC, id DESC"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![task_id], clock_entry_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Sum of closed-entry durations for a task, in whole minutes.
/// In-progress entries (ended_at IS NULL) are skipped — the
/// "right" answer for them depends on the moment you ask, and
/// the inspector renders the running clock separately. Returns
/// `0` for tasks with no entries (Inspector renders "0:00" the
/// same way it would render an empty log).
pub fn total_clock_minutes(conn: &Connection, task_id: i64) -> Result<i64, DbError> {
    let mut stmt = conn.prepare_cached(
        "SELECT COALESCE(SUM( (julianday(ended_at) - julianday(started_at)) * 24 * 60 ), 0) \
         FROM task_clock_entry WHERE task_id = ?1 AND ended_at IS NOT NULL",
    )?;
    // SQLite returns the SUM as REAL; round to whole minutes.
    let total: f64 = stmt.query_row(params![task_id], |r| r.get(0))?;
    Ok(total.max(0.0).round() as i64)
}

/// All clock entries belonging to tasks in a project, grouped
/// by task_id. Newest-first within each group. Used by the Org
/// writer to stamp `:LOGBOOK:` drawers in one query rather than
/// per-task. Tasks with no entries are absent from the map.
pub fn clock_entries_per_project(
    conn: &Connection,
    project_id: i64,
) -> Result<HashMap<i64, Vec<TaskClockEntry>>, DbError> {
    // Qualify every CLOCK_COLUMNS field with `e.` so the JOIN
    // doesn't make `id` ambiguous against `task.id`.
    let qualified = CLOCK_COLUMNS
        .split(", ")
        .map(|c| format!("e.{c}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT {qualified} FROM task_clock_entry e \
         JOIN task t ON e.task_id = t.id \
         WHERE t.project_id = ?1 \
         ORDER BY e.task_id, e.started_at DESC, e.id DESC"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![project_id], clock_entry_from_row)?;
    let mut out: HashMap<i64, Vec<TaskClockEntry>> = HashMap::new();
    for row in rows {
        let entry = row?;
        out.entry(entry.task_id).or_default().push(entry);
    }
    Ok(out)
}

/// Identify the currently-running clock (at most one row per
/// the single-active-clock invariant). Returns `(task_id,
/// started_at)` for the running entry, or `None` when no clock
/// is active.
pub fn active_clock(conn: &Connection) -> Result<Option<(i64, DateTime<Utc>)>, DbError> {
    let row = conn
        .query_row(
            "SELECT task_id, started_at FROM task_clock_entry WHERE ended_at IS NULL LIMIT 1",
            [],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, DateTime<Utc>>(1)?)),
        )
        .optional()?;
    Ok(row)
}

fn clock_entry_from_row(row: &Row<'_>) -> rusqlite::Result<TaskClockEntry> {
    Ok(TaskClockEntry {
        id: row.get("id")?,
        task_id: row.get("task_id")?,
        started_at: row.get("started_at")?,
        ended_at: row.get("ended_at")?,
        note: row.get("note")?,
        created_at: row.get("created_at")?,
        modified_at: row.get("modified_at")?,
    })
}
