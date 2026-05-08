// SPDX-License-Identifier: MIT
//! Search expression AST — Phase 15.5.
//!
//! The AST is what the parser produces and the evaluator consumes.
//! Designed to round-trip: every parsed expression can be re-rendered
//! to the canonical text form via `Display`, and re-parsed without
//! semantic drift.
//!
//! Five kinds of leaf nodes capture the shape of a Calibre-style
//! query:
//!
//! - [`Expr::Text`] — bare freeform text. Substring-matched against
//!   `title` and `note` by the in-memory evaluator; passed to FTS5
//!   when the SQL-translation path can use it.
//! - [`Expr::State`] — `is:open`, `is:done`, `is:overdue`, etc.
//!   State predicates that don't take a value; they read directly
//!   off task fields.
//! - [`Expr::Field`] — `tag:work`, `project:"Q3 plans"`,
//!   `tag:~mystery`. Text/match-shaped operators on a named field.
//! - [`Expr::Compare`] — `due:>today`, `estimated:>=30`. Comparison
//!   operators on date / numeric fields.
//! - [`Expr::Range`] — `due:2026-05-01..2026-05-31`. Inclusive range
//!   on a date field.
//!
//! Plus three composers: [`Expr::Not`], [`Expr::And`], [`Expr::Or`].

use std::fmt;

/// Top-level search expression node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expr {
    /// Bare freeform text token (no `key:` prefix). The empty-string
    /// case is invalid — the parser drops empties.
    Text(String),
    /// `is:X` state predicate — the value is the predicate itself
    /// (e.g. `is:overdue`), not an arbitrary string.
    State(State),
    /// `field:value` with a text-like match modifier.
    Field { field: Field, kind: MatchKind },
    /// `field<comp>value` — `due:>today`, `estimated:>=30`.
    Compare {
        field: Field,
        comp: Comparator,
        value: Value,
    },
    /// `field:lo..hi` (inclusive).
    Range {
        field: Field,
        low: Value,
        high: Value,
    },
    /// `NOT expr` or `!expr`.
    Not(Box<Expr>),
    /// `a AND b AND c …` — n-ary so the parser can collapse implicit
    /// `AND` between bare tokens without nesting.
    And(Vec<Expr>),
    /// `a OR b OR c …` — same shape as And.
    Or(Vec<Expr>),
    /// v0.4.1 — always-true placeholder. The parser emits this for
    /// tokens that don't filter anything but carry metadata (e.g.
    /// `sort:KEY` is captured into `ParseResult.sorts` and the
    /// position in the AST becomes a Pass). And/Or compose Pass
    /// cleanly (it acts as identity).
    Pass,
}

/// v0.4.1 — sort modifier. `sort:KEY` parses to `Asc`; `sort:-KEY`
/// parses to `Desc`. Multiple sorts compose primary → secondary →
/// tertiary in input order, so `sort:-due sort:title` sorts by
/// deadline descending, ties broken alphabetically by title.
///
/// Sort is metadata on the result set, not a per-task predicate, so
/// it lives on [`crate::search::parse::ParseResult`] alongside
/// `expr` rather than inside the `Expr` AST itself. Tasks lacking
/// the sort field (e.g. no deadline for `sort:due`) sort *last*
/// regardless of direction — the SQL convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortSpec {
    pub key: SortKey,
    pub direction: SortDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Due,
    Scheduled,
    Defer,
    Created,
    Modified,
    Completed,
    Estimated,
    Title,
    Position,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

/// Field name. Recognised at parse time; unknown field names parse
/// to a substring match on freeform text rather than raising an error
/// (forward-compat with future field additions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// `tag:` / `tags:` (alias). Match is against the task's tag
    /// names; supports all five MatchKind variants.
    Tag,
    /// `area:` — area title via project.area_id lookup.
    Area,
    /// `project:` — project title.
    Project,
    /// `title:` — task title only (column-scoped FTS5).
    Title,
    /// `note:` — task note only (column-scoped FTS5).
    Note,
    /// `due:` / `deadline:` (alias). Date field.
    Due,
    /// `scheduled:` — date field.
    Scheduled,
    /// `defer:` / `defer_until:` — date field.
    Defer,
    /// `created:` — datetime field; truncated to date for matching.
    Created,
    /// `modified:` — datetime field; same.
    Modified,
    /// `completed:` — datetime field; same.
    Completed,
    /// `estimated:` / `est:` — numeric (minutes).
    Estimated,
    /// `repeats:` — boolean (has a repeat_rule). Only `:true` / `:false`.
    Repeats,
}

/// Match kind on a non-comparison field expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchKind {
    /// Default: case-insensitive substring. `tag:work` matches
    /// `worker`, `homework`. `tag:"two words"` is the same with
    /// quoted spaces.
    Substring(String),
    /// `tag:=foo` / `tag:"=foo bar"` — case-insensitive equality.
    Exact(String),
    /// `tag:~pattern` — regex match. Compiled lazily via the
    /// `regex` crate; SQL-translation path falls back to
    /// in-memory eval when this kind is present.
    Regex(String),
    /// `tag:true` — has at least one matching value.
    HasAny,
    /// `tag:false` — has no values (e.g. zero tags).
    HasNone,
    /// v0.4.1 — `tag:?work` — fuzzy match. A candidate matches when
    /// its case-insensitive Levenshtein distance to the needle is
    /// within the threshold. Threshold scales with needle length:
    /// ≤4 chars → 1, 5–7 → 2, ≥8 → 3 (so "wrok" matches "work" and
    /// "strwbery" matches "strawberry"). In-memory only — SQL
    /// translation falls back to the eval path.
    Fuzzy(String),
}

/// State predicates surfaced as `is:NAME`. Single source of truth for
/// the "is this task in state X" question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Open — completed_at IS NULL.
    Open,
    /// Done — completed_at IS NOT NULL.
    Done,
    /// Overdue — open AND deadline < today.
    Overdue,
    /// Has a scheduled_for date.
    Scheduled,
    /// Has a deadline.
    Deadline,
    /// Has a defer_until in the future.
    Deferred,
    /// Has a repeat_rule.
    Repeating,
    /// Belongs to a project whose archived_at IS NOT NULL.
    Archived,
    /// In the Logbook (synonym for Done).
    Logbook,
    /// Has a project_id.
    InProject,
    /// Belongs (transitively) to an area.
    InArea,
    /// Has at least one tag.
    Tagged,
    /// Sequential project; not the first incomplete task.
    Queued,
    /// Sequential project's first incomplete task, OR not in a
    /// sequential project AND not deferred.
    Available,
    /// v0.4.1 — mirrors the Today list per spec §4.2: open AND
    /// (Schedule ≤ today OR Deadline ≤ today + N) AND defer-resolved.
    /// `N` is `EvalContext::today_deadline_window_days`, default 7
    /// (matches the binary's existing behaviour).
    Today,
    /// v0.4.1 — mirrors the Inbox list: open AND project_id IS NULL.
    Inbox,
    /// v0.4.1 — mirrors the Upcoming list: open AND scheduled_for is
    /// a date strictly in the future.
    Upcoming,
    /// v0.4.1 — mirrors the Anytime list: open AND no scheduled_for
    /// AND defer-resolved.
    Anytime,
    /// v0.4.1 — mirrors the Someday list: open AND scheduled_for ==
    /// Someday sentinel.
    Someday,
}

/// Comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparator {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Value carried by a comparison or a range bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// `2026-05-15` literal.
    Date(chrono::NaiveDate),
    /// `today`, `yesterday`, etc. — resolved at eval time against
    /// the EvalContext's `today`.
    DateKeyword(DateKeyword),
    /// Integer literal — used by `estimated:`.
    Number(i64),
    /// Bare text where neither a date nor a number parses.
    Text(String),
}

/// Calibre-style date keyword. Resolved to a concrete date or range
/// at eval time. The `daysago` / `daysout` cases carry their N inline
/// (`5daysago` → `DaysAgo(5)`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateKeyword {
    Today,
    Yesterday,
    Tomorrow,
    ThisWeek,
    LastWeek,
    NextWeek,
    ThisMonth,
    LastMonth,
    NextMonth,
    ThisYear,
    DaysAgo(u32),
    DaysOut(u32),
}

// ── Display impls (round-trip) ───────────────────────────────────

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Tag => "tag",
            Self::Area => "area",
            Self::Project => "project",
            Self::Title => "title",
            Self::Note => "note",
            Self::Due => "due",
            Self::Scheduled => "scheduled",
            Self::Defer => "defer",
            Self::Created => "created",
            Self::Modified => "modified",
            Self::Completed => "completed",
            Self::Estimated => "estimated",
            Self::Repeats => "repeats",
        };
        f.write_str(s)
    }
}

impl fmt::Display for Comparator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Eq => "=",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Le => "<=",
            Self::Gt => ">",
            Self::Ge => ">=",
        };
        f.write_str(s)
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Open => "open",
            Self::Done => "done",
            Self::Overdue => "overdue",
            Self::Scheduled => "scheduled",
            Self::Deadline => "deadline",
            Self::Deferred => "deferred",
            Self::Repeating => "repeating",
            Self::Archived => "archived",
            Self::Logbook => "logbook",
            Self::InProject => "project",
            Self::InArea => "area",
            Self::Tagged => "tagged",
            Self::Queued => "queued",
            Self::Available => "available",
            Self::Today => "today",
            Self::Inbox => "inbox",
            Self::Upcoming => "upcoming",
            Self::Anytime => "anytime",
            Self::Someday => "someday",
        };
        write!(f, "is:{s}")
    }
}

impl fmt::Display for DateKeyword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Today => f.write_str("today"),
            Self::Yesterday => f.write_str("yesterday"),
            Self::Tomorrow => f.write_str("tomorrow"),
            Self::ThisWeek => f.write_str("thisweek"),
            Self::LastWeek => f.write_str("lastweek"),
            Self::NextWeek => f.write_str("nextweek"),
            Self::ThisMonth => f.write_str("thismonth"),
            Self::LastMonth => f.write_str("lastmonth"),
            Self::NextMonth => f.write_str("nextmonth"),
            Self::ThisYear => f.write_str("thisyear"),
            Self::DaysAgo(n) => write!(f, "{n}daysago"),
            Self::DaysOut(n) => write!(f, "{n}daysout"),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Date(d) => write!(f, "{}", d.format("%Y-%m-%d")),
            Self::DateKeyword(k) => write!(f, "{k}"),
            Self::Number(n) => write!(f, "{n}"),
            Self::Text(s) => f.write_str(s),
        }
    }
}

impl fmt::Display for MatchKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Substring(s) => write_quoted_if_needed(f, s, false),
            Self::Exact(s) => {
                f.write_str("=")?;
                write_quoted_if_needed(f, s, true)
            }
            Self::Regex(s) => write!(f, "~{s}"),
            Self::HasAny => f.write_str("true"),
            Self::HasNone => f.write_str("false"),
            Self::Fuzzy(s) => write!(f, "?{s}"),
        }
    }
}

/// Quote a value when it contains spaces or special characters.
/// `with_eq_inside` puts the `=` inside the quotes — Calibre's
/// `tag:"=foo bar"` form for exact matches with spaces.
fn write_quoted_if_needed(f: &mut fmt::Formatter<'_>, s: &str, eq_inside: bool) -> fmt::Result {
    let needs_quotes = s
        .chars()
        .any(|c| c.is_whitespace() || c == '"' || c == '(' || c == ')');
    if needs_quotes {
        f.write_str("\"")?;
        if eq_inside {
            f.write_str("=")?;
        }
        for c in s.chars() {
            match c {
                '"' => f.write_str("\\\"")?,
                '\\' => f.write_str("\\\\")?,
                _ => write!(f, "{c}")?,
            }
        }
        f.write_str("\"")
    } else {
        // The `=` was already written outside in the Exact arm; we
        // just emit the bare value here.
        let _ = eq_inside;
        f.write_str(s)
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Text(s) => write_quoted_if_needed(f, s, false),
            Self::State(s) => write!(f, "{s}"),
            Self::Field { field, kind } => write!(f, "{field}:{kind}"),
            Self::Compare { field, comp, value } => write!(f, "{field}:{comp}{value}"),
            Self::Range { field, low, high } => write!(f, "{field}:{low}..{high}"),
            Self::Not(inner) => write!(f, "NOT {inner}"),
            Self::And(items) => {
                let mut first = true;
                for item in items {
                    // Pass nodes contribute nothing to the rendered
                    // text; they were sort-modifier placeholders the
                    // parser already lifted into ParseResult.sorts.
                    if matches!(item, Expr::Pass) {
                        continue;
                    }
                    if !first {
                        f.write_str(" AND ")?;
                    }
                    first = false;
                    fmt_with_parens(f, item, BindingPower::And)?;
                }
                Ok(())
            }
            Self::Or(items) => {
                let mut first = true;
                for item in items {
                    if matches!(item, Expr::Pass) {
                        continue;
                    }
                    if !first {
                        f.write_str(" OR ")?;
                    }
                    first = false;
                    fmt_with_parens(f, item, BindingPower::Or)?;
                }
                Ok(())
            }
            Self::Pass => Ok(()),
        }
    }
}

impl fmt::Display for SortKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Due => "due",
            Self::Scheduled => "scheduled",
            Self::Defer => "defer",
            Self::Created => "created",
            Self::Modified => "modified",
            Self::Completed => "completed",
            Self::Estimated => "estimated",
            Self::Title => "title",
            Self::Position => "position",
        };
        f.write_str(s)
    }
}

impl fmt::Display for SortDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Asc => Ok(()), // implicit
            Self::Desc => f.write_str("-"),
        }
    }
}

impl fmt::Display for SortSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "sort:{}{}", self.direction, self.key)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum BindingPower {
    Or = 1,
    And = 2,
}

fn fmt_with_parens(f: &mut fmt::Formatter<'_>, expr: &Expr, parent: BindingPower) -> fmt::Result {
    let needs_parens = match expr {
        Expr::Or(_) if parent >= BindingPower::Or => true,
        Expr::And(_) if parent >= BindingPower::And => false,
        _ => false,
    };
    if needs_parens {
        write!(f, "({expr})")
    } else {
        write!(f, "{expr}")
    }
}
