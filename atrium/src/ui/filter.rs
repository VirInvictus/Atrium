// SPDX-License-Identifier: MIT
//! Search expression entry point for the binary — Phase 15.5 shim.
//!
//! The Phase 7d filter parser (flat `tag:foo is:overdue` shape) was
//! superseded in v0.4.0 by a real recursive-descent parser with
//! Calibre-style match modifiers, boolean composition, ranges, and
//! date keywords; v0.4.2 lifted that engine into its own
//! `atrium_search` crate so it can be exercised independently. This
//! module is the window-side glue between the engine and Atrium's
//! caches:
//!
//! - [`parse`] — wraps [`atrium_search::parse`] and surfaces
//!   non-fatal warnings (unknown field names that fall through to
//!   freeform text).
//! - [`apply`] — runs the parsed expression against a task list,
//!   building an `EvalContext` from the window's existing caches,
//!   plus applies any `sort:KEY` modifiers the parser captured.
//!
//! The window's call sites keep their old shape: `parse(query)`
//! returns a [`FilterQuery`] carrying the AST + warnings + sorts;
//! `apply` filters and (if sorts are present) orders a task vector.
//! Saved Perspectives go through the same path, so expressions
//! written against any v0.4.x grammar evaluate identically.

use std::cmp::Ordering;
use std::collections::HashMap;

use atrium_core::ScheduledFor;
use atrium_core::Task;
use atrium_search::{EvalContext, Expr, SortDirection, SortKey, SortSpec, evaluate};
use chrono::NaiveDate;

/// Output of [`parse`]. The window uses `expr.is_some()` as "the
/// query is non-empty"; uses `warnings` to surface a toast.
#[derive(Debug, Clone, Default)]
pub struct FilterQuery {
    /// Parsed expression. `None` when the input was empty or
    /// fundamentally unparseable.
    pub expr: Option<Expr>,
    /// Warnings collected during parse — unknown field names,
    /// unknown state predicates. Surfaced as toast in the search bar.
    pub warnings: Vec<String>,
    /// v0.4.1 — explicit `sort:KEY` / `sort:-KEY` modifiers in input
    /// order (primary → secondary). Empty when the user didn't
    /// specify a sort; the window then falls back to position order.
    pub sorts: Vec<SortSpec>,
    /// Raw input, kept around for the operator-reference popover and
    /// the search history ring buffer.
    pub raw: String,
}

/// Parse a search-bar / saved-perspective expression.
pub fn parse(input: &str) -> FilterQuery {
    let raw = input.to_string();
    match atrium_search::parse(input) {
        Ok(result) => FilterQuery {
            expr: Some(result.expr),
            warnings: result.warnings,
            sorts: result.sorts,
            raw,
        },
        Err(_) => FilterQuery {
            expr: None,
            warnings: Vec::new(),
            sorts: Vec::new(),
            raw,
        },
    }
}

/// Apply a parsed expression against a task vector. When the query
/// is empty, returns the input unchanged. Builds the `EvalContext`
/// from the window-side caches the caller passes in.
///
/// v0.4.1: when `query.sorts` is non-empty, the result is *also*
/// sorted by those keys (primary → secondary, NULLs last). Callers
/// that have explicit sorts skip their own positional sort; callers
/// without sorts get the unsorted filter output and apply their own
/// ordering as before.
#[allow(clippy::too_many_arguments)]
pub fn apply(
    tasks: Vec<Task>,
    query: &FilterQuery,
    today: NaiveDate,
    tag_names: &HashMap<i64, Vec<String>>,
    project_titles: &HashMap<i64, String>,
    project_areas: &HashMap<i64, Option<i64>>,
    area_titles: &HashMap<i64, String>,
) -> Vec<Task> {
    // Dependency predicates (`is:blocked` / `is:available`) default to
    // "no blockers" on this path. Callers that may evaluate them in
    // the in-memory fallback should use `apply_with_blocked`.
    static EMPTY: std::sync::LazyLock<std::collections::HashSet<i64>> =
        std::sync::LazyLock::new(std::collections::HashSet::new);
    apply_with_blocked(
        tasks,
        query,
        today,
        tag_names,
        project_titles,
        project_areas,
        area_titles,
        &EMPTY,
    )
}

/// As [`apply`], but with the set of currently-blocked task ids so
/// `is:blocked` / `is:available` evaluate correctly in the in-memory
/// fallback (the SQL fast-path handles them when a query translates
/// wholesale; this path is the regex / fuzzy / composite remainder).
#[allow(clippy::too_many_arguments)]
pub fn apply_with_blocked(
    tasks: Vec<Task>,
    query: &FilterQuery,
    today: NaiveDate,
    tag_names: &HashMap<i64, Vec<String>>,
    project_titles: &HashMap<i64, String>,
    project_areas: &HashMap<i64, Option<i64>>,
    area_titles: &HashMap<i64, String>,
    blocked_ids: &std::collections::HashSet<i64>,
) -> Vec<Task> {
    let Some(expr) = &query.expr else {
        return tasks;
    };
    let ctx = EvalContext::new(today, tag_names, project_titles, project_areas, area_titles)
        .with_blocked_ids(blocked_ids);
    let mut filtered: Vec<Task> = tasks
        .into_iter()
        .filter(|t| evaluate(expr, t, &ctx))
        .collect();
    if !query.sorts.is_empty() {
        sort_tasks(&mut filtered, &query.sorts);
    }
    filtered
}

/// Reorder `tasks` in-place by FTS5 bm25 + recency. The caller
/// passes in the `bm25` map from `atrium_core::db::read::bm25_for_terms`
/// (so this helper itself stays DB-agnostic). No-op when `scores`
/// is empty — the caller decides when the bm25 fast-path applies
/// (bare text in the expression AND no explicit `sort:` modifier).
///
/// Unranked tasks (those not in the FTS5 hit set) keep their
/// existing relative order at the bottom of the list — Rust's
/// `sort_by` is stable.
pub fn rank_by_bm25_recency(tasks: &mut [Task], bm25_scores: &HashMap<i64, f64>, today: NaiveDate) {
    if bm25_scores.is_empty() {
        return;
    }
    const HALF_LIFE_DAYS: f64 = 30.0;
    tasks.sort_by(|a, b| {
        let sa = blended_score(a, bm25_scores, today, HALF_LIFE_DAYS);
        let sb = blended_score(b, bm25_scores, today, HALF_LIFE_DAYS);
        // Higher score sorts first.
        sb.partial_cmp(&sa).unwrap_or(Ordering::Equal)
    });
}

fn blended_score(task: &Task, scores: &HashMap<i64, f64>, today: NaiveDate, half_life: f64) -> f64 {
    let bm25 = scores.get(&task.id).copied().unwrap_or(0.0);
    let days = (today - task.modified_at.date_naive()).num_days();
    atrium_search::blend_relevance(bm25, days, half_life)
}

/// Stable-sort `tasks` by the configured sorts in primary-first
/// order. Tasks lacking the sort field always sink to the end of
/// the list regardless of direction (NULLs-last convention).
/// Stable-sort `tasks` by the configured sorts in primary-first
/// order. Public so the SQL fast-path in `window.rs` can apply
/// explicit sort modifiers without going through the full
/// `apply` pipeline.
pub fn sort_tasks_by_specs(tasks: &mut [Task], sorts: &[SortSpec]) {
    sort_tasks(tasks, sorts);
}

fn sort_tasks(tasks: &mut [Task], sorts: &[SortSpec]) {
    tasks.sort_by(|a, b| {
        for spec in sorts {
            let ord = compare_for_sort(a, b, *spec);
            if ord != Ordering::Equal {
                return ord;
            }
        }
        Ordering::Equal
    });
}

fn compare_for_sort(a: &Task, b: &Task, spec: SortSpec) -> Ordering {
    match spec.key {
        SortKey::Due => cmp_option(task_deadline(a), task_deadline(b), spec.direction),
        SortKey::Scheduled => cmp_option(
            task_scheduled_date(a),
            task_scheduled_date(b),
            spec.direction,
        ),
        SortKey::Defer => cmp_option(a.defer_until, b.defer_until, spec.direction),
        SortKey::Created => cmp_with_dir(a.created_at, b.created_at, spec.direction),
        SortKey::Modified => cmp_with_dir(a.modified_at, b.modified_at, spec.direction),
        SortKey::Completed => cmp_option(a.completed_at, b.completed_at, spec.direction),
        SortKey::Estimated => cmp_option(a.estimated_minutes, b.estimated_minutes, spec.direction),
        SortKey::Title => cmp_with_dir(a.title.as_str(), b.title.as_str(), spec.direction),
        SortKey::Position => {
            cmp_with_dir(FloatOrd(a.position), FloatOrd(b.position), spec.direction)
        }
    }
}

fn task_deadline(t: &Task) -> Option<NaiveDate> {
    t.deadline
}

fn task_scheduled_date(t: &Task) -> Option<NaiveDate> {
    match &t.scheduled_for {
        Some(ScheduledFor::Date(d)) => Some(*d),
        // Someday sorts as None — the sentinel isn't a real date.
        _ => None,
    }
}

/// Compare two `Option<T>` values with NULLs last regardless of
/// direction (SQL's NULLS LAST convention; the user wants tasks
/// missing the sort field at the bottom of the list).
fn cmp_option<T: Ord>(a: Option<T>, b: Option<T>, dir: SortDirection) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(av), Some(bv)) => apply_dir(av.cmp(&bv), dir),
    }
}

fn cmp_with_dir<T: Ord>(a: T, b: T, dir: SortDirection) -> Ordering {
    apply_dir(a.cmp(&b), dir)
}

fn apply_dir(ord: Ordering, dir: SortDirection) -> Ordering {
    match dir {
        SortDirection::Asc => ord,
        SortDirection::Desc => ord.reverse(),
    }
}

/// f64 wrapper that's `Ord` (NaN goes last via `total_cmp`). Position
/// is always finite in practice — this is just a guard. PartialOrd
/// is hand-written rather than derived to stay consistent with Ord
/// (clippy::derive_ord_xor_partial_ord).
#[derive(Clone, Copy)]
struct FloatOrd(f64);
impl PartialEq for FloatOrd {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for FloatOrd {}
impl PartialOrd for FloatOrd {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for FloatOrd {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.total_cmp(&other.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use atrium_core::test_support::dummy_task;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn parse_empty_input_returns_none_expr() {
        let q = parse("");
        assert!(q.expr.is_none());
        assert!(q.warnings.is_empty());
    }

    #[test]
    fn parse_collects_warnings() {
        let q = parse("tga:errand");
        assert_eq!(q.warnings, vec!["tga:errand"]);
    }

    #[test]
    fn apply_filters_by_text() {
        let mut t1 = dummy_task(1);
        t1.title = "Buy milk".into();
        let mut t2 = dummy_task(2);
        t2.title = "Read book".into();
        let q = parse("milk");
        let out = apply(
            vec![t1.clone(), t2.clone()],
            &q,
            d(2026, 5, 15),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, 1);
    }

    #[test]
    fn apply_filters_by_tag() {
        let t1 = dummy_task(1);
        let t2 = dummy_task(2);
        let mut tag_names = HashMap::new();
        tag_names.insert(1, vec!["work".to_string()]);
        let q = parse("tag:work");
        let out = apply(
            vec![t1, t2],
            &q,
            d(2026, 5, 15),
            &tag_names,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, 1);
    }

    // v0.5.0 — locking in the binary-side behaviour for `due:today`,
    // since Brandon flagged it as not working in the search bar.
    // The atrium-core unit tests already cover the parse + eval path
    // in isolation; this test exercises the *full* binary integration
    // (parse → context-build → apply against a Vec<Task>) so any
    // regression in the shim layer surfaces here too.
    #[test]
    fn apply_due_today_matches_only_deadline_today() {
        let mut today_task = dummy_task(1);
        today_task.deadline = Some(d(2026, 5, 15));
        let mut tomorrow_task = dummy_task(2);
        tomorrow_task.deadline = Some(d(2026, 5, 16));
        let mut no_deadline_task = dummy_task(3);
        no_deadline_task.deadline = None;
        // Scheduled-for-today but no deadline: must NOT match `due:today`.
        // `due:` is exact match on the `deadline` column; `scheduled:`
        // is the equivalent on `scheduled_for`. The two are distinct
        // fields and the search expression keeps that distinction.
        let mut scheduled_only_task = dummy_task(4);
        scheduled_only_task.scheduled_for = Some(atrium_core::ScheduledFor::Date(d(2026, 5, 15)));

        let q = parse("due:today");
        let out = apply(
            vec![
                today_task,
                tomorrow_task,
                no_deadline_task,
                scheduled_only_task,
            ],
            &q,
            d(2026, 5, 15),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(out.len(), 1, "only the deadline=today task should match");
        assert_eq!(out[0].id, 1);
    }

    // v0.4.1 — `sort:KEY` orders the result Vec. Stable secondary
    // ordering uses input position when primary sort ties.
    #[test]
    fn apply_sort_due_ascending_orders_by_deadline() {
        let mut t1 = dummy_task(1);
        t1.deadline = Some(d(2026, 5, 20));
        let mut t2 = dummy_task(2);
        t2.deadline = Some(d(2026, 5, 10));
        let mut t3 = dummy_task(3);
        t3.deadline = None; // sinks to last (NULLs LAST)
        let mut t4 = dummy_task(4);
        t4.deadline = Some(d(2026, 5, 15));
        let q = parse("sort:due");
        let out = apply(
            vec![t1, t2, t3, t4],
            &q,
            d(2026, 5, 1),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        let ids: Vec<i64> = out.iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![2, 4, 1, 3]);
    }

    #[test]
    fn apply_sort_descending_with_dash_prefix() {
        let mut t1 = dummy_task(1);
        t1.deadline = Some(d(2026, 5, 10));
        let mut t2 = dummy_task(2);
        t2.deadline = Some(d(2026, 5, 20));
        let q = parse("sort:-due");
        let out = apply(
            vec![t1, t2],
            &q,
            d(2026, 5, 1),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        let ids: Vec<i64> = out.iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![2, 1]);
    }

    #[test]
    fn apply_sort_filters_first_then_orders() {
        // Filter to `tag:work` then sort by deadline ascending.
        let mut t1 = dummy_task(1);
        t1.deadline = Some(d(2026, 5, 20));
        let mut t2 = dummy_task(2);
        t2.deadline = Some(d(2026, 5, 10));
        let mut t3 = dummy_task(3);
        t3.deadline = Some(d(2026, 5, 5));
        let mut tag_names = HashMap::new();
        tag_names.insert(1, vec!["work".into()]);
        tag_names.insert(2, vec!["work".into()]);
        // t3 has no `work` tag — must drop out before sort.
        let q = parse("tag:work sort:due");
        let out = apply(
            vec![t1, t2, t3],
            &q,
            d(2026, 5, 1),
            &tag_names,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(out.len(), 2);
        let ids: Vec<i64> = out.iter().map(|t| t.id).collect();
        assert_eq!(ids, vec![2, 1]);
    }

    #[test]
    fn apply_sort_title_alphabetical() {
        let mut t1 = dummy_task(1);
        t1.title = "Zebra".into();
        let mut t2 = dummy_task(2);
        t2.title = "Apple".into();
        let mut t3 = dummy_task(3);
        t3.title = "Mango".into();
        let q = parse("sort:title");
        let out = apply(
            vec![t1, t2, t3],
            &q,
            d(2026, 5, 1),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        let titles: Vec<String> = out.iter().map(|t| t.title.clone()).collect();
        assert_eq!(titles, vec!["Apple", "Mango", "Zebra"]);
    }

    #[test]
    fn apply_sort_multi_key_secondary_breaks_ties() {
        let mut t1 = dummy_task(1);
        t1.deadline = Some(d(2026, 5, 10));
        t1.title = "Bravo".into();
        let mut t2 = dummy_task(2);
        t2.deadline = Some(d(2026, 5, 10));
        t2.title = "Alpha".into();
        let mut t3 = dummy_task(3);
        t3.deadline = Some(d(2026, 5, 5));
        t3.title = "Charlie".into();
        let q = parse("sort:due sort:title");
        let out = apply(
            vec![t1, t2, t3],
            &q,
            d(2026, 5, 1),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        let ids: Vec<i64> = out.iter().map(|t| t.id).collect();
        // Charlie (May 5) first; then May 10 ties broken by title:
        // Alpha (id=2) before Bravo (id=1).
        assert_eq!(ids, vec![3, 2, 1]);
    }

    // Calibre's "comparison form" — `due:>today`, `due:<=today`, etc.
    // Locks in the comparator semantics: strict and inclusive bounds
    // both work as documented.
    #[test]
    fn apply_due_comparison_to_today() {
        let mut overdue = dummy_task(1);
        overdue.deadline = Some(d(2026, 5, 10));
        let mut today_task = dummy_task(2);
        today_task.deadline = Some(d(2026, 5, 15));
        let mut future = dummy_task(3);
        future.deadline = Some(d(2026, 5, 20));

        let today = d(2026, 5, 15);
        let tasks = vec![overdue.clone(), today_task.clone(), future.clone()];

        let after = apply(
            tasks.clone(),
            &parse("due:>today"),
            today,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(after.iter().map(|t| t.id).collect::<Vec<_>>(), vec![3]);

        let on_or_before = apply(
            tasks.clone(),
            &parse("due:<=today"),
            today,
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(
            on_or_before.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn empty_expr_passes_tasks_through_unchanged() {
        let t = vec![dummy_task(1), dummy_task(2)];
        let q = parse("");
        let out = apply(
            t.clone(),
            &q,
            d(2026, 5, 15),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn rank_by_bm25_no_op_on_empty_scores() {
        let mut tasks = vec![dummy_task(1), dummy_task(2), dummy_task(3)];
        let scores: HashMap<i64, f64> = HashMap::new();
        rank_by_bm25_recency(&mut tasks, &scores, d(2026, 5, 15));
        // Order preserved when nothing was scored.
        assert_eq!(
            tasks.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn rank_by_bm25_orders_strong_match_ahead_of_weak_match() {
        let mut tasks = vec![dummy_task(1), dummy_task(2)];
        // Same modified_at (dummy_task default) so recency is a
        // wash; bm25 alone determines order.
        let mut scores = HashMap::new();
        scores.insert(1_i64, -1.0); // weak
        scores.insert(2_i64, -10.0); // strong
        rank_by_bm25_recency(&mut tasks, &scores, d(2026, 5, 15));
        assert_eq!(tasks.iter().map(|t| t.id).collect::<Vec<_>>(), vec![2, 1]);
    }

    #[test]
    fn rank_by_bm25_breaks_ties_with_recency() {
        let mut t_recent = dummy_task(1);
        t_recent.modified_at = chrono::DateTime::parse_from_rfc3339("2026-05-15T08:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let mut t_stale = dummy_task(2);
        t_stale.modified_at = chrono::DateTime::parse_from_rfc3339("2026-04-01T08:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let mut tasks = vec![t_stale.clone(), t_recent.clone()];
        // Equal bm25 → recency tiebreaker.
        let mut scores = HashMap::new();
        scores.insert(1_i64, -3.0);
        scores.insert(2_i64, -3.0);
        rank_by_bm25_recency(&mut tasks, &scores, d(2026, 5, 15));
        assert_eq!(
            tasks.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![1, 2],
            "recent task should outrank stale at equal bm25"
        );
    }

    #[test]
    fn rank_by_bm25_unscored_tasks_sink_below_scored() {
        let mut tasks = vec![dummy_task(1), dummy_task(2), dummy_task(3)];
        // Only id=2 has a score — id=1 and id=3 fall back to a
        // relevance term of 0 + the same recency contribution, so
        // they sit *below* id=2 (the scored one) but keep their
        // existing relative order between themselves.
        let mut scores = HashMap::new();
        scores.insert(2_i64, -5.0);
        rank_by_bm25_recency(&mut tasks, &scores, d(2026, 5, 15));
        assert_eq!(tasks[0].id, 2, "scored task should rank first");
    }
}
