// SPDX-License-Identifier: MIT
//! SQL translation for the subset of [`Expr`] that SQLite can express.
//!
//! [`try_translate`] walks a parsed expression and emits a `WHERE`
//! fragment + parameter list when *every* node in the tree maps
//! cleanly onto SQL. When *any* node can't be translated (regex match
//! modifiers, fuzzy match, sequential-project state, or the more
//! involved composite predicates like `is:today`), we return `None`
//! and the caller falls back to the in-memory evaluator. The
//! "all-or-nothing" rule keeps the semantics in lockstep — there's
//! no path where the SQL evaluator silently diverges from the
//! in-memory one.
//!
//! ## Coverage (v0.5.3)
//!
//! - Boolean composition: `AND`, `OR`, `NOT`, `Pass`.
//! - Bare text: `Expr::Text(_)` — case-insensitive substring on
//!   `title` and `note`.
//! - Field-scoped substring/exact:
//!   - `title:`, `note:`
//!   - `tag:` / `tags:` (via `EXISTS` subquery on `task_tag`)
//!   - `repeats:true` / `repeats:false`
//! - State predicates: `is:open`, `is:done`/`is:logbook`,
//!   `is:overdue`, `is:scheduled`, `is:deadline`, `is:deferred`,
//!   `is:repeating`, `is:inproject`, `is:tagged`.
//! - Date comparisons / ranges on `due`, `scheduled`, `defer`,
//!   `created`, `modified`, `completed`.
//! - Numeric comparison on `estimated:`.
//!
//! ## Falls back to in-memory (returns `None`)
//!
//! - `MatchKind::Regex` and `MatchKind::Fuzzy` — SQLite has no regex
//!   built-in and Damerau-Levenshtein isn't expressible inline.
//! - `State::Available` / `State::Queued` — depend on sequential-
//!   project ordering that would require a window function with
//!   ordering by position; deferred.
//! - `State::Today` / `State::Inbox` / `State::Upcoming` /
//!   `State::Anytime` / `State::Someday` — composite list-membership
//!   predicates; the `read::list_NAME` helpers exist already, but
//!   the search-bar path uses them via `is:NAME` here. Deferred until
//!   we have a clean way to express them as subexpressions; until
//!   then the in-memory eval handles them.
//! - `State::InArea`, `State::Archived` — need joins through
//!   `project.area_id` / `project.archived_at`. Deferred.
//! - `Field::Project`, `Field::Area` — would need a JOIN through
//!   `project` and possibly `area`. Doable; deferred for v1.
//! - `Field::Tag` with `MatchKind::Boolean(_)` — the boolean form
//!   is already covered as `is:tagged`; the `tag:true` / `tag:false`
//!   syntax is rare. Deferred.

use chrono::NaiveDate;

use crate::search::domain::{Field, State};
use vir_search::ast::{Comparator, Expr, MatchKind, Value};

/// Output of [`try_translate`]. The SQL fragment goes inside a
/// `WHERE …` clause; params are bound positionally.
#[derive(Debug, Clone, PartialEq)]
pub struct SqlClause {
    /// SQL `WHERE` fragment. Always wrapped in parens at the top
    /// level so the caller can compose it freely (e.g.
    /// `WHERE {clause.sql} AND completed_at IS NULL`).
    pub sql: String,
    /// Parameters in the order they appear in `sql`. The caller
    /// binds these positionally via `rusqlite::params_from_iter`.
    pub params: Vec<SqlValue>,
}

/// Wire-level value for parameter binding. Kept dep-free (no
/// rusqlite types here) so the search crate stays GUI/storage
/// agnostic. The caller maps these to its driver's bind types —
/// the `From<SqlValue> for crate::SqlBindValue` impl below
/// covers the common rusqlite path so binaries don't have to
/// know about either side.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    Text(String),
    Int(i64),
    /// Date bound as `YYYY-MM-DD` text — matches the column storage
    /// shape (`scheduled_for`, `deadline`, `defer_until`).
    Date(NaiveDate),
}

impl From<SqlValue> for crate::SqlBindValue {
    fn from(value: SqlValue) -> Self {
        match value {
            SqlValue::Text(s) => crate::SqlBindValue::Text(s),
            SqlValue::Int(n) => crate::SqlBindValue::Int(n),
            SqlValue::Date(d) => crate::SqlBindValue::Date(d),
        }
    }
}

impl From<&SqlValue> for crate::SqlBindValue {
    fn from(value: &SqlValue) -> Self {
        value.clone().into()
    }
}

/// Try to translate `expr` into a SQL `WHERE` fragment.
///
/// Returns `None` if any subtree contains an operator we can't
/// express. The caller falls back to the in-memory evaluator
/// (`atrium_search::evaluate`) — semantically identical, just
/// slower at scale.
///
/// `today` resolves date keywords (`thisweek`, `5daysago`, etc.)
/// to concrete dates at translation time.
pub fn try_translate(expr: &Expr<Field, State>, today: NaiveDate) -> Option<SqlClause> {
    let mut params = Vec::new();
    let sql = translate(expr, today, &mut params)?;
    Some(SqlClause { sql, params })
}

fn translate(
    expr: &Expr<Field, State>,
    today: NaiveDate,
    params: &mut Vec<SqlValue>,
) -> Option<String> {
    match expr {
        Expr::Empty => Some("1".into()),
        Expr::Text(s) => Some(text_search_clause(s, params)),
        Expr::State(state) => state_clause(*state, today, params),
        Expr::Field { field, kind } => field_clause(*field, kind, params),
        Expr::Compare { field, comp, value } => compare_clause(*field, *comp, value, today, params),
        Expr::Range { field, low, high } => range_clause(*field, low, high, today, params),
        Expr::Not(inner) => {
            let inner_sql = translate(inner, today, params)?;
            Some(format!("(NOT {inner_sql})"))
        }
        Expr::And(items) => combine(items, "AND", today, params),
        Expr::Or(items) => combine(items, "OR", today, params),
    }
}

fn combine(
    items: &[Expr<Field, State>],
    op: &str,
    today: NaiveDate,
    params: &mut Vec<SqlValue>,
) -> Option<String> {
    if items.is_empty() {
        // Empty AND is identity true; empty OR is identity false.
        // The parser shouldn't produce these but be defensive.
        return Some(if op == "AND" { "1" } else { "0" }.into());
    }
    let mut parts = Vec::with_capacity(items.len());
    for item in items {
        parts.push(translate(item, today, params)?);
    }
    Some(format!("({})", parts.join(&format!(" {op} "))))
}

/// Bare text → case-insensitive substring on title + note. Wrap user
/// text in `%…%` (after escaping LIKE wildcards) so the user can
/// type `100% sure` without it being interpreted as a wildcard.
fn text_search_clause(needle: &str, params: &mut Vec<SqlValue>) -> String {
    let pattern = format!("%{}%", escape_like(&needle.to_ascii_lowercase()));
    params.push(SqlValue::Text(pattern));
    "(LOWER(t.title) LIKE ?1 ESCAPE '\\' OR LOWER(t.note) LIKE ?1 ESCAPE '\\')"
        .replace("?1", &placeholder(params.len()))
}

fn state_clause(state: State, today: NaiveDate, params: &mut Vec<SqlValue>) -> Option<String> {
    Some(match state {
        State::Open => "t.completed_at IS NULL".into(),
        State::Done | State::Logbook => "t.completed_at IS NOT NULL".into(),
        State::Overdue => {
            params.push(SqlValue::Date(today));
            format!(
                "(t.completed_at IS NULL AND t.deadline IS NOT NULL AND t.deadline < {})",
                placeholder(params.len())
            )
        }
        State::Scheduled => "t.scheduled_for IS NOT NULL".into(),
        State::Deadline => "t.deadline IS NOT NULL".into(),
        State::Deferred => {
            params.push(SqlValue::Date(today));
            format!(
                "(t.defer_until IS NOT NULL AND t.defer_until > {})",
                placeholder(params.len())
            )
        }
        State::Repeating => "t.repeat_rule IS NOT NULL".into(),
        State::InProject => "t.project_id IS NOT NULL".into(),
        State::Tagged => "EXISTS (SELECT 1 FROM task_tag tt WHERE tt.task_id = t.id)".into(),
        // v0.29.0 — dependency availability. "Blocked" = open with an
        // open prerequisite; "available" = the open, not-blocked
        // complement. Must mirror `match_state` in eval.rs exactly so
        // the SQL fast-path and the in-memory fallback agree.
        State::Blocked => "(t.completed_at IS NULL AND EXISTS (SELECT 1 FROM \
             task_dependency d JOIN task b ON d.blocked_by_id = b.id \
             WHERE d.task_id = t.id AND b.completed_at IS NULL))"
            .into(),
        State::Available => "(t.completed_at IS NULL AND NOT EXISTS (SELECT 1 FROM \
             task_dependency d JOIN task b ON d.blocked_by_id = b.id \
             WHERE d.task_id = t.id AND b.completed_at IS NULL))"
            .into(),
        // Fall-back cases — handled by the in-memory evaluator.
        // Marked explicitly so a new `State` variant added later
        // forces a compile error here rather than silently drifting.
        State::Queued
        | State::Today
        | State::Inbox
        | State::Upcoming
        | State::Anytime
        | State::Someday
        | State::InArea
        | State::Archived => return None,
    })
}

fn field_clause(field: Field, kind: &MatchKind, params: &mut Vec<SqlValue>) -> Option<String> {
    match (field, kind) {
        // Title / note column matches.
        (Field::Title, MatchKind::Substring(s)) => Some(like_lower("t.title", s, params)),
        (Field::Title, MatchKind::Exact(s)) => Some(eq_lower("t.title", s, params)),
        (Field::Note, MatchKind::Substring(s)) => Some(like_lower("t.note", s, params)),
        (Field::Note, MatchKind::Exact(s)) => Some(eq_lower("t.note", s, params)),

        // Tag — EXISTS subquery against task_tag JOIN tag.
        (Field::Tag, MatchKind::Substring(s)) => Some(tag_exists_like(s, params)),
        (Field::Tag, MatchKind::Exact(s)) => Some(tag_exists_eq(s, params)),
        (Field::Tag, MatchKind::HasAny) => {
            Some("EXISTS (SELECT 1 FROM task_tag tt WHERE tt.task_id = t.id)".into())
        }
        (Field::Tag, MatchKind::HasNone) => {
            Some("NOT EXISTS (SELECT 1 FROM task_tag tt WHERE tt.task_id = t.id)".into())
        }

        // `repeats:true` / `repeats:false` — boolean existence on
        // the repeat_rule column.
        (Field::Repeats, MatchKind::HasAny) => Some("t.repeat_rule IS NOT NULL".into()),
        (Field::Repeats, MatchKind::HasNone) => Some("t.repeat_rule IS NULL".into()),

        // Regex / Fuzzy — fall back; SQLite can't express them
        // safely inline. Project/Area joins deferred for v1.
        _ => None,
    }
}

fn compare_clause(
    field: Field,
    comp: Comparator,
    value: &Value,
    today: NaiveDate,
    params: &mut Vec<SqlValue>,
) -> Option<String> {
    match field {
        // Numeric comparison — only `estimated:` for now.
        Field::Estimated => {
            let n = match value {
                Value::Int(n) => *n,
                _ => return None,
            };
            params.push(SqlValue::Int(n));
            Some(format!(
                "(t.estimated_minutes IS NOT NULL AND t.estimated_minutes {} {})",
                comp_op(comp),
                placeholder(params.len())
            ))
        }
        Field::Due
        | Field::Scheduled
        | Field::Defer
        | Field::Created
        | Field::Modified
        | Field::Completed => {
            let column = date_column(field)?;
            let (lo_epoch, hi_epoch) = match value {
                vir_search::ast::Value::Date(spec) => vir_search::dates::resolve_range(spec, today),
                _ => unreachable!(),
            };
            let lo = chrono::DateTime::from_timestamp(lo_epoch, 0)
                .unwrap()
                .naive_utc()
                .date();
            let hi = chrono::DateTime::from_timestamp(hi_epoch, 0)
                .unwrap()
                .naive_utc()
                .date();
            // Date keywords like `thisweek` produce a range; the
            // comparator semantics mirror the in-memory evaluator's
            // half-open `[lo, hi)` (see `dates::matches`): `Eq` means
            // "inside the range", `Ne` means "outside", `Le` means
            // "before hi", `Gt` means "at or after hi". A single-day
            // RHS collapses to lo == hi - 1 day so these reduce to
            // the obvious day comparisons. We push only the params
            // actually referenced in the SQL — binding an unused
            // param to `params_from_iter` errors at run time, so
            // eq/ne bind two and the others bind one.
            //
            // The upper bound is EXCLUSIVE. Binding `<= hi` instead
            // let a task dated exactly `hi` match through SQL while
            // the evaluator excluded it: `deadline:today` also
            // matched tomorrow, `<=today` also matched tomorrow, and
            // `>today` missed it — a one-day parity break at every
            // range boundary.
            Some(match comp {
                Comparator::Eq => {
                    params.push(SqlValue::Date(lo));
                    let lo_ph = placeholder(params.len());
                    params.push(SqlValue::Date(hi));
                    let hi_ph = placeholder(params.len());
                    format!("({column} IS NOT NULL AND {column} >= {lo_ph} AND {column} < {hi_ph})")
                }
                Comparator::Ne => {
                    params.push(SqlValue::Date(lo));
                    let lo_ph = placeholder(params.len());
                    params.push(SqlValue::Date(hi));
                    let hi_ph = placeholder(params.len());
                    // NOT NULL, not `IS NULL OR`: the in-memory
                    // evaluator returns false for every comparator
                    // when the field has no date (match_compare),
                    // so a dateless task must not match `!=` here
                    // either.
                    format!(
                        "({column} IS NOT NULL AND ({column} < {lo_ph} OR {column} >= {hi_ph}))"
                    )
                }
                Comparator::Lt => {
                    params.push(SqlValue::Date(lo));
                    let lo_ph = placeholder(params.len());
                    format!("({column} IS NOT NULL AND {column} < {lo_ph})")
                }
                Comparator::Le => {
                    params.push(SqlValue::Date(hi));
                    let hi_ph = placeholder(params.len());
                    format!("({column} IS NOT NULL AND {column} < {hi_ph})")
                }
                Comparator::Gt => {
                    params.push(SqlValue::Date(hi));
                    let hi_ph = placeholder(params.len());
                    format!("({column} IS NOT NULL AND {column} >= {hi_ph})")
                }
                Comparator::Ge => {
                    params.push(SqlValue::Date(lo));
                    let lo_ph = placeholder(params.len());
                    format!("({column} IS NOT NULL AND {column} >= {lo_ph})")
                }
            })
        }
        // Tag/Project/Area/Title/Note/Repeats don't take comparators.
        _ => None,
    }
}

fn range_clause(
    field: Field,
    low: &Value,
    high: &Value,
    today: NaiveDate,
    params: &mut Vec<SqlValue>,
) -> Option<String> {
    let column = date_column(field)?;
    // `a..b` is day-a through day-b, both inclusive: take the LOW
    // end of each bound's resolved range, mirroring the evaluator's
    // match_range. (The high bound's own upper edge is exclusive —
    // binding it `<=` let the day after b match.)
    let (low_lo_epoch, _) = match low {
        vir_search::ast::Value::Date(spec) => vir_search::dates::resolve_range(spec, today),
        _ => unreachable!(),
    };
    let low_lo = chrono::DateTime::from_timestamp(low_lo_epoch, 0)
        .unwrap()
        .naive_utc()
        .date();
    let (high_lo_epoch, _) = match high {
        vir_search::ast::Value::Date(spec) => vir_search::dates::resolve_range(spec, today),
        _ => unreachable!(),
    };
    let high_lo = chrono::DateTime::from_timestamp(high_lo_epoch, 0)
        .unwrap()
        .naive_utc()
        .date();
    params.push(SqlValue::Date(low_lo));
    let lo_ph = placeholder(params.len());
    params.push(SqlValue::Date(high_lo));
    let hi_ph = placeholder(params.len());
    Some(format!(
        "({column} IS NOT NULL AND {column} >= {lo_ph} AND {column} <= {hi_ph})"
    ))
}

fn date_column(field: Field) -> Option<&'static str> {
    Some(match field {
        Field::Due => "t.deadline",
        // The `'__someday__'` sentinel must read as "no date" in
        // comparisons, matching the evaluator's field_date_value
        // (None unless ScheduledFor::Date). Without the CASE, the
        // sentinel — which sorts above every ISO date because `_`
        // > any digit — wrongly matches `scheduled:>…` in the SQL
        // fast path while the in-memory fallback excludes it.
        Field::Scheduled => {
            "(CASE WHEN t.scheduled_for = '__someday__' THEN NULL ELSE t.scheduled_for END)"
        }
        Field::Defer => "t.defer_until",
        // The created/modified/completed columns store a full
        // RFC3339 UTC timestamp. The evaluator compares each
        // instant's LOCAL calendar date (`with_timezone(&Local)`
        // before truncating), which is what a user means by
        // "created today" — a task created 22:00 local lands on
        // the next UTC day, and comparing the raw UTC prefix
        // shifted it out of `created:today`. SQLite's
        // `'localtime'` modifier does the same conversion here, so
        // both paths compare local dates. SQLite's text-prefix
        // `>=`/`<` works because `DATE(...)` output and the bound
        // `YYYY-MM-DD` params sort lexicographically.
        Field::Created => "DATE(t.created_at, 'localtime')",
        Field::Modified => "DATE(t.modified_at, 'localtime')",
        Field::Completed => "DATE(t.completed_at, 'localtime')",
        _ => return None,
    })
}

// ── small helpers ─────────────────────────────────────────────

fn like_lower(column: &str, needle: &str, params: &mut Vec<SqlValue>) -> String {
    let pattern = format!("%{}%", escape_like(&needle.to_ascii_lowercase()));
    params.push(SqlValue::Text(pattern));
    format!(
        "LOWER({column}) LIKE {} ESCAPE '\\'",
        placeholder(params.len())
    )
}

fn eq_lower(column: &str, needle: &str, params: &mut Vec<SqlValue>) -> String {
    params.push(SqlValue::Text(needle.to_ascii_lowercase()));
    format!("LOWER({column}) = {}", placeholder(params.len()))
}

fn tag_exists_like(needle: &str, params: &mut Vec<SqlValue>) -> String {
    let pattern = format!("%{}%", escape_like(&needle.to_ascii_lowercase()));
    params.push(SqlValue::Text(pattern));
    format!(
        "EXISTS (SELECT 1 FROM task_tag tt JOIN tag g ON g.id = tt.tag_id \
         WHERE tt.task_id = t.id AND LOWER(g.name) LIKE {} ESCAPE '\\')",
        placeholder(params.len())
    )
}

fn tag_exists_eq(needle: &str, params: &mut Vec<SqlValue>) -> String {
    params.push(SqlValue::Text(needle.to_ascii_lowercase()));
    format!(
        "EXISTS (SELECT 1 FROM task_tag tt JOIN tag g ON g.id = tt.tag_id \
         WHERE tt.task_id = t.id AND LOWER(g.name) = {})",
        placeholder(params.len())
    )
}

fn comp_op(comp: Comparator) -> &'static str {
    match comp {
        Comparator::Eq => "=",
        Comparator::Ne => "!=",
        Comparator::Lt => "<",
        Comparator::Le => "<=",
        Comparator::Gt => ">",
        Comparator::Ge => ">=",
    }
}

/// Escape SQL `LIKE` metacharacters so user-supplied text is treated
/// literally. We always pair this with `ESCAPE '\\'` in the SQL.
fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | '%' | '_' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Positional placeholder (`?N`) given the parameter index. Keeps
/// the indices and the params Vec in lockstep — the caller binds
/// `params[0]` to `?1`, `params[1]` to `?2`, etc.
fn placeholder(one_based_index: usize) -> String {
    format!("?{one_based_index}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use chrono::{DateTime, Utc};
    use std::collections::HashMap;
    use vir_search::ast::Expr;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn translate_sql(query: &str, today: NaiveDate) -> SqlClause {
        let parsed = crate::search::parse(query);
        try_translate(&parsed.expr, today).expect("query must translate cleanly")
    }

    fn dates(clause: &SqlClause) -> Vec<NaiveDate> {
        clause
            .params
            .iter()
            .map(|p| match p {
                SqlValue::Date(d) => *d,
                other => panic!("unexpected param {other:?}"),
            })
            .collect()
    }

    #[test]
    fn eq_binds_half_open_upper_bound() {
        // `deadline:2026-06-10` must match ONLY that day. The old
        // `<= hi` binding matched 2026-06-11 too — a one-day break
        // against the evaluator's half-open `[lo, hi)`.
        let c = translate_sql("deadline:2026-06-10", d(2026, 6, 15));
        assert!(c.sql.contains(">= ?1"));
        assert!(c.sql.contains("< ?2"));
        assert!(!c.sql.contains("<="));
        assert_eq!(dates(&c), vec![d(2026, 6, 10), d(2026, 6, 11)]);
    }

    #[test]
    fn comparator_matrix_mirrors_evaluator_bounds() {
        // Each comparator against a single-day RHS, with the bound
        // the in-memory evaluator uses (`dates::matches`): Lt/Ge on
        // lo, Eq on [lo, hi), Le/Gt on hi, Ne outside it.
        let cases: &[(&str, &str, Vec<NaiveDate>)] = &[
            ("=", ">= ?1 AND < ?2", vec![d(2026, 6, 10), d(2026, 6, 11)]),
            ("!=", "< ?1 OR >= ?2", vec![d(2026, 6, 10), d(2026, 6, 11)]),
            ("<", "< ?1", vec![d(2026, 6, 10)]),
            ("<=", "< ?1", vec![d(2026, 6, 11)]),
            (">", ">= ?1", vec![d(2026, 6, 11)]),
            (">=", ">= ?1", vec![d(2026, 6, 10)]),
        ];
        for (op, ops, expected) in cases {
            let c = translate_sql(&format!("deadline:{op}2026-06-10"), d(2026, 6, 15));
            for piece in ops.split(" AND ").flat_map(|p| p.split(" OR ")) {
                let needle = piece.trim();
                assert!(
                    c.sql.contains(needle),
                    "comparator {op}: SQL `{}` missing `{needle}`",
                    c.sql
                );
            }
            assert_eq!(dates(&c), *expected, "comparator {op} params");
        }
    }

    #[test]
    fn timestamp_columns_compare_in_localtime() {
        // created / modified / completed are UTC instants; both the
        // SQL and the in-memory path must compare their LOCAL
        // calendar date, or an evening task shifts a day.
        for field in ["created", "modified", "completed"] {
            let c = translate_sql(&format!("{field}:2026-06-10"), d(2026, 6, 15));
            assert!(
                c.sql.contains(&format!("DATE(t.{field}_at, 'localtime')")),
                "{field}: SQL must convert to localtime: `{}`",
                c.sql
            );
        }
        // Pure date columns stay bare.
        let c = translate_sql("deadline:2026-06-10", d(2026, 6, 15));
        assert!(c.sql.contains("t.deadline"));
        assert!(!c.sql.contains("DATE("));
    }

    #[test]
    fn range_binds_stated_high_day_inclusive() {
        // `field:a..b` spans day a through day b inclusive: the
        // bounds are the stated days themselves. (Both paths took
        // the high bound's exclusive upper edge here, so the day
        // AFTER b matched — a one-day overshoot on each side.)
        let c = translate_sql("deadline:2026-06-01..2026-06-10", d(2026, 6, 15));
        assert!(c.sql.contains(">= ?1"));
        assert!(c.sql.contains("<= ?2"));
        assert_eq!(dates(&c), vec![d(2026, 6, 1), d(2026, 6, 10)]);
    }

    // ── SQL / evaluator parity over a real database ──────────────

    fn parity_conn() -> rusqlite::Connection {
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        db::configure_pragmas(&conn).unwrap();
        crate::db::migrations::migrate(&mut conn).unwrap();
        conn
    }

    fn insert_task(
        conn: &rusqlite::Connection,
        title: &str,
        deadline: Option<NaiveDate>,
        scheduled: Option<&str>,
        created_at: &str,
        completed_at: Option<&str>,
    ) -> i64 {
        conn.execute(
            "INSERT INTO task (uuid, title, deadline, scheduled_for, created_at, modified_at, completed_at, position) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![
                format!("uuid-{title}"),
                title,
                deadline.map(|x| x.to_string()),
                scheduled,
                created_at,
                created_at,
                completed_at,
                1.0
            ],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn sql_ids(conn: &rusqlite::Connection, clause: &SqlClause) -> Vec<i64> {
        let bound: Vec<rusqlite::types::Value> = clause
            .params
            .iter()
            .map(|p| match p {
                SqlValue::Text(s) => rusqlite::types::Value::Text(s.clone()),
                SqlValue::Int(n) => rusqlite::types::Value::Integer(*n),
                SqlValue::Date(x) => rusqlite::types::Value::Text(x.format("%Y-%m-%d").to_string()),
            })
            .collect();
        let mut stmt = conn
            .prepare(&format!("SELECT id FROM task t WHERE {}", clause.sql))
            .unwrap();
        let rows = stmt
            .query_map(rusqlite::params_from_iter(bound), |r| r.get::<_, i64>(0))
            .unwrap();
        let mut ids: Vec<i64> = rows.map(|r| r.unwrap()).collect();
        ids.sort_unstable();
        ids
    }

    fn eval_ids(
        expr: &Expr<Field, State>,
        tasks: &[crate::domain::Task],
        today: NaiveDate,
    ) -> Vec<i64> {
        let tag_names = HashMap::new();
        let project_titles = HashMap::new();
        let project_areas = HashMap::new();
        let area_titles = HashMap::new();
        let ctx = crate::search::EvalContext::new(
            today,
            &tag_names,
            &project_titles,
            &project_areas,
            &area_titles,
        );
        let mut ids: Vec<i64> = tasks
            .iter()
            .filter(|t| crate::search::evaluate(expr, t, &ctx))
            .map(|t| t.id)
            .collect();
        ids.sort_unstable();
        ids
    }

    fn ts(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn sql_and_evaluator_agree_at_range_boundaries() {
        let conn = parity_conn();
        let today = d(2026, 6, 15);
        let lo = d(2026, 6, 10);
        let hi = d(2026, 6, 11);

        // Boundary fixture: both edges of the range, one past each
        // edge, a dateless task, a Someday-sentinel task, and a
        // completed task (completed_at set, deadline still present).
        // created_at mirrors the rows inserted below so the
        // timestamp parity checks compare like with like.
        let created = ts("2026-06-01T12:00:00Z");
        let mut tasks: Vec<crate::domain::Task> = vec![
            crate::domain::Task {
                id: 1,
                deadline: Some(lo),
                ..fixture_task(1)
            },
            crate::domain::Task {
                id: 2,
                deadline: Some(hi),
                ..fixture_task(2)
            },
            crate::domain::Task {
                id: 3,
                deadline: Some(d(2026, 6, 12)),
                ..fixture_task(3)
            },
            crate::domain::Task {
                id: 4,
                ..fixture_task(4)
            },
            crate::domain::Task {
                id: 5,
                scheduled_for: Some(crate::domain::ScheduledFor::Someday),
                ..fixture_task(5)
            },
            crate::domain::Task {
                id: 6,
                completed_at: Some(ts("2026-06-10T22:30:00+00:00")),
                deadline: Some(lo),
                ..fixture_task(6)
            },
        ];
        for t in &mut tasks {
            t.created_at = created;
            t.modified_at = created;
        }
        tasks.sort_by_key(|t| t.id);

        insert_task(&conn, "on-lo", Some(lo), None, "2026-06-01T12:00:00Z", None);
        insert_task(&conn, "on-hi", Some(hi), None, "2026-06-01T12:00:00Z", None);
        insert_task(
            &conn,
            "past-hi",
            Some(d(2026, 6, 12)),
            None,
            "2026-06-01T12:00:00Z",
            None,
        );
        insert_task(&conn, "dateless", None, None, "2026-06-01T12:00:00Z", None);
        insert_task(
            &conn,
            "someday",
            None,
            Some("__someday__"),
            "2026-06-01T12:00:00Z",
            None,
        );
        insert_task(
            &conn,
            "done-on-lo",
            Some(lo),
            None,
            "2026-06-01T12:00:00Z",
            Some("2026-06-10T22:30:00+00:00"),
        );

        // Every comparator on the boundary day: the two paths must
        // return the same id sets, and the known-good expectations
        // pin WHICH side is right (the evaluator's half-open one).
        let expectations: Vec<(&str, Vec<i64>)> = vec![
            ("deadline:2026-06-10", vec![1, 6]),
            ("deadline:=2026-06-10", vec![1, 6]),
            ("deadline:!=2026-06-10", vec![2, 3]),
            ("deadline:<2026-06-10", vec![]),
            ("deadline:<=2026-06-10", vec![1, 6]),
            ("deadline:>2026-06-10", vec![2, 3]),
            ("deadline:>=2026-06-10", vec![1, 2, 3, 6]),
        ];
        for (query, mut expected) in expectations {
            let parsed = crate::search::parse(query);
            let clause = try_translate(&parsed.expr, today).unwrap();
            let via_sql = sql_ids(&conn, &clause);
            let via_eval = eval_ids(&parsed.expr, &tasks, today);
            expected.sort_unstable();
            assert_eq!(via_sql, expected, "SQL wrong for {query}");
            assert_eq!(via_eval, expected, "evaluator wrong for {query}");
        }

        // The Someday sentinel reads as "no date" in both paths.
        for query in ["scheduled:2026-06-10", "scheduled:>2020-01-01"] {
            let parsed = crate::search::parse(query);
            let clause = try_translate(&parsed.expr, today).unwrap();
            assert_eq!(
                sql_ids(&conn, &clause),
                eval_ids(&parsed.expr, &tasks, today),
                "Someday parity for {query}"
            );
        }

        // created/completed instants: parity is the pinned contract
        // (the expected set depends on the machine's local zone, but
        // both paths must move together).
        for query in [
            "created:2026-06-01",
            "created:>2026-06-01",
            "completed:2026-06-10",
            "completed:>=2026-06-10",
        ] {
            let parsed = crate::search::parse(query);
            if let Some(clause) = try_translate(&parsed.expr, today) {
                assert_eq!(
                    sql_ids(&conn, &clause),
                    eval_ids(&parsed.expr, &tasks, today),
                    "timestamp parity for {query}"
                );
            } else {
                panic!("{query} must translate; the fast path silently diverging is the bug");
            }
        }
    }

    fn fixture_task(id: i64) -> crate::domain::Task {
        crate::domain::Task {
            id,
            uuid: format!("u{id}"),
            title: format!("t{id}"),
            note: String::new(),
            project_id: None,
            parent_id: None,
            scheduled_for: None,
            deadline: None,
            defer_until: None,
            estimated_minutes: None,
            completed_at: None,
            repeat_rule: None,
            repeat_mode: None,
            last_reviewed_at: None,
            orig_keyword: None,
            deadline_warn_days: None,
            scheduled_time: None,
            reminder_at: None,
            extra_properties: std::collections::BTreeMap::new(),
            position: id as f64,
            created_at: Utc::now(),
            modified_at: Utc::now(),
        }
    }
}
