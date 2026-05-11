// SPDX-License-Identifier: MIT
//! Search-side read helpers: SQL fast-path execution + FTS5 ranking.
//! Extracted from `read.rs` in the v0.21.0 maintenance pass — search
//! has its own callers (`atrium-search`'s SQL translator, the in-app
//! Search bar) and its own concept (SqlBindValue). Better as its own
//! file.

use std::collections::HashMap;

use chrono::NaiveDate;
use rusqlite::{Connection, params};

use crate::domain::Task;
use crate::error::DbError;

use super::{TASK_COLUMNS, task_from_row};

/// Wire-level value for the SQL fast-path's bound parameters.
/// Mirrors `atrium_search::SqlValue` so binaries don't have to
/// know about `rusqlite::types::Value` directly. `From<atrium_search::SqlValue>`
/// lives over in `atrium-search/src/sql_translate.rs`.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlBindValue {
    Text(String),
    Int(i64),
    /// Date bound as `YYYY-MM-DD` text — matches the column storage
    /// shape (`scheduled_for`, `deadline`, `defer_until`, etc.).
    Date(NaiveDate),
}

impl SqlBindValue {
    fn to_rusqlite(&self) -> rusqlite::types::Value {
        match self {
            Self::Text(s) => rusqlite::types::Value::Text(s.clone()),
            Self::Int(n) => rusqlite::types::Value::Integer(*n),
            Self::Date(d) => rusqlite::types::Value::Text(d.format("%Y-%m-%d").to_string()),
        }
    }
}

/// Run a pre-built SQL `WHERE` fragment against the `task` table.
/// Used by the SQL-translation evaluator (`atrium-search`) — the
/// caller composes the fragment + bound params, this helper just
/// executes it and decodes rows.
///
/// Each row is selected with the standard `TASK_COLUMNS` set so the
/// resulting `Vec<Task>` is interchangeable with output from
/// `list_all_tasks`. Ordering is `t.position` so the post-query
/// in-memory rank/sort steps see a deterministic input.
///
/// `where_sql` is bound *literally* into the prepared statement —
/// the caller is responsible for ensuring it came from
/// `atrium_search::try_translate` (or an equally-trusted source)
/// rather than user input. `params` are bound positionally and
/// match the `?N` placeholders inside `where_sql`.
pub fn list_tasks_matching(
    conn: &Connection,
    where_sql: &str,
    params: &[SqlBindValue],
) -> Result<Vec<Task>, DbError> {
    let bound: Vec<rusqlite::types::Value> = params.iter().map(SqlBindValue::to_rusqlite).collect();
    let task_cols = TASK_COLUMNS
        .split(", ")
        .map(|c| format!("t.{c}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("SELECT {task_cols} FROM task t WHERE {where_sql} ORDER BY t.position");
    // Plain `prepare` rather than `prepare_cached` — the WHERE
    // fragment varies per query, so caching would unboundedly grow
    // the per-connection statement cache.
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(bound), task_from_row)?;
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

/// FTS5 bm25 scores for the given bare-text terms. Returns one
/// entry per task that matches *any* of the terms; absent rows are
/// "no match" (the caller falls back to its in-memory rank).
///
/// The bm25 contract: more negative = more relevant. We forward the
/// raw FTS5 score so the caller can apply its own blend (recency,
/// per-column weighting, etc.) without us coupling a policy in.
///
/// Callers don't need this for *correctness* — the in-memory
/// evaluator does substring on title + note and returns the same
/// match set. This is the *ranking* path: when bare text is in the
/// query and no `sort:` modifier was given, the call site can
/// reorder its already-filtered results by these scores blended
/// with recency. See `atrium_search::blend_relevance`.
pub fn bm25_for_terms(conn: &Connection, terms: &[String]) -> Result<HashMap<i64, f64>, DbError> {
    if terms.is_empty() {
        return Ok(HashMap::new());
    }
    // Sanitise each term: drop double quotes (we wrap in our own
    // quotes for phrase semantics), and reject any term that
    // becomes empty after trimming. FTS5 is permissive about most
    // ASCII but can choke on unbalanced quotes — quoting prevents
    // the user's text from injecting MATCH operators.
    let phrases: Vec<String> = terms
        .iter()
        .map(|t| {
            let clean: String = t.chars().filter(|c| *c != '"').collect();
            format!("\"{}\"", clean.trim())
        })
        .filter(|p| p.len() > 2) // 2 = the two quotes alone
        .collect();
    if phrases.is_empty() {
        return Ok(HashMap::new());
    }
    // FTS5's MATCH glues phrases with implicit AND when separated
    // by whitespace; we want OR so any term contributes. The
    // explicit `OR` keyword does that.
    let match_clause = phrases.join(" OR ");

    let sql = "SELECT rowid, bm25(task_fts) \
               FROM task_fts \
               WHERE task_fts MATCH ?1";
    let mut stmt = conn.prepare_cached(sql)?;
    let rows = stmt.query_map(params![match_clause], |row| {
        let id: i64 = row.get(0)?;
        let score: f64 = row.get(1)?;
        Ok((id, score))
    })?;
    rows.collect::<rusqlite::Result<HashMap<_, _>>>()
        .map_err(Into::into)
}
