// SPDX-License-Identifier: MIT
//! Counting queries — sidebar badges, statistics-cookie projections,
//! per-canonical-list batched counts. Extracted from `read.rs` in
//! the v0.21.0 maintenance pass — counts are a coherent group:
//! they share the same shape (return a HashMap or struct of
//! integers), they're invoked together by the sidebar refresh
//! batch, and they're isolated from the per-row read helpers.

use std::collections::HashMap;

use chrono::NaiveDate;
use rusqlite::{Connection, params};

use crate::error::DbError;

use super::TODAY_DEADLINE_WINDOW_DAYS;

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

/// Total task count, including completed.
pub fn count_tasks(conn: &Connection) -> Result<i64, DbError> {
    Ok(conn.query_row("SELECT count(*) FROM task", [], |r| r.get(0))?)
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

    // Mirror list_today's per-row deadline horizon (v0.14.0): each
    // task's effective horizon is `today + COALESCE(deadline_warn_days,
    // global_default)`. Sidebar badge must match list contents.
    let today_count: i64 = conn.query_row(
        "SELECT count(*) FROM task \
         WHERE completed_at IS NULL \
           AND ( \
                 (scheduled_for IS NOT NULL \
                    AND scheduled_for != '__someday__' \
                    AND scheduled_for <= ?1) \
                 OR (deadline IS NOT NULL \
                     AND deadline <= date(?1, '+' || COALESCE(deadline_warn_days, ?2) || ' days')) \
               ) \
           AND (defer_until IS NULL OR defer_until <= ?1)",
        params![today_str, TODAY_DEADLINE_WINDOW_DAYS],
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

/// v0.15.0 — Phase 18.5 Tier-1 statistics-cookie projection.
/// `(done, total)` per project, keyed by project id. Total
/// counts every task in the project (including completed); done
/// counts the subset with non-NULL `completed_at`. The
/// projection feeds both the inline `[N/M]` cookie on the project
/// sub-heading the writer emits and the future cookie display on
/// the project sidebar entry. Projects that hold zero tasks are
/// absent from the map (cookie isn't meaningful when there's
/// nothing to count).
pub fn count_done_total_per_project(
    conn: &Connection,
) -> Result<HashMap<i64, (u32, u32)>, DbError> {
    let mut stmt = conn.prepare_cached(
        "SELECT project_id, \
                SUM(CASE WHEN completed_at IS NOT NULL THEN 1 ELSE 0 END), \
                count(*) \
         FROM task \
         WHERE project_id IS NOT NULL \
         GROUP BY project_id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (pid, done, total) = row?;
        // Counts can never realistically overflow u32 (4B tasks per
        // project), but be defensive at the cast.
        let done = u32::try_from(done).unwrap_or(u32::MAX);
        let total = u32::try_from(total).unwrap_or(u32::MAX);
        out.insert(pid, (done, total));
    }
    Ok(out)
}

/// v0.15.0 — `(done, total)` per parent task, keyed by parent
/// task id. Counts immediate children (one level only — Org's
/// statistics cookie convention is per-headline, not recursive,
/// matching `org-hierarchical-todo-statistics` defaults). Parent
/// tasks with zero children are absent from the map.
pub fn count_done_total_per_parent(conn: &Connection) -> Result<HashMap<i64, (u32, u32)>, DbError> {
    let mut stmt = conn.prepare_cached(
        "SELECT parent_id, \
                SUM(CASE WHEN completed_at IS NOT NULL THEN 1 ELSE 0 END), \
                count(*) \
         FROM task \
         WHERE parent_id IS NOT NULL \
         GROUP BY parent_id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (pid, done, total) = row?;
        let done = u32::try_from(done).unwrap_or(u32::MAX);
        let total = u32::try_from(total).unwrap_or(u32::MAX);
        out.insert(pid, (done, total));
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
