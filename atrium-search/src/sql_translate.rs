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

use crate::ast::{Comparator, Expr, Field, MatchKind, State, Value};
use crate::dates::value_to_range;

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
/// the `From<SqlValue> for atrium_core::SqlBindValue` impl below
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

impl From<SqlValue> for atrium_core::SqlBindValue {
    fn from(value: SqlValue) -> Self {
        match value {
            SqlValue::Text(s) => atrium_core::SqlBindValue::Text(s),
            SqlValue::Int(n) => atrium_core::SqlBindValue::Int(n),
            SqlValue::Date(d) => atrium_core::SqlBindValue::Date(d),
        }
    }
}

impl From<&SqlValue> for atrium_core::SqlBindValue {
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
pub fn try_translate(expr: &Expr, today: NaiveDate) -> Option<SqlClause> {
    let mut params = Vec::new();
    let sql = translate(expr, today, &mut params)?;
    Some(SqlClause { sql, params })
}

fn translate(expr: &Expr, today: NaiveDate, params: &mut Vec<SqlValue>) -> Option<String> {
    match expr {
        Expr::Pass => Some("1".into()),
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
    items: &[Expr],
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
                Value::Number(n) => *n,
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
            let (lo, hi) = value_to_range(value, today);
            // Date keywords like `thisweek` produce a range; the
            // comparator semantics are the same as the in-memory
            // path (see `dates::compare_date`). For a range-valued
            // RHS, `Eq` means "in the range", `Ne` means "outside",
            // etc. Single-day RHS collapses lo == hi so all of
            // these reduce to the obvious comparison. We push only
            // the params actually referenced in the SQL — binding
            // an unused param to `params_from_iter` errors at run
            // time, so the eq/ne paths bind two and the others
            // bind one.
            Some(match comp {
                Comparator::Eq => {
                    params.push(SqlValue::Date(lo));
                    let lo_ph = placeholder(params.len());
                    params.push(SqlValue::Date(hi));
                    let hi_ph = placeholder(params.len());
                    format!(
                        "({column} IS NOT NULL AND {column} >= {lo_ph} AND {column} <= {hi_ph})"
                    )
                }
                Comparator::Ne => {
                    params.push(SqlValue::Date(lo));
                    let lo_ph = placeholder(params.len());
                    params.push(SqlValue::Date(hi));
                    let hi_ph = placeholder(params.len());
                    format!("({column} IS NULL OR {column} < {lo_ph} OR {column} > {hi_ph})")
                }
                Comparator::Lt => {
                    params.push(SqlValue::Date(lo));
                    let lo_ph = placeholder(params.len());
                    format!("({column} IS NOT NULL AND {column} < {lo_ph})")
                }
                Comparator::Le => {
                    params.push(SqlValue::Date(hi));
                    let hi_ph = placeholder(params.len());
                    format!("({column} IS NOT NULL AND {column} <= {hi_ph})")
                }
                Comparator::Gt => {
                    params.push(SqlValue::Date(hi));
                    let hi_ph = placeholder(params.len());
                    format!("({column} IS NOT NULL AND {column} > {hi_ph})")
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
    let (low_lo, _) = value_to_range(low, today);
    let (_, high_hi) = value_to_range(high, today);
    params.push(SqlValue::Date(low_lo));
    let lo_ph = placeholder(params.len());
    params.push(SqlValue::Date(high_hi));
    let hi_ph = placeholder(params.len());
    Some(format!(
        "({column} IS NOT NULL AND {column} >= {lo_ph} AND {column} <= {hi_ph})"
    ))
}

fn date_column(field: Field) -> Option<&'static str> {
    Some(match field {
        Field::Due => "t.deadline",
        Field::Scheduled => "t.scheduled_for",
        Field::Defer => "t.defer_until",
        // The created/modified/completed columns store a full
        // RFC3339 timestamp; we compare the date prefix so semantics
        // align with the in-memory evaluator (which truncates to
        // date when comparing). SQLite's text-prefix `>=`/`<=` works
        // because RFC3339 sorts lexicographically.
        Field::Created => "DATE(t.created_at)",
        Field::Modified => "DATE(t.modified_at)",
        Field::Completed => "DATE(t.completed_at)",
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
    use crate::ast::{DateKeyword, Expr, Field, MatchKind, State, Value};
    use chrono::NaiveDate;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 5, 15).unwrap()
    }

    fn translate_to_string(expr: Expr) -> Option<String> {
        try_translate(&expr, today()).map(|c| c.sql)
    }

    // ── boolean composition ────────────────────────────────

    #[test]
    fn pass_translates_to_identity() {
        assert_eq!(translate_to_string(Expr::Pass).as_deref(), Some("1"));
    }

    #[test]
    fn and_combines_subexpressions() {
        let expr = Expr::And(vec![Expr::State(State::Open), Expr::State(State::Deadline)]);
        let sql = translate_to_string(expr).unwrap();
        assert_eq!(sql, "(t.completed_at IS NULL AND t.deadline IS NOT NULL)");
    }

    #[test]
    fn or_combines_subexpressions() {
        let expr = Expr::Or(vec![Expr::State(State::Open), Expr::State(State::Done)]);
        let sql = translate_to_string(expr).unwrap();
        assert_eq!(
            sql,
            "(t.completed_at IS NULL OR t.completed_at IS NOT NULL)"
        );
    }

    #[test]
    fn not_wraps_subexpression() {
        let expr = Expr::Not(Box::new(Expr::State(State::Open)));
        let sql = translate_to_string(expr).unwrap();
        assert_eq!(sql, "(NOT t.completed_at IS NULL)");
    }

    // ── bare text ─────────────────────────────────────────

    #[test]
    fn bare_text_substring_on_title_and_note() {
        let clause = try_translate(&Expr::Text("milk".into()), today()).unwrap();
        assert_eq!(
            clause.sql,
            "(LOWER(t.title) LIKE ?1 ESCAPE '\\' OR LOWER(t.note) LIKE ?1 ESCAPE '\\')"
        );
        assert_eq!(clause.params, vec![SqlValue::Text("%milk%".into())]);
    }

    #[test]
    fn bare_text_escapes_like_wildcards() {
        let clause = try_translate(&Expr::Text("100%".into()), today()).unwrap();
        assert_eq!(clause.params, vec![SqlValue::Text("%100\\%%".into())]);
    }

    // ── state predicates ──────────────────────────────────

    #[test]
    fn state_open_translates() {
        let sql = translate_to_string(Expr::State(State::Open)).unwrap();
        assert_eq!(sql, "t.completed_at IS NULL");
    }

    #[test]
    fn state_overdue_binds_today() {
        let clause = try_translate(&Expr::State(State::Overdue), today()).unwrap();
        assert_eq!(
            clause.sql,
            "(t.completed_at IS NULL AND t.deadline IS NOT NULL AND t.deadline < ?1)"
        );
        assert_eq!(clause.params, vec![SqlValue::Date(today())]);
    }

    #[test]
    fn state_today_falls_back_to_in_memory() {
        // Composite list-membership predicates are deferred — the
        // translator must return None so the in-memory eval handles
        // them.
        assert!(try_translate(&Expr::State(State::Today), today()).is_none());
    }

    #[test]
    fn state_available_and_blocked_translate() {
        // v0.29.0 — both translate to an EXISTS / NOT EXISTS subquery
        // over task_dependency, so the dependency filter runs in SQL
        // rather than falling back to the in-memory evaluator.
        let avail = try_translate(&Expr::State(State::Available), today()).unwrap();
        assert!(avail.sql.contains("NOT EXISTS"));
        assert!(avail.sql.contains("task_dependency"));
        assert!(avail.params.is_empty());

        let blocked = try_translate(&Expr::State(State::Blocked), today()).unwrap();
        assert!(blocked.sql.contains("EXISTS"));
        assert!(!blocked.sql.contains("NOT EXISTS"));
        assert!(blocked.sql.contains("task_dependency"));
        assert!(blocked.params.is_empty());
    }

    #[test]
    fn state_queued_falls_back() {
        // Sequential "queued" state is still not exposed via SQL.
        assert!(try_translate(&Expr::State(State::Queued), today()).is_none());
    }

    #[test]
    fn state_in_area_falls_back() {
        assert!(try_translate(&Expr::State(State::InArea), today()).is_none());
    }

    // ── field-scoped matches ──────────────────────────────

    #[test]
    fn title_substring_lowercases_pattern() {
        let clause = try_translate(
            &Expr::Field {
                field: Field::Title,
                kind: MatchKind::Substring("Milk".into()),
            },
            today(),
        )
        .unwrap();
        assert_eq!(clause.sql, "LOWER(t.title) LIKE ?1 ESCAPE '\\'");
        assert_eq!(clause.params, vec![SqlValue::Text("%milk%".into())]);
    }

    #[test]
    fn tag_substring_uses_exists_subquery() {
        let clause = try_translate(
            &Expr::Field {
                field: Field::Tag,
                kind: MatchKind::Substring("work".into()),
            },
            today(),
        )
        .unwrap();
        assert!(clause.sql.contains("EXISTS"));
        assert!(clause.sql.contains("task_tag tt"));
        assert!(clause.sql.contains("LOWER(g.name) LIKE"));
        assert_eq!(clause.params, vec![SqlValue::Text("%work%".into())]);
    }

    #[test]
    fn tag_regex_falls_back() {
        let r = try_translate(
            &Expr::Field {
                field: Field::Tag,
                kind: MatchKind::Regex(".*work.*".into()),
            },
            today(),
        );
        assert!(r.is_none());
    }

    #[test]
    fn tag_fuzzy_falls_back() {
        let r = try_translate(
            &Expr::Field {
                field: Field::Tag,
                kind: MatchKind::Fuzzy("wrok".into()),
            },
            today(),
        );
        assert!(r.is_none());
    }

    #[test]
    fn project_substring_falls_back_for_v1() {
        // Deferred — would need a JOIN through `project`. Today
        // returns None so the in-memory eval handles it.
        let r = try_translate(
            &Expr::Field {
                field: Field::Project,
                kind: MatchKind::Substring("Q3".into()),
            },
            today(),
        );
        assert!(r.is_none());
    }

    // ── compare / range ───────────────────────────────────

    #[test]
    fn compare_due_equals_today_uses_range() {
        let clause = try_translate(
            &Expr::Compare {
                field: Field::Due,
                comp: Comparator::Eq,
                value: Value::DateKeyword(DateKeyword::Today),
            },
            today(),
        )
        .unwrap();
        assert!(clause.sql.contains("t.deadline IS NOT NULL"));
        assert!(clause.sql.contains("t.deadline >= ?1"));
        assert!(clause.sql.contains("t.deadline <= ?2"));
        assert_eq!(
            clause.params,
            vec![SqlValue::Date(today()), SqlValue::Date(today())]
        );
    }

    #[test]
    fn compare_due_thisweek_expands_to_range() {
        let clause = try_translate(
            &Expr::Compare {
                field: Field::Due,
                comp: Comparator::Eq,
                value: Value::DateKeyword(DateKeyword::ThisWeek),
            },
            today(),
        )
        .unwrap();
        // 2026-05-15 is a Friday; this-week is 2026-05-11 (Mon) ..
        // 2026-05-17 (Sun).
        assert_eq!(
            clause.params,
            vec![
                SqlValue::Date(NaiveDate::from_ymd_opt(2026, 5, 11).unwrap()),
                SqlValue::Date(NaiveDate::from_ymd_opt(2026, 5, 17).unwrap()),
            ]
        );
    }

    #[test]
    fn compare_estimated_lt_30() {
        let clause = try_translate(
            &Expr::Compare {
                field: Field::Estimated,
                comp: Comparator::Lt,
                value: Value::Number(30),
            },
            today(),
        )
        .unwrap();
        assert_eq!(
            clause.sql,
            "(t.estimated_minutes IS NOT NULL AND t.estimated_minutes < ?1)"
        );
        assert_eq!(clause.params, vec![SqlValue::Int(30)]);
    }

    #[test]
    fn range_due_inclusive() {
        let clause = try_translate(
            &Expr::Range {
                field: Field::Due,
                low: Value::Date(NaiveDate::from_ymd_opt(2026, 5, 1).unwrap()),
                high: Value::Date(NaiveDate::from_ymd_opt(2026, 5, 31).unwrap()),
            },
            today(),
        )
        .unwrap();
        assert!(clause.sql.contains("t.deadline >= ?1"));
        assert!(clause.sql.contains("t.deadline <= ?2"));
    }

    // ── compound — placeholder numbers stay in lockstep ───

    #[test]
    fn placeholders_renumber_across_subexpressions() {
        let expr = Expr::And(vec![
            Expr::Text("foo".into()),
            Expr::Field {
                field: Field::Tag,
                kind: MatchKind::Substring("bar".into()),
            },
        ]);
        let clause = try_translate(&expr, today()).unwrap();
        assert!(clause.sql.contains("?1"));
        assert!(clause.sql.contains("?2"));
        // Three params: text uses ?1 twice (the OR shape), tag uses ?3?
        // No — text's "?1" appears twice in the SQL but binds once.
        // params length is the count of distinct bound values, not
        // placeholders. text=1, tag=1 → 2.
        // Wait — text uses ?1 in two places (title + note); we still
        // bind once. tag pushes one param. So len = 2.
        assert_eq!(clause.params.len(), 2);
    }
}
