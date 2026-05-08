// SPDX-License-Identifier: MIT
//! In-memory evaluator. Runs an [`Expr`] against a single [`Task`]
//! and returns whether the task matches.
//!
//! The evaluator is the pure-Rust path that handles every operator
//! the grammar exposes, including the SQL-incompatible ones (regex,
//! tag-set predicates). The SQL-translation evaluator (in `sql.rs`,
//! Phase 15.5 stage 3) handles the subset SQLite can express, falls
//! back to in-memory when it can't.
//!
//! The eval loop traverses the expression once, short-circuits AND
//! and OR. Regex compilation is lazy and cached per-call via a small
//! HashMap keyed on the pattern string (a search bar refresh might
//! evaluate the same regex against a thousand tasks; compiling once
//! per query rather than once per task matters).

use std::collections::HashMap;

use chrono::{Duration, NaiveDate};
use regex::Regex;

use atrium_core::domain::{ScheduledFor, Task};

use super::ast::{Comparator, Expr, Field, MatchKind, State, Value};
use super::dates::{compare_date, value_to_range};

/// Read-only context the evaluator needs to resolve fields like
/// `area:` and tag matches. Built once per query in the window-side
/// caller.
pub struct EvalContext<'a> {
    pub today: NaiveDate,
    pub tag_names: &'a HashMap<i64, Vec<String>>,
    pub project_titles: &'a HashMap<i64, String>,
    pub project_areas: &'a HashMap<i64, Option<i64>>,
    pub area_titles: &'a HashMap<i64, String>,
    /// Cache of compiled regexes — populated lazily as the evaluator
    /// encounters `MatchKind::Regex` nodes. Same query against many
    /// tasks reuses the compiled Regex.
    regex_cache: std::cell::RefCell<HashMap<String, Option<Regex>>>,
}

impl<'a> EvalContext<'a> {
    pub fn new(
        today: NaiveDate,
        tag_names: &'a HashMap<i64, Vec<String>>,
        project_titles: &'a HashMap<i64, String>,
        project_areas: &'a HashMap<i64, Option<i64>>,
        area_titles: &'a HashMap<i64, String>,
    ) -> Self {
        Self {
            today,
            tag_names,
            project_titles,
            project_areas,
            area_titles,
            regex_cache: std::cell::RefCell::new(HashMap::new()),
        }
    }

    /// Compile and cache a regex. Returns `None` if the pattern is
    /// malformed; the evaluator treats that as "no task matches" so
    /// a typo'd regex doesn't crash the query.
    fn compile_regex(&self, pattern: &str) -> bool {
        let cache = self.regex_cache.borrow();
        if cache.contains_key(pattern) {
            return cache[pattern].is_some();
        }
        drop(cache);
        let compiled = Regex::new(&format!("(?i){pattern}")).ok();
        let ok = compiled.is_some();
        self.regex_cache
            .borrow_mut()
            .insert(pattern.to_string(), compiled);
        ok
    }

    fn regex_match(&self, pattern: &str, haystack: &str) -> bool {
        if !self.compile_regex(pattern) {
            return false;
        }
        let cache = self.regex_cache.borrow();
        cache[pattern]
            .as_ref()
            .map(|r| r.is_match(haystack))
            .unwrap_or(false)
    }
}

/// Evaluate an expression against a single task. Returns `true` when
/// the task matches.
pub fn evaluate(expr: &Expr, task: &Task, ctx: &EvalContext<'_>) -> bool {
    match expr {
        Expr::Text(s) => match_text(task, s),
        Expr::State(state) => match_state(task, *state, ctx),
        Expr::Field { field, kind } => match_field(task, *field, kind, ctx),
        Expr::Compare { field, comp, value } => match_compare(task, *field, *comp, value, ctx),
        Expr::Range { field, low, high } => match_range(task, *field, low, high, ctx),
        Expr::Not(inner) => !evaluate(inner, task, ctx),
        Expr::And(items) => items.iter().all(|e| evaluate(e, task, ctx)),
        Expr::Or(items) => items.iter().any(|e| evaluate(e, task, ctx)),
        // v0.4.1 — Pass is the parser's placeholder for tokens that
        // don't filter (e.g., a sort modifier). Always-true makes it
        // act as identity in And/Or composition.
        Expr::Pass => true,
    }
}

/// Bare-text match: case-insensitive substring on title + note.
fn match_text(task: &Task, needle: &str) -> bool {
    let n = needle.to_ascii_lowercase();
    task.title.to_ascii_lowercase().contains(&n) || task.note.to_ascii_lowercase().contains(&n)
}

fn match_state(task: &Task, state: State, ctx: &EvalContext<'_>) -> bool {
    let today = ctx.today;
    match state {
        State::Open => task.completed_at.is_none(),
        State::Done | State::Logbook => task.completed_at.is_some(),
        State::Overdue => task.completed_at.is_none() && task.deadline.is_some_and(|d| d < today),
        State::Scheduled => task.scheduled_for.is_some(),
        State::Deadline => task.deadline.is_some(),
        State::Deferred => task.defer_until.is_some_and(|d| d > today),
        State::Repeating => task.repeat_rule.is_some(),
        State::Archived => false, // resolved via project-side cache; not a Task field
        State::InProject => task.project_id.is_some(),
        State::InArea => task
            .project_id
            .and_then(|pid| ctx.project_areas.get(&pid).copied().flatten())
            .is_some(),
        State::Tagged => ctx.tag_names.get(&task.id).is_some_and(|v| !v.is_empty()),
        State::Queued | State::Available => false, // sequential-project state, not a task field
        // v0.4.1 — canonical-list mirrors. Each must agree with the
        // corresponding read fn in `db::read` so the search predicate
        // and the sidebar list select the same set. Spec §4.2 is the
        // contract.
        State::Today => is_in_today_list(task, today),
        State::Inbox => task.completed_at.is_none() && task.project_id.is_none(),
        State::Upcoming => task.completed_at.is_none() && is_scheduled_strictly_future(task, today),
        State::Anytime => {
            task.completed_at.is_none()
                && task.scheduled_for.is_none()
                && !is_deferred_to_future(task, today)
        }
        State::Someday => {
            task.completed_at.is_none() && matches!(task.scheduled_for, Some(ScheduledFor::Someday))
        }
    }
}

/// Mirror of `db::read::list_today` membership: open AND
/// (Schedule ≤ today OR Deadline ≤ today + heads-up window) AND
/// defer-resolved. The window matches `read::TODAY_DEADLINE_WINDOW_DAYS`
/// (7 days, locked at v0.1; spec §4.2 notes a future Phase 8d
/// preferences task to make it user-configurable).
fn is_in_today_list(task: &Task, today: NaiveDate) -> bool {
    if task.completed_at.is_some() {
        return false;
    }
    if is_deferred_to_future(task, today) {
        return false;
    }
    let scheduled_today_or_past = match &task.scheduled_for {
        Some(ScheduledFor::Date(d)) => *d <= today,
        Some(ScheduledFor::Someday) => false,
        None => false,
    };
    let horizon = today + Duration::days(TODAY_DEADLINE_WINDOW_DAYS);
    let deadline_approaching = task.deadline.is_some_and(|d| d <= horizon);
    scheduled_today_or_past || deadline_approaching
}

fn is_scheduled_strictly_future(task: &Task, today: NaiveDate) -> bool {
    matches!(&task.scheduled_for, Some(ScheduledFor::Date(d)) if *d > today)
}

fn is_deferred_to_future(task: &Task, today: NaiveDate) -> bool {
    task.defer_until.is_some_and(|d| d > today)
}

/// Today list's deadline heads-up window, in days. Must agree with
/// `db::read::TODAY_DEADLINE_WINDOW_DAYS` — both are the v0.1
/// frozen constant per spec §4.2. Duplicated here rather than imported
/// to keep the search module's dep graph independent of `db::`,
/// which matters for the v0.4.2 atrium-search extraction.
const TODAY_DEADLINE_WINDOW_DAYS: i64 = 7;

fn match_field(task: &Task, field: Field, kind: &MatchKind, ctx: &EvalContext<'_>) -> bool {
    let candidates: Vec<String> = collect_field_values(task, field, ctx);
    match kind {
        MatchKind::Substring(needle) => {
            let n = needle.to_ascii_lowercase();
            candidates
                .iter()
                .any(|v| v.to_ascii_lowercase().contains(&n))
        }
        MatchKind::Exact(needle) => {
            let n = needle.to_ascii_lowercase();
            candidates.iter().any(|v| v.to_ascii_lowercase() == n)
        }
        MatchKind::Regex(pattern) => candidates.iter().any(|v| ctx.regex_match(pattern, v)),
        MatchKind::HasAny => !candidates.iter().all(String::is_empty) && !candidates.is_empty(),
        MatchKind::HasNone => candidates.iter().all(String::is_empty) || candidates.is_empty(),
        MatchKind::Fuzzy(needle) => {
            let n_lower = needle.to_ascii_lowercase();
            let threshold = fuzzy_threshold_for(n_lower.chars().count());
            candidates.iter().any(|v| {
                let v_lower = v.to_ascii_lowercase();
                levenshtein_within(&n_lower, &v_lower, threshold)
            })
        }
    }
}

/// Length-aware fuzzy threshold. Short queries get tight matching
/// (one typo); longer ones tolerate proportionally more so multi-
/// character words like `strawberry` can survive a missed letter
/// or two without throwing the user back to substring.
fn fuzzy_threshold_for(needle_len: usize) -> u32 {
    match needle_len {
        0..=4 => 1,
        5..=7 => 2,
        _ => 3,
    }
}

/// Compute whether the Damerau-Levenshtein distance between `a` and
/// `b` is at most `max`. Damerau (vs plain Levenshtein) counts a
/// transposition of two adjacent characters as a single edit — the
/// most common typing slip ("wrok" ↔ "work"), which the user
/// expects fuzzy match to handle. Early-exits as soon as the running
/// minimum across a row exceeds `max`. Both strings are expected to
/// be lowercased by the caller.
fn levenshtein_within(a: &str, b: &str, max: u32) -> bool {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    // Length difference alone exceeds the budget — short-circuit.
    let diff = a_chars.len().abs_diff(b_chars.len()) as u32;
    if diff > max {
        return false;
    }
    if a_chars.is_empty() {
        return (b_chars.len() as u32) <= max;
    }
    if b_chars.is_empty() {
        return (a_chars.len() as u32) <= max;
    }
    let n = a_chars.len();
    let m = b_chars.len();
    // Three rows for Damerau: prev_prev (i-2), prev (i-1), curr (i).
    // Damerau's transposition rule needs the row two before to look
    // up cost `prev_prev[j-2] + 1`.
    let mut prev_prev: Vec<u32> = vec![0; m + 1];
    let mut prev: Vec<u32> = (0..=m as u32).collect();
    let mut curr: Vec<u32> = vec![0; m + 1];
    for i in 1..=n {
        curr[0] = i as u32;
        let mut row_min = curr[0];
        for j in 1..=m {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            let mut v = (curr[j - 1] + 1).min(prev[j] + 1).min(prev[j - 1] + cost);
            // Damerau transposition: a[i-1]a[i-2] swap to b[j-2]b[j-1].
            if i >= 2
                && j >= 2
                && a_chars[i - 1] == b_chars[j - 2]
                && a_chars[i - 2] == b_chars[j - 1]
            {
                v = v.min(prev_prev[j - 2] + 1);
            }
            curr[j] = v;
            row_min = row_min.min(v);
        }
        // If every cell on this row is already past the budget,
        // the final answer can only grow from here.
        if row_min > max {
            return false;
        }
        // Rotate rows: prev_prev ← prev, prev ← curr, curr scratch.
        std::mem::swap(&mut prev_prev, &mut prev);
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m] <= max
}

/// Collect the string-shaped values for a field on a single task.
/// Returns an empty Vec when the field has no value (e.g. `tag:` on
/// a task with no tags). Numeric / date fields return an empty Vec
/// here — those flow through `match_compare` instead.
fn collect_field_values(task: &Task, field: Field, ctx: &EvalContext<'_>) -> Vec<String> {
    match field {
        Field::Tag => ctx.tag_names.get(&task.id).cloned().unwrap_or_default(),
        Field::Project => task
            .project_id
            .and_then(|pid| ctx.project_titles.get(&pid).cloned())
            .into_iter()
            .collect(),
        Field::Area => task
            .project_id
            .and_then(|pid| ctx.project_areas.get(&pid).copied().flatten())
            .and_then(|aid| ctx.area_titles.get(&aid).cloned())
            .into_iter()
            .collect(),
        Field::Title => vec![task.title.clone()],
        Field::Note => vec![task.note.clone()],
        Field::Repeats => match task.repeat_rule.as_ref() {
            Some(rule) => vec![rule.clone()],
            None => Vec::new(),
        },
        Field::Due
        | Field::Scheduled
        | Field::Defer
        | Field::Created
        | Field::Modified
        | Field::Completed
        | Field::Estimated => Vec::new(),
    }
}

fn match_compare(
    task: &Task,
    field: Field,
    comp: Comparator,
    value: &Value,
    ctx: &EvalContext<'_>,
) -> bool {
    if let Some(d) = field_date_value(task, field) {
        let (lo, hi) = value_to_range(value, ctx.today);
        return compare_date(d, lo, hi, comp);
    }
    if let Some(n) = field_numeric_value(task, field)
        && let Value::Number(target) = value
    {
        return compare_number(n, *target, comp);
    }
    false
}

fn match_range(
    task: &Task,
    field: Field,
    low: &Value,
    high: &Value,
    ctx: &EvalContext<'_>,
) -> bool {
    let Some(d) = field_date_value(task, field) else {
        return false;
    };
    let (low_lo, _) = value_to_range(low, ctx.today);
    let (_, high_hi) = value_to_range(high, ctx.today);
    d >= low_lo && d <= high_hi
}

/// Pull a date out of the task for date-shaped fields.
fn field_date_value(task: &Task, field: Field) -> Option<NaiveDate> {
    match field {
        Field::Due => task.deadline,
        Field::Scheduled => match &task.scheduled_for {
            Some(ScheduledFor::Date(d)) => Some(*d),
            _ => None,
        },
        Field::Defer => task.defer_until,
        Field::Created => Some(task.created_at.date_naive()),
        Field::Modified => Some(task.modified_at.date_naive()),
        Field::Completed => task.completed_at.map(|dt| dt.date_naive()),
        _ => None,
    }
}

fn field_numeric_value(task: &Task, field: Field) -> Option<i64> {
    match field {
        Field::Estimated => task.estimated_minutes,
        _ => None,
    }
}

// Date-range helpers (`keyword_to_range`, `week_bounds`,
// `month_bounds`, `compare_date`, `value_to_range`) live in
// `super::dates` so the SQL translator can produce the same range
// arithmetic without duplicating the rules. See `dates.rs`.

fn compare_number(n: i64, target: i64, comp: Comparator) -> bool {
    match comp {
        Comparator::Eq => n == target,
        Comparator::Ne => n != target,
        Comparator::Lt => n < target,
        Comparator::Le => n <= target,
        Comparator::Gt => n > target,
        Comparator::Ge => n >= target,
    }
}
