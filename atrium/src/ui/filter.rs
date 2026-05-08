// SPDX-License-Identifier: MIT
//! Search expression entry point for the binary — Phase 15.5 shim.
//!
//! The Phase 7d filter parser (flat `tag:foo is:overdue` shape) was
//! superseded in v0.4.0 by `atrium_core::search`, a real recursive-
//! descent parser with Calibre-style match modifiers, boolean
//! composition, ranges, and date keywords. This module holds the
//! window-side glue:
//!
//! - [`parse`] — wraps `atrium_core::search::parse` and surfaces
//!   non-fatal warnings (unknown field names that fall through to
//!   freeform text).
//! - [`apply`] — runs the parsed expression against a task list,
//!   building an `EvalContext` from the window's existing caches.
//!
//! The window's call sites keep their old shape: `parse(query)`
//! returns a [`FilterQuery`] carrying the AST + warnings; `apply`
//! filters a task vector. Saved Perspectives go through the same
//! path so v0.1.17 perspective expressions inherit the new grammar
//! the moment v0.4.0 ships.

use std::collections::HashMap;

use atrium_core::Task;
use atrium_core::search::{EvalContext, Expr, evaluate};
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
    /// Raw input, kept around for the operator-reference popover and
    /// the search history ring buffer.
    pub raw: String,
}

/// Parse a search-bar / saved-perspective expression.
pub fn parse(input: &str) -> FilterQuery {
    let raw = input.to_string();
    match atrium_core::search::parse(input) {
        Ok(result) => FilterQuery {
            expr: Some(result.expr),
            warnings: result.warnings,
            raw,
        },
        Err(_) => FilterQuery {
            expr: None,
            warnings: Vec::new(),
            raw,
        },
    }
}

/// Apply a parsed expression against a task vector. When the query
/// is empty, returns the input unchanged. Builds the `EvalContext`
/// from the window-side caches the caller passes in.
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
    let Some(expr) = &query.expr else {
        return tasks;
    };
    let ctx = EvalContext::new(today, tag_names, project_titles, project_areas, area_titles);
    tasks
        .into_iter()
        .filter(|t| evaluate(expr, t, &ctx))
        .collect()
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
}
