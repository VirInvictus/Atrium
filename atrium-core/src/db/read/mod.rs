// SPDX-License-Identifier: MIT
//! Read-side query helpers. Free functions that take a borrowed
//! `Connection` so they compose with both the worker's writable
//! connection (during command processing) and the `ReadPool`'s
//! read-only connections (during UI list refreshes).
//!
//! Split into per-surface sub-modules in v0.21.0 (maintenance pass)
//! after the file grew past 2200 lines. The shared task row helper
//! and column constant live in this file (`mod.rs`) so every
//! sub-module can reach them via `super::`.

mod clock;
mod counts;
mod search;
mod templates;

pub use clock::{
    active_clock, clock_entries_per_project, clock_entry_by_id, clock_entry_task_id,
    list_clock_entries, total_clock_minutes,
};
pub use counts::{
    CanonicalCounts, count_done_total_per_parent, count_done_total_per_project,
    count_open_canonical, count_open_per_area, count_open_per_project, count_open_per_tag,
    count_tasks,
};
pub use search::{SqlBindValue, bm25_for_terms, list_tasks_matching, search_tasks};
pub use templates::{
    list_quick_entry_templates, list_task_templates, quick_entry_template_by_id,
    task_template_by_id, task_template_by_name, task_template_items,
};

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::domain::{Area, Perspective, Project, ScheduledFor, Tag, Task};
use crate::error::DbError;

pub(super) const TASK_COLUMNS: &str = "id, uuid, title, note, project_id, parent_id, \
    scheduled_for, deadline, defer_until, estimated_minutes, completed_at, \
    repeat_rule, repeat_mode, last_reviewed_at, orig_keyword, deadline_warn_days, \
    scheduled_time, reminder_at, extra_properties, position, created_at, modified_at";

/// `TASK_COLUMNS` with every column prefixed `t.`, for the queries that
/// alias the task table as `t` and join it against another table. The
/// split/map/join was previously recomputed on every list-refresh query
/// at five call sites; this computes it once.
pub(super) static TASK_COLUMNS_T: LazyLock<String> = LazyLock::new(|| {
    TASK_COLUMNS
        .split(", ")
        .map(|c| format!("t.{c}"))
        .collect::<Vec<_>>()
        .join(", ")
});

/// Fetch a single task by primary key.
pub fn task_by_id(conn: &Connection, id: i64) -> Result<Option<Task>, DbError> {
    let sql = format!("SELECT {TASK_COLUMNS} FROM task WHERE id = ?1");
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query_map(params![id], task_from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// v0.20.0 — Phase 19.5 reminder service queue. Returns the
/// soonest open task whose `reminder_at` is strictly after
/// `after`. The reminder service uses this to set its sleep
/// timer; re-queries on every TaskChanges so a freshly-set
/// reminder takes effect without a service restart. Returns
/// `(task_id, reminder_at)` or `None` when no pending reminder
/// exists.
pub fn next_pending_reminder(
    conn: &Connection,
    after: DateTime<Utc>,
) -> Result<Option<(i64, DateTime<Utc>)>, DbError> {
    // Bind `after` as a DateTime so rusqlite serialises it exactly the
    // way it serialised the stored `reminder_at` on write. The old code
    // hand-formatted the bound string with a divergent shape (a `Z`
    // suffix vs rusqlite's `+00:00`, and it even dropped the seconds
    // field), which made the boundary comparison unreliable.
    let row = conn
        .query_row(
            "SELECT id, reminder_at FROM task \
             WHERE reminder_at IS NOT NULL \
               AND completed_at IS NULL \
               AND reminder_at > ?1 \
             ORDER BY reminder_at ASC LIMIT 1",
            params![after],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, DateTime<Utc>>(1)?)),
        )
        .optional()?;
    Ok(row)
}

/// v0.19.0 — Phase 18.5 Tier-2 Org-link target resolution.
/// Returns the rowid of the task whose `uuid` matches; `None`
/// for stale UUIDs (link points to a deleted task) so the
/// caller can no-op silently rather than treat it as an error.
pub fn task_id_for_uuid(conn: &Connection, uuid: &str) -> Result<Option<i64>, DbError> {
    let mut stmt = conn.prepare_cached("SELECT id FROM task WHERE uuid = ?1")?;
    let mut rows = stmt.query_map(params![uuid], |r| r.get::<_, i64>(0))?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// Inbox per spec §4.2: open tasks with no project assignment.
pub fn list_inbox(conn: &Connection) -> Result<Vec<Task>, DbError> {
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM task \
         WHERE project_id IS NULL AND completed_at IS NULL \
         ORDER BY position"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map([], task_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Every task in the database, ordered by position. Useful for tests
/// and the debug pane; production list views use scoped queries.
pub fn list_all_tasks(conn: &Connection) -> Result<Vec<Task>, DbError> {
    let sql = format!("SELECT {TASK_COLUMNS} FROM task ORDER BY position");
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map([], task_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Default heads-up window for upcoming deadlines surfaced in Today
/// (spec §4.2). A task with a deadline within `today + N` days
/// appears in Today even before that deadline arrives, matching
/// Things 3's "deadlines approaching" behaviour. v0.14.0 (Phase
/// 18.5 Tier-1) made this a per-task overridable default — when
/// `task.deadline_warn_days` is set, the per-task value wins; when
/// NULL, the global constant applies. Turning the global default
/// itself into a GSettings key is a Phase 8d / 19.5 preferences
/// task.
pub const TODAY_DEADLINE_WINDOW_DAYS: i64 = 7;

/// Today list per spec §4.2:
///
/// > `task WHERE completed_at IS NULL`
/// > `  AND ( scheduled_for ≤ today`
/// > `        OR deadline ≤ today + COALESCE(deadline_warn_days, N) )`
/// > `  AND ( defer_until IS NULL OR defer_until ≤ today )`
///
/// The `scheduled_for != '__someday__'` clause is the implementation
/// detail that keeps the Someday sentinel out of Today: ISO date
/// strings sort lexicographically, but `__someday__` starts with
/// underscores (`0x5F`) which compare *less than* any digit, so a
/// naive `scheduled_for <= ?today` would otherwise match it.
///
/// The deadline clause is the v0.0.38 Things-3 alignment: deadlines
/// approaching surface as a heads-up so the user isn't blindsided.
/// Earlier versions used `deadline ≤ today`, which left a future-
/// deadlined task buried in Anytime until its deadline date arrived.
/// v0.14.0 (Phase 18.5 Tier-1) added the per-task warning override
/// via `COALESCE(deadline_warn_days, ?2)` so a sensitive task can
/// surface earlier than the global default.
pub fn list_today(conn: &Connection, today: NaiveDate) -> Result<Vec<Task>, DbError> {
    let today_str = today.format("%Y-%m-%d").to_string();
    // SQLite stores `deadline` as an ISO date string. The horizon
    // is computed per-row in SQL by adding `COALESCE(warn, default)`
    // days to the parameter `today`, then comparing to the deadline
    // string lexicographically — both sides use `YYYY-MM-DD`, which
    // sorts cleanly without converting to julianday.
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM task \
         WHERE completed_at IS NULL \
           AND ( \
                 (scheduled_for IS NOT NULL \
                    AND scheduled_for != '__someday__' \
                    AND scheduled_for <= ?1) \
                 OR (deadline IS NOT NULL \
                     AND deadline <= date(?1, '+' || COALESCE(deadline_warn_days, ?2) || ' days')) \
               ) \
           AND (defer_until IS NULL OR defer_until <= ?1) \
         ORDER BY position"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(
        params![today_str, TODAY_DEADLINE_WINDOW_DAYS],
        task_from_row,
    )?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Anytime per spec §4.2: open tasks without a scheduled date that
/// aren't currently deferred.
pub fn list_anytime(conn: &Connection, today: NaiveDate) -> Result<Vec<Task>, DbError> {
    let today_str = today.format("%Y-%m-%d").to_string();
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM task \
         WHERE completed_at IS NULL \
           AND scheduled_for IS NULL \
           AND (defer_until IS NULL OR defer_until <= ?1) \
         ORDER BY position"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![today_str], task_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Someday per spec §4.2: open tasks parked on the Someday sentinel.
pub fn list_someday(conn: &Connection) -> Result<Vec<Task>, DbError> {
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM task \
         WHERE completed_at IS NULL AND scheduled_for = '__someday__' \
         ORDER BY position"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map([], task_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Upcoming per spec §4.2: open tasks scheduled strictly after today
/// (Someday excluded; same lexical caveat as `list_today`).
pub fn list_upcoming(conn: &Connection, today: NaiveDate) -> Result<Vec<Task>, DbError> {
    let today_str = today.format("%Y-%m-%d").to_string();
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM task \
         WHERE completed_at IS NULL \
           AND scheduled_for IS NOT NULL \
           AND scheduled_for != '__someday__' \
           AND scheduled_for > ?1 \
         ORDER BY scheduled_for, position"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![today_str], task_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Forecast window for Phase 12: every open, non-deferred-future
/// task touching the `[today, today + days]` date span. A task
/// "touches" the window if its `scheduled_for`, `deadline`, or
/// `defer_until` lands on a date in the range. The Someday
/// sentinel is excluded.
///
/// Tasks that match more than one column (e.g., scheduled today
/// AND deadlined later in the window) are returned once — the UI
/// renders them under each date they touch separately.
pub fn list_forecast(conn: &Connection, today: NaiveDate, days: i64) -> Result<Vec<Task>, DbError> {
    let today_str = today.format("%Y-%m-%d").to_string();
    let horizon_str = (today + chrono::Duration::days(days))
        .format("%Y-%m-%d")
        .to_string();
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM task \
         WHERE completed_at IS NULL \
           AND ( \
                 (scheduled_for IS NOT NULL \
                    AND scheduled_for != '__someday__' \
                    AND scheduled_for >= ?1 \
                    AND scheduled_for <= ?2) \
                 OR (deadline IS NOT NULL \
                       AND deadline >= ?1 \
                       AND deadline <= ?2) \
                 OR (defer_until IS NOT NULL \
                       AND defer_until >= ?1 \
                       AND defer_until <= ?2) \
               ) \
         ORDER BY position"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![today_str, horizon_str], task_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Overdue tasks for the Forecast Overdue header: every open task
/// whose scheduled_for OR deadline is strictly before `today` AND
/// which isn't currently deferred to a future date. Someday is
/// excluded (it's a state, not a past date despite the
/// lexicographic-low sentinel).
pub fn list_overdue(conn: &Connection, today: NaiveDate) -> Result<Vec<Task>, DbError> {
    let today_str = today.format("%Y-%m-%d").to_string();
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM task \
         WHERE completed_at IS NULL \
           AND ( \
                 (scheduled_for IS NOT NULL \
                    AND scheduled_for != '__someday__' \
                    AND scheduled_for < ?1) \
                 OR (deadline IS NOT NULL AND deadline < ?1) \
               ) \
           AND (defer_until IS NULL OR defer_until <= ?1) \
         ORDER BY \
             COALESCE(deadline, scheduled_for, '9999-12-31') ASC, \
             position"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![today_str], task_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Logbook per spec §4.2: completed tasks, newest-first.
pub fn list_logbook(conn: &Connection) -> Result<Vec<Task>, DbError> {
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM task \
         WHERE completed_at IS NOT NULL \
         ORDER BY completed_at DESC"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map([], task_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// All open tasks belonging to `project_id`, ordered by position.
pub fn list_project(conn: &Connection, project_id: i64) -> Result<Vec<Task>, DbError> {
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM task \
         WHERE project_id = ?1 AND completed_at IS NULL \
         ORDER BY position"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![project_id], task_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Subtasks (Phase 19.5) — direct children of `parent_id`, ordered by
/// position. Includes completed children (the Inspector Subtasks group
/// renders them struck-through); callers filter to open-only if needed.
pub fn list_subtasks(conn: &Connection, parent_id: i64) -> Result<Vec<Task>, DbError> {
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM task \
         WHERE parent_id = ?1 \
         ORDER BY position"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![parent_id], task_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// all tasks belonging to `project_id` regardless of
/// completion state, ordered by position. Used by the Org vault
/// writer (sync::org::write) so the projected `.org` file
/// reflects the complete project state — DONE tasks land in the
/// file with a CLOSED cookie, not silently dropped.
pub fn list_all_in_project(conn: &Connection, project_id: i64) -> Result<Vec<Task>, DbError> {
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM task \
         WHERE project_id = ?1 \
         ORDER BY position"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![project_id], task_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// All open tasks across the area's projects, ordered by project
/// position then task position. Aggregate "click an area in the
/// sidebar" view per spec §5.1.
pub fn list_area(conn: &Connection, area_id: i64) -> Result<Vec<Task>, DbError> {
    let sql = format!(
        "SELECT {} FROM task t \
         JOIN project p ON t.project_id = p.id \
         WHERE p.area_id = ?1 AND t.completed_at IS NULL \
         ORDER BY p.position, t.position",
        &*TASK_COLUMNS_T
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![area_id], task_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

const AREA_COLUMNS: &str =
    "id, uuid, title, color, default_review_interval_days, position, created_at, modified_at";

const PROJECT_COLUMNS: &str = "id, uuid, title, note, area_id, sequential, \
    review_interval_days, last_reviewed_at, archived_at, position, \
    created_at, modified_at";

/// All areas, ordered by position. Sidebar load.
pub fn list_areas(conn: &Connection) -> Result<Vec<Area>, DbError> {
    let sql = format!("SELECT {AREA_COLUMNS} FROM area ORDER BY position");
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map([], area_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Single area by id.
pub fn area_by_id(conn: &Connection, id: i64) -> Result<Option<Area>, DbError> {
    let sql = format!("SELECT {AREA_COLUMNS} FROM area WHERE id = ?1");
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query_map(params![id], area_from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// Single project by id (regardless of archived state).
pub fn project_by_id(conn: &Connection, id: i64) -> Result<Option<Project>, DbError> {
    let sql = format!("SELECT {PROJECT_COLUMNS} FROM project WHERE id = ?1");
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query_map(params![id], project_from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// All non-archived projects, ordered by area then position. Sidebar
/// load consumes this and groups by `area_id` (None = unfiled).
pub fn list_projects(conn: &Connection) -> Result<Vec<Project>, DbError> {
    let sql = format!(
        "SELECT {PROJECT_COLUMNS} FROM project \
         WHERE archived_at IS NULL \
         ORDER BY area_id, position"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map([], project_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// every project including archived. Used by the JSON
/// snapshot exporter so a backup includes the full project
/// history, not just the active set.
pub fn list_all_projects(conn: &Connection) -> Result<Vec<Project>, DbError> {
    let sql = format!("SELECT {PROJECT_COLUMNS} FROM project ORDER BY area_id, position");
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map([], project_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// every heading row across all projects, ordered by
/// project then position. Used by the JSON snapshot exporter.
pub fn list_headings(conn: &Connection) -> Result<Vec<crate::domain::Heading>, DbError> {
    let sql = "SELECT id, uuid, project_id, title, position, created_at, modified_at \
               FROM heading ORDER BY project_id, position";
    let mut stmt = conn.prepare_cached(sql)?;
    let rows = stmt.query_map([], heading_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Headings belonging to `project_id`, in position order. Used by
/// the Org writer's heading-emit path so sections interleave with
/// tasks correctly.
pub fn list_headings_in_project(
    conn: &Connection,
    project_id: i64,
) -> Result<Vec<crate::domain::Heading>, DbError> {
    let sql = "SELECT id, uuid, project_id, title, position, created_at, modified_at \
               FROM heading WHERE project_id = ?1 ORDER BY position";
    let mut stmt = conn.prepare_cached(sql)?;
    let rows = stmt.query_map(params![project_id], heading_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Fetch a single heading by primary key.
pub fn heading_by_id(
    conn: &Connection,
    id: i64,
) -> Result<Option<crate::domain::Heading>, DbError> {
    let sql = "SELECT id, uuid, project_id, title, position, created_at, modified_at \
               FROM heading WHERE id = ?1";
    let mut stmt = conn.prepare_cached(sql)?;
    let mut rows = stmt.query_map(params![id], heading_from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

fn heading_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<crate::domain::Heading> {
    Ok(crate::domain::Heading {
        id: row.get("id")?,
        uuid: row.get("uuid")?,
        project_id: row.get("project_id")?,
        title: row.get("title")?,
        position: row.get("position")?,
        created_at: row.get("created_at")?,
        modified_at: row.get("modified_at")?,
    })
}

/// every task_tag relation as `(task_id, tag_id)` pairs.
/// Used by the JSON snapshot exporter so tag membership is
/// preserved alongside the task + tag tables.
pub fn list_task_tags(conn: &Connection) -> Result<Vec<(i64, i64)>, DbError> {
    let sql = "SELECT task_id, tag_id FROM task_tag ORDER BY task_id, tag_id";
    let mut stmt = conn.prepare_cached(sql)?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Phase 13 — projects due for review. A project surfaces in the
/// queue when:
///
/// - it has a non-NULL `review_interval_days` (the user opted in),
/// - it isn't archived,
/// - either it has never been reviewed (`last_reviewed_at IS NULL`)
///   or its last review plus the interval is on or before `today`.
///
/// Order: never-reviewed projects first (highest priority — they've
/// been waiting since creation), then by oldest `last_reviewed_at`.
/// Tie-break by `position` so the user's manual ordering shows
/// through.
///
/// SQLite's `date(timestamp, '+N days')` does the math directly;
/// we concat `review_interval_days` into the modifier string at the
/// SQL level to avoid pulling each row into Rust just to filter.
pub fn list_review_queue(conn: &Connection, today: NaiveDate) -> Result<Vec<Project>, DbError> {
    let today_str = today.format("%Y-%m-%d").to_string();
    // Prefix the projected columns with the project alias — the
    // LEFT JOIN onto area shares column names (id, title, position,
    // created_at, modified_at), so bare names would be ambiguous.
    let project_cols = PROJECT_COLUMNS
        .split(", ")
        .map(|c| format!("p.{c}"))
        .collect::<Vec<_>>()
        .join(", ");
    // v0.28.0 — the effective cadence is the project's own
    // review_interval_days, falling back to its area's
    // default_review_interval_days. Both NULL keeps the project out
    // of the queue, exactly as before the area default existed.
    let sql = format!(
        "SELECT {project_cols} FROM project p \
         LEFT JOIN area a ON p.area_id = a.id \
         WHERE COALESCE(p.review_interval_days, a.default_review_interval_days) IS NOT NULL \
           AND p.archived_at IS NULL \
           AND ( \
                 p.last_reviewed_at IS NULL \
                 OR date( \
                      p.last_reviewed_at, \
                      '+' || COALESCE(p.review_interval_days, a.default_review_interval_days) || ' days' \
                    ) <= ?1 \
               ) \
         ORDER BY \
             CASE WHEN p.last_reviewed_at IS NULL THEN 0 ELSE 1 END, \
             p.last_reviewed_at ASC, \
             p.position"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![today_str], project_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

// ── Task dependencies (v0.29.0) ─────────────────────────────────

/// The set of task ids that are currently blocked: an open task with
/// at least one open prerequisite (a `blocked_by_id` task that isn't
/// completed). Feeds both the GUI "Blocked" pill and the search
/// engine's `is:blocked` / `is:available` evaluation. A completed task
/// is never blocked, and a prerequisite that's already done doesn't
/// count — both ends must be open for the edge to gate availability.
pub fn blocked_task_ids(conn: &Connection) -> Result<HashSet<i64>, DbError> {
    let mut stmt = conn.prepare_cached(
        "SELECT DISTINCT d.task_id FROM task_dependency d \
         JOIN task b ON d.blocked_by_id = b.id \
         JOIN task t ON d.task_id = t.id \
         WHERE t.completed_at IS NULL AND b.completed_at IS NULL",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
    rows.collect::<rusqlite::Result<HashSet<i64>>>()
        .map_err(Into::into)
}

/// The prerequisite tasks blocking `task_id` (its `blocked_by_id`
/// edges), ordered by position. Feeds the Builder Inspector's
/// "Blocked by" group and the CLI `info` view. Includes completed
/// prerequisites so the caller can show the full picture; filter on
/// `completed_at` if only open blockers matter.
pub fn list_prerequisites(conn: &Connection, task_id: i64) -> Result<Vec<Task>, DbError> {
    let cols = TASK_COLUMNS_T.as_str();
    let sql = format!(
        "SELECT {cols} FROM task t \
         JOIN task_dependency d ON d.blocked_by_id = t.id \
         WHERE d.task_id = ?1 \
         ORDER BY t.position"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![task_id], task_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

const TAG_COLUMNS: &str = "id, uuid, name, color, created_at, modified_at";

/// Single tag by id.
pub fn tag_by_id(conn: &Connection, id: i64) -> Result<Option<Tag>, DbError> {
    let sql = format!("SELECT {TAG_COLUMNS} FROM tag WHERE id = ?1");
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query_map(params![id], tag_from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// All tags, ordered by name (case-insensitive — the column itself
/// uses `COLLATE NOCASE`).
pub fn list_tags(conn: &Connection) -> Result<Vec<Tag>, DbError> {
    let sql = format!("SELECT {TAG_COLUMNS} FROM tag ORDER BY name");
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map([], tag_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

// ── Perspectives (Phase 14) ─────────────────────────────────────

const PERSPECTIVE_COLUMNS: &str = "id, uuid, name, icon, filter_expr, sort_order, grouping, \
    renderer, renderer_config, position, created_at, modified_at";

/// Single perspective by id.
pub fn perspective_by_id(conn: &Connection, id: i64) -> Result<Option<Perspective>, DbError> {
    let sql = format!("SELECT {PERSPECTIVE_COLUMNS} FROM perspective WHERE id = ?1");
    let mut stmt = conn.prepare_cached(&sql)?;
    let mut rows = stmt.query_map(params![id], perspective_from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

/// All perspectives, ordered by user-managed `position`. The
/// sidebar consumes this and renders one row per perspective
/// under the "Perspectives" section header (Builder mode).
pub fn list_perspectives(conn: &Connection) -> Result<Vec<Perspective>, DbError> {
    let sql = format!("SELECT {PERSPECTIVE_COLUMNS} FROM perspective ORDER BY position, name");
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map([], perspective_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Open tasks bearing the given tag, ordered by position. The tag
/// page view (`ActiveList::Tag(id)`) calls this.
pub fn list_tasks_with_tag(conn: &Connection, tag_id: i64) -> Result<Vec<Task>, DbError> {
    let sql = format!(
        "SELECT {} FROM task t \
         JOIN task_tag tt ON tt.task_id = t.id \
         WHERE tt.tag_id = ?1 AND t.completed_at IS NULL \
         ORDER BY t.position",
        &*TASK_COLUMNS_T
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![tag_id], task_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// All tag ids attached to a task (Phase 6b uses this for the pill
/// editor's initial state).
pub fn tag_ids_for_task(conn: &Connection, task_id: i64) -> Result<Vec<i64>, DbError> {
    let mut stmt = conn.prepare_cached("SELECT tag_id FROM task_tag WHERE task_id = ?1")?;
    let rows = stmt.query_map(params![task_id], |r| r.get::<_, i64>(0))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Map of `task_id` → tag names (sorted alphabetically) for every
/// task that has any tag. Phase 6b uses this to build the per-row
/// pill display in one batched query rather than N+1.
pub fn tag_names_per_task(conn: &Connection) -> Result<HashMap<i64, Vec<String>>, DbError> {
    let mut stmt = conn.prepare_cached(
        "SELECT tt.task_id, tag.name FROM task_tag tt \
         JOIN tag ON tag.id = tt.tag_id \
         ORDER BY tt.task_id, tag.name",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut out: HashMap<i64, Vec<String>> = HashMap::new();
    for row in rows {
        let (task_id, name) = row?;
        out.entry(task_id).or_default().push(name);
    }
    Ok(out)
}

/// v0.38.2 — project-scoped variant of [`tag_names_per_task`]. Joins
/// through `task.project_id` and filters to one project so a caller
/// that only diffs a single project (the vault watcher, on every
/// `.org` save) doesn't scan the whole `task_tag` table per event.
pub fn tag_names_for_project(
    conn: &Connection,
    project_id: i64,
) -> Result<HashMap<i64, Vec<String>>, DbError> {
    let mut stmt = conn.prepare_cached(
        "SELECT tt.task_id, tag.name FROM task_tag tt \
         JOIN tag ON tag.id = tt.tag_id \
         JOIN task ON task.id = tt.task_id \
         WHERE task.project_id = ?1 \
         ORDER BY tt.task_id, tag.name",
    )?;
    let rows = stmt.query_map([project_id], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut out: HashMap<i64, Vec<String>> = HashMap::new();
    for row in rows {
        let (task_id, name) = row?;
        out.entry(task_id).or_default().push(name);
    }
    Ok(out)
}

/// Per-task list of `(tag_name, optional_hex_color)` pairs. Returned
/// by [`tag_info_per_task`] as the renderer-side companion to
/// [`tag_names_per_task`]; aliased to keep the public signature
/// readable.
pub type TagInfoMap = HashMap<i64, Vec<(String, Option<String>)>>;

/// v0.3.0 — same join as `tag_names_per_task`, but also returns each
/// tag's `color` so the row factory can render coloured Pango spans
/// per pill. Single batched query; the renderer keeps `tag_names_per_task`
/// for paths that only need names (the Phase 7d filter evaluator).
pub fn tag_info_per_task(conn: &Connection) -> Result<TagInfoMap, DbError> {
    let mut stmt = conn.prepare_cached(
        "SELECT tt.task_id, tag.name, tag.color FROM task_tag tt \
         JOIN tag ON tag.id = tt.tag_id \
         ORDER BY tt.task_id, tag.name",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut out: HashMap<i64, Vec<(String, Option<String>)>> = HashMap::new();
    for row in rows {
        let (task_id, name, color) = row?;
        out.entry(task_id).or_default().push((name, color));
    }
    Ok(out)
}

fn tag_from_row(row: &Row<'_>) -> rusqlite::Result<Tag> {
    Ok(Tag {
        id: row.get("id")?,
        uuid: row.get("uuid")?,
        name: row.get("name")?,
        color: row.get("color")?,
        created_at: row.get("created_at")?,
        modified_at: row.get("modified_at")?,
    })
}

fn area_from_row(row: &Row<'_>) -> rusqlite::Result<Area> {
    Ok(Area {
        id: row.get("id")?,
        uuid: row.get("uuid")?,
        title: row.get("title")?,
        color: row.get("color")?,
        default_review_interval_days: row.get("default_review_interval_days")?,
        position: row.get("position")?,
        created_at: row.get("created_at")?,
        modified_at: row.get("modified_at")?,
    })
}

fn perspective_from_row(row: &Row<'_>) -> rusqlite::Result<Perspective> {
    Ok(Perspective {
        id: row.get("id")?,
        uuid: row.get("uuid")?,
        name: row.get("name")?,
        icon: row.get("icon")?,
        filter_expr: row.get("filter_expr")?,
        sort_order: row.get("sort_order")?,
        grouping: row.get("grouping")?,
        renderer: row.get("renderer")?,
        renderer_config: row.get("renderer_config")?,
        position: row.get("position")?,
        created_at: row.get("created_at")?,
        modified_at: row.get("modified_at")?,
    })
}

fn project_from_row(row: &Row<'_>) -> rusqlite::Result<Project> {
    let sequential: i64 = row.get("sequential")?;
    Ok(Project {
        id: row.get("id")?,
        uuid: row.get("uuid")?,
        title: row.get("title")?,
        note: row.get("note")?,
        area_id: row.get("area_id")?,
        sequential: sequential != 0,
        review_interval_days: row.get("review_interval_days")?,
        last_reviewed_at: row.get("last_reviewed_at")?,
        archived_at: row.get("archived_at")?,
        position: row.get("position")?,
        created_at: row.get("created_at")?,
        modified_at: row.get("modified_at")?,
    })
}

pub(super) fn task_from_row(row: &Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get("id")?,
        uuid: row.get("uuid")?,
        title: row.get("title")?,
        note: row.get("note")?,
        project_id: row.get("project_id")?,
        parent_id: row.get("parent_id")?,
        scheduled_for: row.get::<_, Option<ScheduledFor>>("scheduled_for")?,
        deadline: row.get("deadline")?,
        defer_until: row.get("defer_until")?,
        estimated_minutes: row.get("estimated_minutes")?,
        completed_at: row.get("completed_at")?,
        repeat_rule: row.get("repeat_rule")?,
        repeat_mode: row.get("repeat_mode")?,
        last_reviewed_at: row.get("last_reviewed_at")?,
        orig_keyword: row.get("orig_keyword")?,
        deadline_warn_days: row.get("deadline_warn_days")?,
        scheduled_time: row
            .get::<_, Option<String>>("scheduled_time")?
            .and_then(|s| chrono::NaiveTime::parse_from_str(&s, "%H:%M").ok()),
        reminder_at: row.get("reminder_at")?,
        extra_properties: row
            .get::<_, Option<String>>("extra_properties")?
            .as_deref()
            .map(|s| {
                serde_json::from_str(s).unwrap_or_else(|e| {
                    // A corrupt blob here would silently drop every
                    // unmodeled :PROPERTIES: value — the exact Org
                    // round-trip data the v0.24.0 column exists to
                    // preserve. Surface it instead of vanishing it.
                    tracing::warn!(
                        error = %e,
                        "task.extra_properties is not valid JSON; dropping it for this read"
                    );
                    Default::default()
                })
            })
            .unwrap_or_default(),
        position: row.get("position")?,
        created_at: row.get("created_at")?,
        modified_at: row.get("modified_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use rusqlite::Connection;

    fn fresh_conn() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        db::configure_pragmas(&conn).unwrap();
        crate::db::migrations::migrate(&mut conn).unwrap();
        conn
    }

    fn insert_task(
        conn: &Connection,
        uuid: &str,
        title: &str,
        scheduled: Option<&str>,
        deadline: Option<&str>,
        defer: Option<&str>,
        completed: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO task \
             (uuid, title, scheduled_for, deadline, defer_until, completed_at, position) \
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![uuid, title, scheduled, deadline, defer, completed, 1.0],
        )
        .unwrap();
    }

    fn insert_task_with_warn(
        conn: &Connection,
        uuid: &str,
        title: &str,
        deadline: &str,
        warn: Option<i64>,
    ) {
        conn.execute(
            "INSERT INTO task \
             (uuid, title, deadline, deadline_warn_days, position) \
             VALUES (?, ?, ?, ?, ?)",
            params![uuid, title, deadline, warn, 1.0],
        )
        .unwrap();
    }

    fn insert_task_with_reminder(
        conn: &Connection,
        uuid: &str,
        title: &str,
        reminder_at: Option<&str>,
        completed_at: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO task (uuid, title, reminder_at, completed_at, position) \
             VALUES (?, ?, ?, ?, ?)",
            params![uuid, title, reminder_at, completed_at, 1.0],
        )
        .unwrap();
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 5, 15).unwrap()
    }

    #[test]
    fn today_includes_scheduled_for_today() {
        let conn = fresh_conn();
        insert_task(
            &conn,
            "a",
            "due today",
            Some("2026-05-15"),
            None,
            None,
            None,
        );
        let rows = list_today(&conn, today()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "due today");
    }

    #[test]
    fn today_includes_overdue() {
        let conn = fresh_conn();
        insert_task(&conn, "a", "overdue", Some("2026-05-10"), None, None, None);
        let rows = list_today(&conn, today()).unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn today_excludes_future_scheduled() {
        let conn = fresh_conn();
        insert_task(&conn, "a", "tomorrow", Some("2026-05-16"), None, None, None);
        assert!(list_today(&conn, today()).unwrap().is_empty());
    }

    #[test]
    fn today_excludes_someday_sentinel() {
        let conn = fresh_conn();
        insert_task(&conn, "a", "later", Some("__someday__"), None, None, None);
        assert!(list_today(&conn, today()).unwrap().is_empty());
    }

    #[test]
    fn today_excludes_completed() {
        let conn = fresh_conn();
        insert_task(
            &conn,
            "a",
            "done",
            Some("2026-05-10"),
            None,
            None,
            Some("2026-05-12T08:00:00.000Z"),
        );
        assert!(list_today(&conn, today()).unwrap().is_empty());
    }

    #[test]
    fn today_excludes_deferred_to_future() {
        let conn = fresh_conn();
        insert_task(
            &conn,
            "a",
            "deferred",
            Some("2026-05-10"),
            None,
            Some("2026-05-20"),
            None,
        );
        assert!(list_today(&conn, today()).unwrap().is_empty());
    }

    #[test]
    fn today_includes_deferred_now_active() {
        let conn = fresh_conn();
        insert_task(
            &conn,
            "a",
            "active",
            Some("2026-05-10"),
            None,
            Some("2026-05-15"),
            None,
        );
        let rows = list_today(&conn, today()).unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn today_includes_deadline_only_due_today() {
        let conn = fresh_conn();
        insert_task(
            &conn,
            "a",
            "deadline only",
            None,
            Some("2026-05-15"),
            None,
            None,
        );
        let rows = list_today(&conn, today()).unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn today_includes_deadline_within_heads_up_window() {
        // Spec §4.2 (v0.0.38) — a deadline N days in the future
        // surfaces in Today as a heads-up. With today = 2026-05-15
        // and the 7-day window, a deadline of 2026-05-20 (5 days
        // out) should appear.
        let conn = fresh_conn();
        insert_task(
            &conn,
            "a",
            "approaching",
            None,
            Some("2026-05-20"),
            None,
            None,
        );
        let rows = list_today(&conn, today()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "approaching");
    }

    #[test]
    fn today_includes_deadline_at_window_edge() {
        // Boundary: today + TODAY_DEADLINE_WINDOW_DAYS days exactly
        // is *included* (≤ horizon). 2026-05-15 + 7 = 2026-05-22.
        let conn = fresh_conn();
        insert_task(&conn, "a", "edge", None, Some("2026-05-22"), None, None);
        let rows = list_today(&conn, today()).unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn today_excludes_deadline_past_window() {
        // 8 days out: outside the heads-up window, lives in Anytime
        // until it crosses into the window.
        let conn = fresh_conn();
        insert_task(
            &conn,
            "a",
            "far future",
            None,
            Some("2026-05-23"),
            None,
            None,
        );
        assert!(list_today(&conn, today()).unwrap().is_empty());
    }

    // v0.14.0 — Phase 18.5 Tier-1: per-task `deadline_warn_days`
    // overrides the global TODAY_DEADLINE_WINDOW_DAYS. With today
    // = 2026-05-15, a deadline 14 days out (2026-05-29) only
    // surfaces in Today when the row carries an override large
    // enough to cover it.
    #[test]
    fn today_includes_deadline_when_per_task_warn_overrides_default() {
        let conn = fresh_conn();
        insert_task_with_warn(&conn, "with-warn", "sensitive", "2026-05-29", Some(14));
        insert_task_with_warn(&conn, "no-warn", "default", "2026-05-29", None);
        let rows = list_today(&conn, today()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "sensitive");
    }

    #[test]
    fn today_excludes_deadline_when_per_task_warn_is_below_offset() {
        // A deadline 5 days out with warn=2 is *below* the global
        // default's reach, but the override wins — the per-task
        // value is the contract, not a maximum.
        let conn = fresh_conn();
        insert_task_with_warn(&conn, "tight", "tight window", "2026-05-20", Some(2));
        assert!(list_today(&conn, today()).unwrap().is_empty());
    }

    #[test]
    fn today_per_task_warn_zero_surfaces_only_on_or_past_deadline() {
        // warn=0 means "no early surfacing." The deadline itself
        // stays in scope (`deadline ≤ today + 0` ⇒ deadline ≤ today),
        // but a future deadline doesn't.
        let conn = fresh_conn();
        insert_task_with_warn(&conn, "future", "tomorrow", "2026-05-16", Some(0));
        insert_task_with_warn(&conn, "now", "today", "2026-05-15", Some(0));
        insert_task_with_warn(&conn, "past", "yesterday", "2026-05-14", Some(0));
        let rows = list_today(&conn, today()).unwrap();
        let titles: Vec<&str> = rows.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(titles, vec!["today", "yesterday"]);
    }

    #[test]
    fn today_count_matches_list_today_with_per_task_warn() {
        // Sidebar badge + list contents must agree under the
        // per-row horizon (v0.14.0 mirrored the COALESCE into
        // count_open_canonical alongside list_today).
        let conn = fresh_conn();
        insert_task_with_warn(&conn, "with-warn", "sensitive", "2026-05-29", Some(14));
        insert_task_with_warn(&conn, "no-warn", "default", "2026-05-29", None);
        let counts = count_open_canonical(&conn, today()).unwrap();
        let listed = list_today(&conn, today()).unwrap().len() as i64;
        assert_eq!(counts.today, listed);
        assert_eq!(counts.today, 1);
    }

    // v0.20.0 — Phase 19.5 next_pending_reminder ordering.
    #[test]
    fn next_pending_reminder_returns_soonest_open_task() {
        let conn = fresh_conn();
        // Reminder timestamps: A two hours from "now", B 30
        // mins, C one hour but completed, D in the past.
        // Stored in rusqlite's DateTime form (`+00:00`), matching what
        // the worker writes — so the boundary comparison is exercised
        // against the real on-disk format, not a hand-built `Z` shape.
        insert_task_with_reminder(&conn, "a", "A", Some("2030-01-01T12:00:00+00:00"), None);
        insert_task_with_reminder(&conn, "b", "B", Some("2030-01-01T10:30:00+00:00"), None);
        insert_task_with_reminder(
            &conn,
            "c",
            "C",
            Some("2030-01-01T11:00:00+00:00"),
            Some("2030-01-01T09:00:00+00:00"),
        );
        insert_task_with_reminder(&conn, "d", "D", Some("2020-01-01T00:00:00+00:00"), None);
        // Cutoff: 2030-01-01 10:00 — D is in the past relative
        // to it (skipped); B (10:30) is the soonest.
        let cutoff: DateTime<Utc> = "2030-01-01T10:00:00Z".parse().unwrap();
        let result = next_pending_reminder(&conn, cutoff).unwrap();
        let (task_id, when) = result.expect("expected B as next reminder");
        let title: String = conn
            .query_row(
                "SELECT title FROM task WHERE id = ?1",
                params![task_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(title, "B");
        assert_eq!(when.format("%H:%M").to_string(), "10:30");
    }

    #[test]
    fn next_pending_reminder_returns_none_when_all_past_or_completed() {
        let conn = fresh_conn();
        insert_task_with_reminder(
            &conn,
            "done",
            "Done",
            Some("2030-01-01T10:30:00+00:00"),
            Some("2030-01-01T09:00:00+00:00"),
        );
        insert_task_with_reminder(
            &conn,
            "past",
            "Past",
            Some("2020-01-01T00:00:00+00:00"),
            None,
        );
        let cutoff: DateTime<Utc> = "2030-01-01T10:00:00Z".parse().unwrap();
        assert!(next_pending_reminder(&conn, cutoff).unwrap().is_none());
    }

    #[test]
    fn malformed_extra_properties_defaults_to_empty_without_panic() {
        // A corrupt extra_properties blob must not crash the read path;
        // it degrades to an empty map (and logs a warning).
        let conn = fresh_conn();
        insert_task(&conn, "a", "t", None, None, None, None);
        conn.execute(
            "UPDATE task SET extra_properties = '{not valid json' WHERE uuid = 'a'",
            [],
        )
        .unwrap();
        let id: i64 = conn
            .query_row("SELECT id FROM task WHERE uuid = 'a'", [], |r| r.get(0))
            .unwrap();
        let task = task_by_id(&conn, id).unwrap().unwrap();
        assert!(task.extra_properties.is_empty());
    }

    #[test]
    fn tag_names_for_project_scopes_to_one_project() {
        let conn = fresh_conn();
        let p1 = insert_project(&conn, "p1", "One", None, None);
        let p2 = insert_project(&conn, "p2", "Two", None, None);
        conn.execute(
            "INSERT INTO task (uuid, title, project_id, position) VALUES ('a','A',?,1)",
            params![p1],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO task (uuid, title, project_id, position) VALUES ('c','C',?,1)",
            params![p2],
        )
        .unwrap();
        conn.execute("INSERT INTO tag (uuid, name) VALUES ('t','work')", [])
            .unwrap();
        let ta: i64 = conn
            .query_row("SELECT id FROM task WHERE uuid='a'", [], |r| r.get(0))
            .unwrap();
        let tc: i64 = conn
            .query_row("SELECT id FROM task WHERE uuid='c'", [], |r| r.get(0))
            .unwrap();
        let tag: i64 = conn
            .query_row("SELECT id FROM tag WHERE name='work'", [], |r| r.get(0))
            .unwrap();
        conn.execute(
            "INSERT INTO task_tag (task_id, tag_id) VALUES (?,?), (?,?)",
            params![ta, tag, tc, tag],
        )
        .unwrap();

        let map = tag_names_for_project(&conn, p1).unwrap();
        // Only project p1's task appears; p2's tagged task is excluded.
        assert_eq!(map.len(), 1);
        assert_eq!(map.get(&ta).unwrap(), &vec!["work".to_string()]);
        assert!(!map.contains_key(&tc));
    }

    #[test]
    fn today_count_matches_list_today_with_window() {
        // The sidebar badge query (`count_open_canonical.today`) and
        // the Today list query must agree about who's in Today —
        // they share the same predicate, so a bug in one shows up
        // here as a count mismatch.
        let conn = fresh_conn();
        insert_task(
            &conn,
            "a",
            "scheduled today",
            Some("2026-05-15"),
            None,
            None,
            None,
        );
        insert_task(&conn, "b", "overdue", Some("2026-05-10"), None, None, None);
        insert_task(
            &conn,
            "c",
            "deadline today",
            None,
            Some("2026-05-15"),
            None,
            None,
        );
        insert_task(
            &conn,
            "d",
            "deadline 5d",
            None,
            Some("2026-05-20"),
            None,
            None,
        );
        insert_task(
            &conn,
            "e",
            "deadline 10d",
            None,
            Some("2026-05-25"),
            None,
            None,
        );
        insert_task(
            &conn,
            "f",
            "future scheduled",
            Some("2026-05-25"),
            None,
            None,
            None,
        );

        let rows = list_today(&conn, today()).unwrap();
        let counts = count_open_canonical(&conn, today()).unwrap();
        assert_eq!(rows.len() as i64, counts.today);
        assert_eq!(rows.len(), 4); // a, b, c, d — not e (10d out) or f (future scheduled)
    }

    // ── Anytime ────────────────────────────────────────────────────

    #[test]
    fn anytime_returns_unscheduled_open_tasks() {
        let conn = fresh_conn();
        insert_task(&conn, "a", "anytime", None, None, None, None);
        insert_task(
            &conn,
            "b",
            "scheduled",
            Some("2026-06-01"),
            None,
            None,
            None,
        );
        let rows = list_anytime(&conn, today()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].uuid, "a");
    }

    #[test]
    fn anytime_excludes_someday_and_completed() {
        let conn = fresh_conn();
        insert_task(&conn, "a", "someday", Some("__someday__"), None, None, None);
        insert_task(
            &conn,
            "b",
            "done",
            None,
            None,
            None,
            Some("2026-04-01T00:00:00.000Z"),
        );
        assert!(list_anytime(&conn, today()).unwrap().is_empty());
    }

    #[test]
    fn anytime_excludes_future_deferred() {
        let conn = fresh_conn();
        insert_task(&conn, "a", "deferred", None, None, Some("2026-06-01"), None);
        assert!(list_anytime(&conn, today()).unwrap().is_empty());
    }

    // ── Forecast (Phase 12) ───────────────────────────────────────

    #[test]
    fn forecast_picks_up_scheduled_and_deadline_in_window() {
        // today = 2026-05-15. Window = 30 days → through 2026-06-14.
        let conn = fresh_conn();
        // Scheduled in window.
        insert_task(
            &conn,
            "a",
            "sched-soon",
            Some("2026-05-20"),
            None,
            None,
            None,
        );
        // Deadline in window.
        insert_task(
            &conn,
            "b",
            "due-mid-window",
            None,
            Some("2026-06-01"),
            None,
            None,
        );
        // Defer expires in window.
        insert_task(
            &conn,
            "c",
            "defer-ends",
            None,
            None,
            Some("2026-05-25"),
            None,
        );
        // Scheduled past window.
        insert_task(
            &conn,
            "d",
            "sched-far",
            Some("2026-09-01"),
            None,
            None,
            None,
        );
        // Completed — must not appear.
        insert_task(
            &conn,
            "e",
            "done",
            Some("2026-05-20"),
            None,
            None,
            Some("2026-05-15T08:00:00.000Z"),
        );
        // Someday sentinel.
        insert_task(&conn, "f", "later", Some("__someday__"), None, None, None);

        let rows = list_forecast(&conn, today(), 30).unwrap();
        let uuids: Vec<&str> = rows.iter().map(|t| t.uuid.as_str()).collect();
        assert_eq!(uuids.len(), 3);
        assert!(uuids.contains(&"a"));
        assert!(uuids.contains(&"b"));
        assert!(uuids.contains(&"c"));
        assert!(!uuids.contains(&"d"));
        assert!(!uuids.contains(&"e"));
        assert!(!uuids.contains(&"f"));
    }

    #[test]
    fn forecast_excludes_overdue() {
        // Overdue tasks (scheduled or deadline before today) belong
        // in the Overdue header, not in the day-grouped window.
        let conn = fresh_conn();
        insert_task(&conn, "a", "old", Some("2026-05-01"), None, None, None);
        let rows = list_forecast(&conn, today(), 30).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn overdue_picks_up_late_scheduled_and_deadline() {
        let conn = fresh_conn();
        // Overdue scheduled.
        insert_task(
            &conn,
            "a",
            "old-sched",
            Some("2026-05-01"),
            None,
            None,
            None,
        );
        // Overdue deadline.
        insert_task(&conn, "b", "old-due", None, Some("2026-05-10"), None, None);
        // Today — *not* overdue.
        insert_task(
            &conn,
            "c",
            "due-today",
            None,
            Some("2026-05-15"),
            None,
            None,
        );
        // Future — *not* overdue.
        insert_task(&conn, "d", "future", Some("2026-06-01"), None, None, None);
        // Overdue scheduled but completed.
        insert_task(
            &conn,
            "e",
            "old-done",
            Some("2026-05-01"),
            None,
            None,
            Some("2026-05-05T08:00:00.000Z"),
        );

        let rows = list_overdue(&conn, today()).unwrap();
        let uuids: Vec<&str> = rows.iter().map(|t| t.uuid.as_str()).collect();
        assert_eq!(uuids.len(), 2);
        assert!(uuids.contains(&"a"));
        assert!(uuids.contains(&"b"));
    }

    #[test]
    fn overdue_excludes_deferred_to_future() {
        // An overdue scheduled task with a future defer_until is
        // not actionable yet — keep it out of Overdue.
        let conn = fresh_conn();
        insert_task(
            &conn,
            "a",
            "deferred",
            Some("2026-05-01"),
            None,
            Some("2026-06-01"),
            None,
        );
        assert!(list_overdue(&conn, today()).unwrap().is_empty());
    }

    // ── Someday ────────────────────────────────────────────────────

    #[test]
    fn someday_returns_only_sentinel_open_tasks() {
        let conn = fresh_conn();
        insert_task(&conn, "a", "later", Some("__someday__"), None, None, None);
        insert_task(&conn, "b", "anytime", None, None, None, None);
        insert_task(&conn, "c", "future", Some("2026-12-01"), None, None, None);
        let rows = list_someday(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].uuid, "a");
    }

    // ── Upcoming ───────────────────────────────────────────────────

    #[test]
    fn upcoming_returns_future_scheduled_only() {
        let conn = fresh_conn();
        insert_task(&conn, "a", "today", Some("2026-05-15"), None, None, None);
        insert_task(&conn, "b", "tomorrow", Some("2026-05-16"), None, None, None);
        insert_task(&conn, "c", "someday", Some("__someday__"), None, None, None);
        let rows = list_upcoming(&conn, today()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].uuid, "b");
    }

    // ── Logbook ────────────────────────────────────────────────────

    #[test]
    fn logbook_returns_completed_newest_first() {
        let conn = fresh_conn();
        insert_task(
            &conn,
            "a",
            "older",
            None,
            None,
            None,
            Some("2026-04-01T08:00:00.000Z"),
        );
        insert_task(
            &conn,
            "b",
            "newer",
            None,
            None,
            None,
            Some("2026-04-15T08:00:00.000Z"),
        );
        insert_task(&conn, "c", "open", None, None, None, None);
        let rows = list_logbook(&conn).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].uuid, "b", "newest first");
        assert_eq!(rows[1].uuid, "a");
    }

    // ── Project / Area ─────────────────────────────────────────────

    fn insert_project(
        conn: &Connection,
        uuid: &str,
        title: &str,
        area_id: Option<i64>,
        archived_at: Option<&str>,
    ) -> i64 {
        conn.execute(
            "INSERT INTO project (uuid, title, area_id, archived_at, position) \
             VALUES (?, ?, ?, ?, ?)",
            params![uuid, title, area_id, archived_at, 1.0],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn insert_area(conn: &Connection, uuid: &str, title: &str) -> i64 {
        conn.execute(
            "INSERT INTO area (uuid, title, position) VALUES (?, ?, ?)",
            params![uuid, title, 1.0],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn list_project_returns_open_tasks_for_project() {
        let conn = fresh_conn();
        let p = insert_project(&conn, "p1", "Q3", None, None);
        let q = insert_project(&conn, "p2", "Q4", None, None);
        conn.execute(
            "INSERT INTO task (uuid, title, project_id, position) VALUES \
             (?, 'p1-task', ?, 1.0), \
             (?, 'p2-task', ?, 1.0), \
             ('done', 'completed in p1', ?, 1.0)",
            params!["t1", p, "t2", q, p],
        )
        .unwrap();
        // Mark the third one done.
        conn.execute(
            "UPDATE task SET completed_at = '2026-04-01T00:00:00.000Z' WHERE uuid = 'done'",
            [],
        )
        .unwrap();

        let rows = list_project(&conn, p).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "p1-task");
    }

    #[test]
    fn list_subtasks_returns_children_ordered_by_position() {
        let conn = fresh_conn();
        conn.execute(
            "INSERT INTO task (uuid, title, position) VALUES ('parent', 'Parent', 1.0)",
            [],
        )
        .unwrap();
        let parent_id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO task (uuid, title, parent_id, position) VALUES \
             ('c2', 'second', ?1, 2.0), \
             ('c1', 'first', ?1, 1.0)",
            params![parent_id],
        )
        .unwrap();
        // A top-level task that must not appear among the children.
        conn.execute(
            "INSERT INTO task (uuid, title, position) VALUES ('top', 'top-level', 9.0)",
            [],
        )
        .unwrap();

        let kids = list_subtasks(&conn, parent_id).unwrap();
        assert_eq!(kids.len(), 2);
        assert_eq!(kids[0].title, "first");
        assert_eq!(kids[1].title, "second");
        assert!(kids.iter().all(|k| k.parent_id == Some(parent_id)));
    }

    #[test]
    fn list_area_aggregates_across_projects() {
        let conn = fresh_conn();
        let area = insert_area(&conn, "a1", "Personal");
        let p1 = insert_project(&conn, "p1", "Errands", Some(area), None);
        let p2 = insert_project(&conn, "p2", "Reading", Some(area), None);
        let other = insert_area(&conn, "a2", "Work");
        let p3 = insert_project(&conn, "p3", "Q3", Some(other), None);

        conn.execute(
            "INSERT INTO task (uuid, title, project_id, position) VALUES \
             ('t1', 'errand', ?, 1.0), \
             ('t2', 'reading', ?, 1.0), \
             ('t3', 'work', ?, 1.0)",
            params![p1, p2, p3],
        )
        .unwrap();

        let rows = list_area(&conn, area).unwrap();
        assert_eq!(rows.len(), 2);
        let titles: Vec<&str> = rows.iter().map(|t| t.title.as_str()).collect();
        assert!(titles.contains(&"errand"));
        assert!(titles.contains(&"reading"));
        assert!(!titles.contains(&"work"));
    }

    #[test]
    fn list_areas_returns_areas() {
        let conn = fresh_conn();
        insert_area(&conn, "a1", "Work");
        insert_area(&conn, "a2", "Personal");
        let areas = list_areas(&conn).unwrap();
        assert_eq!(areas.len(), 2);
    }

    // Phase 13 — review queue.

    /// Insert a project with explicit review_interval_days /
    /// last_reviewed_at so we can exercise the queue's filter.
    fn insert_project_with_review(
        conn: &Connection,
        uuid: &str,
        title: &str,
        review_interval_days: Option<i64>,
        last_reviewed_at: Option<&str>,
    ) -> i64 {
        conn.execute(
            "INSERT INTO project (uuid, title, review_interval_days, last_reviewed_at, position) \
             VALUES (?, ?, ?, ?, ?)",
            params![uuid, title, review_interval_days, last_reviewed_at, 1.0],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn review_queue_includes_never_reviewed_with_interval() {
        let conn = fresh_conn();
        // Set up — project A has interval but no review yet; B
        // has no interval (opted out); C is archived.
        let _a = insert_project_with_review(&conn, "a", "needs review", Some(7), None);
        let _b = insert_project_with_review(&conn, "b", "no interval", None, None);
        let _c = insert_project_with_review(&conn, "c", "archived", Some(7), None);
        conn.execute(
            "UPDATE project SET archived_at = '2026-04-01T00:00:00.000Z' WHERE uuid = 'c'",
            [],
        )
        .unwrap();

        let queue = list_review_queue(&conn, today()).unwrap();
        let titles: Vec<&str> = queue.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles, vec!["needs review"]);
    }

    #[test]
    fn review_queue_includes_overdue_projects() {
        let conn = fresh_conn();
        // today = 2026-05-15; reviewed 2026-05-01 with 7-day
        // interval = next review due 2026-05-08. Overdue.
        let _a = insert_project_with_review(
            &conn,
            "a",
            "overdue",
            Some(7),
            Some("2026-05-01T08:00:00.000Z"),
        );
        // Reviewed 2026-05-10 with 7-day interval = next review
        // due 2026-05-17. Not yet due.
        let _b = insert_project_with_review(
            &conn,
            "b",
            "fresh",
            Some(7),
            Some("2026-05-10T08:00:00.000Z"),
        );
        // Reviewed exactly today (interval 0 means review every
        // day; today + 0 = today; <=today fires).
        let _c = insert_project_with_review(
            &conn,
            "c",
            "every-day",
            Some(0),
            Some("2026-05-15T08:00:00.000Z"),
        );

        let queue = list_review_queue(&conn, today()).unwrap();
        let titles: Vec<&str> = queue.iter().map(|p| p.title.as_str()).collect();
        assert!(titles.contains(&"overdue"));
        assert!(!titles.contains(&"fresh"));
        assert!(titles.contains(&"every-day"));
    }

    #[test]
    fn review_queue_orders_never_reviewed_first_then_oldest() {
        let conn = fresh_conn();
        let _newest = insert_project_with_review(
            &conn,
            "a",
            "two days ago",
            Some(1),
            Some("2026-05-13T08:00:00.000Z"),
        );
        let _never = insert_project_with_review(&conn, "b", "never", Some(7), None);
        let _oldest = insert_project_with_review(
            &conn,
            "c",
            "two weeks ago",
            Some(1),
            Some("2026-05-01T08:00:00.000Z"),
        );

        let queue = list_review_queue(&conn, today()).unwrap();
        let titles: Vec<&str> = queue.iter().map(|p| p.title.as_str()).collect();
        // Never-reviewed first, then oldest review next, then most recent.
        assert_eq!(titles, vec!["never", "two weeks ago", "two days ago"]);
    }

    // v0.28.0 — per-area review default cascade.

    #[test]
    fn review_queue_honors_area_default_when_project_interval_null() {
        let conn = fresh_conn();
        let area = insert_area(&conn, "ar", "Work");
        conn.execute(
            "UPDATE area SET default_review_interval_days = 7 WHERE id = ?1",
            params![area],
        )
        .unwrap();

        // Project filed under the area, never reviewed, with no own
        // interval — it inherits the area default and enters the queue.
        let _inherits = insert_project(&conn, "p", "inherits", Some(area), None);
        // No area, no interval — stays out, exactly as before.
        let _orphan = insert_project(&conn, "q", "orphan", None, None);

        let queue = list_review_queue(&conn, today()).unwrap();
        let titles: Vec<&str> = queue.iter().map(|p| p.title.as_str()).collect();
        assert_eq!(titles, vec!["inherits"]);
    }

    #[test]
    fn review_queue_project_interval_overrides_area_default() {
        let conn = fresh_conn();
        // Area default is aggressive (1 day); the project sets its own
        // slower cadence (7 days). today = 2026-05-15.
        let area = insert_area(&conn, "ar", "Work");
        conn.execute(
            "UPDATE area SET default_review_interval_days = 1 WHERE id = ?1",
            params![area],
        )
        .unwrap();
        // Reviewed 5 days ago. Own interval 7 → next due 2026-05-17
        // (not yet). The area default 1 would make it overdue; if the
        // override didn't work this project would wrongly appear.
        conn.execute(
            "INSERT INTO project \
             (uuid, title, area_id, review_interval_days, last_reviewed_at, position) \
             VALUES (?, ?, ?, ?, ?, ?)",
            params!["p", "own wins", area, 7, "2026-05-10T08:00:00.000Z", 1.0],
        )
        .unwrap();

        let queue = list_review_queue(&conn, today()).unwrap();
        assert!(queue.is_empty());
    }

    // v0.29.0 — task dependencies.

    #[test]
    fn blocked_task_ids_and_list_prerequisites() {
        let conn = fresh_conn();
        insert_task(&conn, "a", "A", None, None, None, None);
        let a = conn.last_insert_rowid();
        insert_task(&conn, "b", "B", None, None, None, None);
        let b = conn.last_insert_rowid();
        // c is a completed prerequisite — present in the list, but it
        // doesn't gate availability.
        insert_task(
            &conn,
            "c",
            "C",
            None,
            None,
            None,
            Some("2026-05-01T00:00:00.000Z"),
        );
        let c = conn.last_insert_rowid();
        for (t, blocker) in [(a, b), (a, c)] {
            conn.execute(
                "INSERT INTO task_dependency (task_id, blocked_by_id) VALUES (?1, ?2)",
                params![t, blocker],
            )
            .unwrap();
        }

        // a has one open prerequisite (b) → blocked; b itself isn't.
        let blocked = blocked_task_ids(&conn).unwrap();
        assert!(blocked.contains(&a));
        assert!(!blocked.contains(&b));

        // list_prerequisites returns both blockers (open + completed).
        let prereqs = list_prerequisites(&conn, a).unwrap();
        let ids: Vec<i64> = prereqs.iter().map(|t| t.id).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&b) && ids.contains(&c));
    }

    #[test]
    fn blocked_excludes_task_when_all_prereqs_completed() {
        let conn = fresh_conn();
        insert_task(&conn, "a", "A", None, None, None, None);
        let a = conn.last_insert_rowid();
        insert_task(
            &conn,
            "b",
            "B",
            None,
            None,
            None,
            Some("2026-05-01T00:00:00.000Z"),
        );
        let b = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO task_dependency (task_id, blocked_by_id) VALUES (?1, ?2)",
            params![a, b],
        )
        .unwrap();
        // Only prerequisite is done → a is available, not blocked.
        assert!(!blocked_task_ids(&conn).unwrap().contains(&a));
    }

    #[test]
    fn deleting_a_task_cascades_its_dependency_rows() {
        let conn = fresh_conn();
        insert_task(&conn, "a", "A", None, None, None, None);
        let a = conn.last_insert_rowid();
        insert_task(&conn, "b", "B", None, None, None, None);
        let b = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO task_dependency (task_id, blocked_by_id) VALUES (?1, ?2)",
            params![a, b],
        )
        .unwrap();
        // Delete the prerequisite — the FK CASCADE drops the edge.
        conn.execute("DELETE FROM task WHERE id = ?1", params![b])
            .unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM task_dependency", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        assert!(!blocked_task_ids(&conn).unwrap().contains(&a));
    }

    #[test]
    fn list_projects_excludes_archived() {
        let conn = fresh_conn();
        insert_project(&conn, "p1", "Active", None, None);
        insert_project(&conn, "p2", "Done", None, Some("2026-04-15T00:00:00.000Z"));
        let projects = list_projects(&conn).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].title, "Active");
    }

    // ── Counts (Phase 5c) ──────────────────────────────────────────

    #[test]
    fn count_canonical_lists_all_zero_on_empty_db() {
        let conn = fresh_conn();
        let c = count_open_canonical(&conn, today()).unwrap();
        assert_eq!(c, CanonicalCounts::default());
    }

    #[test]
    fn count_canonical_distributes_across_lists() {
        let conn = fresh_conn();
        // i1 — unscheduled inbox task. Counts: Inbox + Anytime.
        insert_task(&conn, "i1", "in", None, None, None, None);
        // t1 — scheduled for today, no project. Counts: Inbox + Today.
        insert_task(&conn, "t1", "today", Some("2026-05-15"), None, None, None);
        // u1 — scheduled for the future, no project. Counts: Inbox + Upcoming.
        insert_task(&conn, "u1", "future", Some("2026-06-01"), None, None, None);
        // s1 — Someday-parked, no project. Counts: Inbox + Someday.
        insert_task(&conn, "s1", "later", Some("__someday__"), None, None, None);
        // l1 — completed (no schedule, no project). Counts: Logbook only
        // (Inbox excludes completed_at IS NOT NULL).
        insert_task(
            &conn,
            "l1",
            "done",
            None,
            None,
            None,
            Some("2026-04-01T00:00:00.000Z"),
        );

        let c = count_open_canonical(&conn, today()).unwrap();
        // Inbox per spec §4.2 is "project_id IS NULL AND completed_at IS NULL" —
        // it doesn't care about scheduled state. The four open unfiled tasks
        // (i1/t1/u1/s1) all qualify. Tasks routinely live in Inbox AND Today
        // simultaneously; that's by design.
        assert_eq!(c.inbox, 4);
        assert_eq!(c.today, 1);
        assert_eq!(c.upcoming, 1);
        assert_eq!(c.someday, 1);
        assert_eq!(c.logbook, 1);
        assert_eq!(c.anytime, 1);
    }

    #[test]
    fn count_per_project_groups_correctly() {
        let conn = fresh_conn();
        let p1 = insert_project(&conn, "p1", "P1", None, None);
        let p2 = insert_project(&conn, "p2", "P2", None, None);
        conn.execute(
            "INSERT INTO task (uuid, title, project_id, position) VALUES \
             ('a', 'a', ?, 1.0), ('b', 'b', ?, 2.0), ('c', 'c', ?, 3.0)",
            params![p1, p1, p2],
        )
        .unwrap();

        let counts = count_open_per_project(&conn).unwrap();
        assert_eq!(counts.get(&p1).copied(), Some(2));
        assert_eq!(counts.get(&p2).copied(), Some(1));
    }

    #[test]
    fn count_per_area_aggregates_across_projects() {
        let conn = fresh_conn();
        let area = insert_area(&conn, "a1", "Area");
        let p1 = insert_project(&conn, "p1", "Project 1", Some(area), None);
        let p2 = insert_project(&conn, "p2", "Project 2", Some(area), None);
        conn.execute(
            "INSERT INTO task (uuid, title, project_id, position) VALUES \
             ('a', 'a', ?, 1.0), ('b', 'b', ?, 2.0)",
            params![p1, p2],
        )
        .unwrap();

        let counts = count_open_per_area(&conn).unwrap();
        assert_eq!(counts.get(&area).copied(), Some(2));
    }

    // ── Search (Phase 7a) ──────────────────────────────────────────

    #[test]
    fn search_finds_token_in_title() {
        let conn = fresh_conn();
        insert_task(&conn, "a", "buy milk and bread", None, None, None, None);
        insert_task(&conn, "b", "call dentist", None, None, None, None);
        let rows = search_tasks(&conn, "milk").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].uuid, "a");
    }

    #[test]
    fn search_finds_token_in_note() {
        let conn = fresh_conn();
        conn.execute(
            "INSERT INTO task (uuid, title, note, position) VALUES ('a', 'Email someone', 'about Q3 project review', 1.0)",
            [],
        )
        .unwrap();
        let rows = search_tasks(&conn, "Q3").unwrap();
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn search_returns_empty_for_no_match() {
        let conn = fresh_conn();
        insert_task(&conn, "a", "buy milk", None, None, None, None);
        let rows = search_tasks(&conn, "zzz_no_match").unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn search_handles_empty_input() {
        let conn = fresh_conn();
        insert_task(&conn, "a", "buy milk", None, None, None, None);
        let rows = search_tasks(&conn, "").unwrap();
        assert!(rows.is_empty());
        let rows = search_tasks(&conn, "   ").unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn search_finds_phrase() {
        let conn = fresh_conn();
        insert_task(&conn, "a", "buy milk and bread", None, None, None, None);
        insert_task(
            &conn,
            "b",
            "milk delivery scheduled",
            None,
            None,
            None,
            None,
        );
        let rows = search_tasks(&conn, "buy milk").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].uuid, "a");
    }

    #[test]
    fn list_projects_groups_unfiled_first() {
        let conn = fresh_conn();
        // SQLite orders NULLs before non-NULLs by default.
        insert_project(&conn, "p1", "Unfiled", None, None);
        let area = insert_area(&conn, "a1", "Personal");
        insert_project(&conn, "p2", "Filed", Some(area), None);
        let projects = list_projects(&conn).unwrap();
        assert_eq!(projects.len(), 2);
        assert!(projects[0].area_id.is_none());
        assert!(projects[1].area_id.is_some());
    }

    #[test]
    fn bm25_for_terms_returns_one_score_per_match() {
        let conn = fresh_conn();
        insert_task(&conn, "a", "buy milk and bread", None, None, None, None);
        insert_task(&conn, "b", "schedule milk delivery", None, None, None, None);
        insert_task(&conn, "c", "wash the car", None, None, None, None);
        let scores = bm25_for_terms(&conn, &["milk".to_string()]).unwrap();
        assert_eq!(scores.len(), 2, "two tasks contain 'milk'");
        // FTS5's bm25 returns more-negative for a stronger match;
        // we keep the raw value, so every score must be ≤ 0.
        for s in scores.values() {
            assert!(*s <= 0.0, "bm25 should be ≤ 0; got {s}");
        }
    }

    #[test]
    fn bm25_for_terms_or_unions_terms() {
        let conn = fresh_conn();
        insert_task(&conn, "a", "buy milk", None, None, None, None);
        insert_task(&conn, "b", "buy bread", None, None, None, None);
        insert_task(&conn, "c", "wash the car", None, None, None, None);
        // Either "milk" or "bread" should pull both rows, but not the car.
        let scores = bm25_for_terms(&conn, &["milk".to_string(), "bread".to_string()]).unwrap();
        assert_eq!(scores.len(), 2);
    }

    #[test]
    fn bm25_for_terms_empty_input_is_empty() {
        let conn = fresh_conn();
        insert_task(&conn, "a", "buy milk", None, None, None, None);
        let scores = bm25_for_terms(&conn, &[]).unwrap();
        assert!(scores.is_empty());
    }

    #[test]
    fn bm25_for_terms_skips_blank_terms() {
        let conn = fresh_conn();
        insert_task(&conn, "a", "buy milk", None, None, None, None);
        // Blanks become empty phrases (just two quotes); we drop
        // those before issuing the MATCH so the user doesn't get a
        // FTS5 syntax error from `""`.
        let scores = bm25_for_terms(&conn, &["   ".to_string()]).unwrap();
        assert!(scores.is_empty());
    }

    #[test]
    fn bm25_for_terms_strips_double_quotes_in_input() {
        // A user-supplied bare term shouldn't be able to break out
        // of our own phrase-quoting and inject MATCH operators.
        let conn = fresh_conn();
        insert_task(&conn, "a", "buy milk", None, None, None, None);
        let scores = bm25_for_terms(&conn, &["mi\"lk".to_string()]).unwrap();
        // After stripping the inner quote we look for `milk`,
        // which matches.
        assert_eq!(scores.len(), 1);
    }

    #[test]
    fn bm25_for_terms_ranks_term_frequency() {
        // Two tasks contain "milk"; the one that mentions it twice
        // and has shorter overall content should score more
        // strongly (i.e., a more-negative bm25). The shape of bm25
        // is "shorter doc + more occurrences = higher relevance =
        // smaller (more negative) score."
        let conn = fresh_conn();
        let title_a = "milk milk";
        let title_b = "buy milk and bread and eggs at the store later today";
        insert_task(&conn, "a", title_a, None, None, None, None);
        insert_task(&conn, "b", title_b, None, None, None, None);
        let scores = bm25_for_terms(&conn, &["milk".to_string()]).unwrap();
        let id_a: i64 = conn
            .query_row("SELECT id FROM task WHERE uuid='a'", [], |r| r.get(0))
            .unwrap();
        let id_b: i64 = conn
            .query_row("SELECT id FROM task WHERE uuid='b'", [], |r| r.get(0))
            .unwrap();
        let score_a = scores[&id_a];
        let score_b = scores[&id_b];
        assert!(
            score_a < score_b,
            "stronger match (a) should have a smaller bm25 \
             than weaker match (b); got a={score_a}, b={score_b}"
        );
    }
}
