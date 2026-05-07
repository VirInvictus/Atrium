// SPDX-License-Identifier: MIT
//! Read-side query helpers. Free functions that take a borrowed
//! `Connection` so they compose with both the worker's writable
//! connection (during command processing) and the `ReadPool`'s
//! read-only connections (during UI list refreshes).

use std::collections::HashMap;

use chrono::NaiveDate;
use rusqlite::{Connection, Row, params};

use crate::domain::{Area, Perspective, Project, ScheduledFor, Tag, Task};
use crate::error::DbError;

const TASK_COLUMNS: &str = "id, uuid, title, note, project_id, parent_id, \
    scheduled_for, deadline, defer_until, estimated_minutes, completed_at, \
    repeat_rule, repeat_mode, position, created_at, modified_at";

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

/// Total task count, including completed.
pub fn count_tasks(conn: &Connection) -> Result<i64, DbError> {
    Ok(conn.query_row("SELECT count(*) FROM task", [], |r| r.get(0))?)
}

/// Heads-up window for upcoming deadlines surfaced in Today (spec
/// §4.2). A task with a deadline within `today + N` days appears in
/// Today even before that deadline arrives, matching Things 3's
/// "deadlines approaching" behaviour. The window stays at one
/// constant for v0.1; turning it into a GSettings key is a Phase 8d
/// preferences task.
pub const TODAY_DEADLINE_WINDOW_DAYS: i64 = 7;

/// Today list per spec §4.2:
///
/// > `task WHERE completed_at IS NULL`
/// > `  AND ( scheduled_for ≤ today`
/// > `        OR deadline ≤ today + TODAY_DEADLINE_WINDOW_DAYS )`
/// > `  AND ( defer_until IS NULL OR defer_until ≤ today )`
///
/// The `scheduled_for != '__someday__'` clause is the implementation
/// detail that keeps the Someday sentinel out of Today: ISO date
/// strings sort lexicographically, but `__someday__` starts with
/// underscores (`0x5F`) which compare *less than* any digit, so a
/// naive `scheduled_for <= ?today` would otherwise match it.
///
/// The `deadline ≤ today + window` clause is the v0.0.38 Things-3
/// alignment: deadlines approaching surface as a heads-up so the
/// user isn't blindsided. Earlier versions used `deadline ≤ today`,
/// which left a future-deadlined task buried in Anytime until its
/// deadline date arrived.
pub fn list_today(conn: &Connection, today: NaiveDate) -> Result<Vec<Task>, DbError> {
    let today_str = today.format("%Y-%m-%d").to_string();
    let horizon_str = (today + chrono::Duration::days(TODAY_DEADLINE_WINDOW_DAYS))
        .format("%Y-%m-%d")
        .to_string();
    let sql = format!(
        "SELECT {TASK_COLUMNS} FROM task \
         WHERE completed_at IS NULL \
           AND ( \
                 (scheduled_for IS NOT NULL \
                    AND scheduled_for != '__someday__' \
                    AND scheduled_for <= ?1) \
                 OR (deadline IS NOT NULL AND deadline <= ?2) \
               ) \
           AND (defer_until IS NULL OR defer_until <= ?1) \
         ORDER BY position"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![today_str, horizon_str], task_from_row)?;
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

/// All open tasks across the area's projects, ordered by project
/// position then task position. Aggregate "click an area in the
/// sidebar" view per spec §5.1.
pub fn list_area(conn: &Connection, area_id: i64) -> Result<Vec<Task>, DbError> {
    let sql = format!(
        "SELECT {} FROM task t \
         JOIN project p ON t.project_id = p.id \
         WHERE p.area_id = ?1 AND t.completed_at IS NULL \
         ORDER BY p.position, t.position",
        TASK_COLUMNS
            .split(", ")
            .map(|c| format!("t.{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![area_id], task_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

const AREA_COLUMNS: &str = "id, uuid, title, position, created_at, modified_at";

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

/// Open-task counts for the six canonical Simple Mode lists. Phase 5c
/// surfaces these as sidebar badges (hidden when zero).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CanonicalCounts {
    pub inbox: i64,
    pub today: i64,
    pub upcoming: i64,
    pub anytime: i64,
    pub someday: i64,
    pub logbook: i64,
}

/// Compute all six canonical-list counts in one batched call.
pub fn count_open_canonical(
    conn: &Connection,
    today: NaiveDate,
) -> Result<CanonicalCounts, DbError> {
    let today_str = today.format("%Y-%m-%d").to_string();

    let inbox: i64 = conn.query_row(
        "SELECT count(*) FROM task WHERE project_id IS NULL AND completed_at IS NULL",
        [],
        |r| r.get(0),
    )?;

    let horizon_str = (today + chrono::Duration::days(TODAY_DEADLINE_WINDOW_DAYS))
        .format("%Y-%m-%d")
        .to_string();
    let today_count: i64 = conn.query_row(
        "SELECT count(*) FROM task \
         WHERE completed_at IS NULL \
           AND ( \
                 (scheduled_for IS NOT NULL \
                    AND scheduled_for != '__someday__' \
                    AND scheduled_for <= ?1) \
                 OR (deadline IS NOT NULL AND deadline <= ?2) \
               ) \
           AND (defer_until IS NULL OR defer_until <= ?1)",
        params![today_str, horizon_str],
        |r| r.get(0),
    )?;

    let upcoming: i64 = conn.query_row(
        "SELECT count(*) FROM task \
         WHERE completed_at IS NULL \
           AND scheduled_for IS NOT NULL \
           AND scheduled_for != '__someday__' \
           AND scheduled_for > ?1",
        params![today_str],
        |r| r.get(0),
    )?;

    let anytime: i64 = conn.query_row(
        "SELECT count(*) FROM task \
         WHERE completed_at IS NULL \
           AND scheduled_for IS NULL \
           AND (defer_until IS NULL OR defer_until <= ?1)",
        params![today_str],
        |r| r.get(0),
    )?;

    let someday: i64 = conn.query_row(
        "SELECT count(*) FROM task \
         WHERE completed_at IS NULL AND scheduled_for = '__someday__'",
        [],
        |r| r.get(0),
    )?;

    let logbook: i64 = conn.query_row(
        "SELECT count(*) FROM task WHERE completed_at IS NOT NULL",
        [],
        |r| r.get(0),
    )?;

    Ok(CanonicalCounts {
        inbox,
        today: today_count,
        upcoming,
        anytime,
        someday,
        logbook,
    })
}

/// Open-task count per project, keyed by project id.
pub fn count_open_per_project(conn: &Connection) -> Result<HashMap<i64, i64>, DbError> {
    let mut stmt = conn.prepare_cached(
        "SELECT project_id, count(*) FROM task \
         WHERE project_id IS NOT NULL AND completed_at IS NULL \
         GROUP BY project_id",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    let mut out = HashMap::new();
    for row in rows {
        let (pid, c) = row?;
        out.insert(pid, c);
    }
    Ok(out)
}

/// Open-task count per area (aggregated across the area's projects),
/// keyed by area id.
pub fn count_open_per_area(conn: &Connection) -> Result<HashMap<i64, i64>, DbError> {
    let mut stmt = conn.prepare_cached(
        "SELECT p.area_id, count(*) FROM task t \
         JOIN project p ON t.project_id = p.id \
         WHERE p.area_id IS NOT NULL AND t.completed_at IS NULL \
         GROUP BY p.area_id",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    let mut out = HashMap::new();
    for row in rows {
        let (aid, c) = row?;
        out.insert(aid, c);
    }
    Ok(out)
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
    let sql = format!(
        "SELECT {PROJECT_COLUMNS} FROM project \
         WHERE review_interval_days IS NOT NULL \
           AND archived_at IS NULL \
           AND ( \
                 last_reviewed_at IS NULL \
                 OR date(last_reviewed_at, '+' || review_interval_days || ' days') <= ?1 \
               ) \
         ORDER BY \
             CASE WHEN last_reviewed_at IS NULL THEN 0 ELSE 1 END, \
             last_reviewed_at ASC, \
             position"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![today_str], project_from_row)?;
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
    position, created_at, modified_at";

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

/// FTS5-backed search over `task.title` + `task.note`. Returns
/// matches ranked by `bm25` (FTS5's default — closer to the top means
/// stronger relevance). Phase 7a's "recency × relevance" requirement
/// from spec §4.3 lands as a follow-up multiplier; this is the
/// pure-relevance base.
///
/// `query` is wrapped in double quotes so user input is treated as a
/// phrase search by default — we don't expose FTS5's `OR`/`NOT`
/// operators yet (that's Phase 7c filter expressions). Internal
/// double quotes in the user input are stripped to keep the wrapping
/// well-formed.
pub fn search_tasks(conn: &Connection, query: &str) -> Result<Vec<Task>, DbError> {
    let cleaned: String = query.chars().filter(|c| *c != '"').collect();
    if cleaned.trim().is_empty() {
        return Ok(Vec::new());
    }
    let phrase = format!("\"{}\"", cleaned.trim());

    let task_cols = TASK_COLUMNS
        .split(", ")
        .map(|c| format!("t.{c}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT {task_cols} FROM task t \
         JOIN task_fts ON task_fts.rowid = t.id \
         WHERE task_fts MATCH ?1 \
         ORDER BY rank",
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![phrase], task_from_row)?;
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
        TASK_COLUMNS
            .split(", ")
            .map(|c| format!("t.{c}"))
            .collect::<Vec<_>>()
            .join(", ")
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

/// Open-task counts per tag id. Sidebar Tags section consumes this
/// for badge values.
pub fn count_open_per_tag(conn: &Connection) -> Result<HashMap<i64, i64>, DbError> {
    let mut stmt = conn.prepare_cached(
        "SELECT tt.tag_id, count(*) FROM task t \
         JOIN task_tag tt ON tt.task_id = t.id \
         WHERE t.completed_at IS NULL \
         GROUP BY tt.tag_id",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    let mut out = HashMap::new();
    for row in rows {
        let (tid, c) = row?;
        out.insert(tid, c);
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

fn task_from_row(row: &Row<'_>) -> rusqlite::Result<Task> {
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
}
