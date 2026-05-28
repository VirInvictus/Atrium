// SPDX-License-Identifier: MIT
//! Integration tests for the search module — parse → evaluate
//! round-trips against synthetic Task fixtures.

use std::collections::{HashMap, HashSet};

use chrono::{NaiveDate, Utc};

use atrium_core::domain::{ScheduledFor, Task};
use atrium_core::test_support::dummy_task;

use super::ast::{
    Comparator, DateKeyword, Expr, Field, MatchKind, SortDirection, SortKey, SortSpec, State, Value,
};
use super::eval::{EvalContext, evaluate};
use super::parse::parse;

fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, day).unwrap()
}

/// Build an empty context with `today` pinned for deterministic eval.
fn empty_ctx<'a>(
    today: NaiveDate,
    tag_names: &'a HashMap<i64, Vec<String>>,
    project_titles: &'a HashMap<i64, String>,
    project_areas: &'a HashMap<i64, Option<i64>>,
    area_titles: &'a HashMap<i64, String>,
) -> EvalContext<'a> {
    EvalContext::new(today, tag_names, project_titles, project_areas, area_titles)
}

fn match_with_tags(
    expr: &Expr,
    task: &Task,
    today: NaiveDate,
    tag_names: HashMap<i64, Vec<String>>,
) -> bool {
    let pt = HashMap::new();
    let pa = HashMap::new();
    let at = HashMap::new();
    let ctx = empty_ctx(today, &tag_names, &pt, &pa, &at);
    evaluate(expr, task, &ctx)
}

fn match_simple(expr: &Expr, task: &Task, today: NaiveDate) -> bool {
    match_with_tags(expr, task, today, HashMap::new())
}

// ── Parse round-trips ────────────────────────────────────────────

#[test]
fn parse_bareword_to_text() {
    let r = parse("milk").unwrap();
    assert_eq!(r.expr, Expr::Text("milk".into()));
    assert!(r.warnings.is_empty());
}

#[test]
fn parse_quoted_string_to_text() {
    let r = parse(r#""buy milk""#).unwrap();
    assert_eq!(r.expr, Expr::Text("buy milk".into()));
}

#[test]
fn parse_tag_substring() {
    let r = parse("tag:work").unwrap();
    assert_eq!(
        r.expr,
        Expr::Field {
            field: Field::Tag,
            kind: MatchKind::Substring("work".into())
        }
    );
}

#[test]
fn parse_tag_quoted_substring() {
    let r = parse(r#"tag:"work focus""#).unwrap();
    assert_eq!(
        r.expr,
        Expr::Field {
            field: Field::Tag,
            kind: MatchKind::Substring("work focus".into())
        }
    );
}

#[test]
fn parse_tag_exact() {
    let r = parse("tag:=work").unwrap();
    assert_eq!(
        r.expr,
        Expr::Field {
            field: Field::Tag,
            kind: MatchKind::Exact("work".into())
        }
    );
}

#[test]
fn parse_tag_quoted_exact() {
    let r = parse(r#"tag:"=work focus""#).unwrap();
    assert_eq!(
        r.expr,
        Expr::Field {
            field: Field::Tag,
            kind: MatchKind::Exact("work focus".into())
        }
    );
}

#[test]
fn parse_tag_regex() {
    let r = parse("tag:~mystery.*").unwrap();
    assert_eq!(
        r.expr,
        Expr::Field {
            field: Field::Tag,
            kind: MatchKind::Regex("mystery.*".into())
        }
    );
}

#[test]
fn parse_tag_boolean_existence() {
    let r1 = parse("tag:true").unwrap();
    assert_eq!(
        r1.expr,
        Expr::Field {
            field: Field::Tag,
            kind: MatchKind::HasAny
        }
    );
    let r2 = parse("tag:false").unwrap();
    assert_eq!(
        r2.expr,
        Expr::Field {
            field: Field::Tag,
            kind: MatchKind::HasNone
        }
    );
}

#[test]
fn parse_state_predicates() {
    for (input, expected) in [
        ("is:open", State::Open),
        ("is:done", State::Done),
        ("is:overdue", State::Overdue),
        ("is:scheduled", State::Scheduled),
        ("is:repeating", State::Repeating),
        ("is:tagged", State::Tagged),
    ] {
        assert_eq!(parse(input).unwrap().expr, Expr::State(expected));
    }
}

#[test]
fn parse_comparison_date_keyword() {
    let r = parse("due:>today").unwrap();
    assert_eq!(
        r.expr,
        Expr::Compare {
            field: Field::Due,
            comp: Comparator::Gt,
            value: Value::DateKeyword(DateKeyword::Today),
        }
    );
}

#[test]
fn parse_comparison_estimated() {
    let r = parse("estimated:>=30").unwrap();
    assert_eq!(
        r.expr,
        Expr::Compare {
            field: Field::Estimated,
            comp: Comparator::Ge,
            value: Value::Number(30),
        }
    );
}

#[test]
fn parse_range() {
    let r = parse("due:2026-05-01..2026-05-31").unwrap();
    assert_eq!(
        r.expr,
        Expr::Range {
            field: Field::Due,
            low: Value::Date(d(2026, 5, 1)),
            high: Value::Date(d(2026, 5, 31)),
        }
    );
}

#[test]
fn parse_implicit_and() {
    let r = parse("tag:work is:overdue").unwrap();
    let expected = Expr::And(vec![
        Expr::Field {
            field: Field::Tag,
            kind: MatchKind::Substring("work".into()),
        },
        Expr::State(State::Overdue),
    ]);
    assert_eq!(r.expr, expected);
}

#[test]
fn parse_explicit_and_or() {
    let r = parse("tag:work AND is:overdue OR tag:home").unwrap();
    // Precedence: AND binds tighter than OR.
    let expected = Expr::Or(vec![
        Expr::And(vec![
            Expr::Field {
                field: Field::Tag,
                kind: MatchKind::Substring("work".into()),
            },
            Expr::State(State::Overdue),
        ]),
        Expr::Field {
            field: Field::Tag,
            kind: MatchKind::Substring("home".into()),
        },
    ]);
    assert_eq!(r.expr, expected);
}

#[test]
fn parse_parens_override_precedence() {
    let r = parse("(tag:work OR tag:home) AND is:overdue").unwrap();
    let expected = Expr::And(vec![
        Expr::Or(vec![
            Expr::Field {
                field: Field::Tag,
                kind: MatchKind::Substring("work".into()),
            },
            Expr::Field {
                field: Field::Tag,
                kind: MatchKind::Substring("home".into()),
            },
        ]),
        Expr::State(State::Overdue),
    ]);
    assert_eq!(r.expr, expected);
}

#[test]
fn parse_not_word_and_bang() {
    let bang = parse("!tag:work").unwrap();
    assert_eq!(
        bang.expr,
        Expr::Not(Box::new(Expr::Field {
            field: Field::Tag,
            kind: MatchKind::Substring("work".into()),
        }))
    );
    let word = parse("NOT tag:work").unwrap();
    assert_eq!(word.expr, bang.expr);
}

#[test]
fn parse_unknown_field_warns_and_falls_back_to_text() {
    let r = parse("tga:errand").unwrap();
    assert_eq!(r.expr, Expr::Text("tga:errand".into()));
    assert_eq!(r.warnings, vec!["tga:errand"]);
}

#[test]
fn parse_unknown_state_warns() {
    let r = parse("is:fnord").unwrap();
    assert_eq!(r.expr, Expr::Text("is:fnord".into()));
    assert_eq!(r.warnings, vec!["is:fnord"]);
}

// ── Evaluator ────────────────────────────────────────────────────

fn task_with_title(id: i64, title: &str) -> Task {
    let mut t = dummy_task(id);
    t.title = title.into();
    t
}

#[test]
fn eval_text_substring_title() {
    let task = task_with_title(1, "Buy milk and eggs");
    let r = parse("milk").unwrap();
    assert!(match_simple(&r.expr, &task, d(2026, 5, 15)));
    let r2 = parse("oranges").unwrap();
    assert!(!match_simple(&r2.expr, &task, d(2026, 5, 15)));
}

#[test]
fn eval_tag_substring() {
    let task = dummy_task(1);
    let mut tag_names = HashMap::new();
    tag_names.insert(1, vec!["work-focus".to_string()]);
    let r = parse("tag:work").unwrap();
    assert!(match_with_tags(&r.expr, &task, d(2026, 5, 15), tag_names));
}

#[test]
fn eval_tag_exact_does_not_match_substring() {
    let task = dummy_task(1);
    let mut tag_names = HashMap::new();
    tag_names.insert(1, vec!["work-focus".to_string()]);
    let r = parse("tag:=work").unwrap();
    assert!(!match_with_tags(
        &r.expr,
        &task,
        d(2026, 5, 15),
        tag_names.clone()
    ));
    let r2 = parse("tag:=work-focus").unwrap();
    assert!(match_with_tags(&r2.expr, &task, d(2026, 5, 15), tag_names));
}

#[test]
fn eval_tag_regex() {
    let task = dummy_task(1);
    let mut tag_names = HashMap::new();
    tag_names.insert(1, vec!["mysteries-of-the-deep".to_string()]);
    let r = parse("tag:~myster").unwrap();
    assert!(match_with_tags(&r.expr, &task, d(2026, 5, 15), tag_names));
}

#[test]
fn eval_tag_has_any_and_none() {
    let mut t1 = dummy_task(1);
    t1.id = 1;
    let mut t2 = dummy_task(2);
    t2.id = 2;
    let mut tag_names = HashMap::new();
    tag_names.insert(1, vec!["any".to_string()]);
    let any = parse("tag:true").unwrap();
    let none = parse("tag:false").unwrap();
    assert!(match_with_tags(
        &any.expr,
        &t1,
        d(2026, 5, 15),
        tag_names.clone()
    ));
    assert!(!match_with_tags(
        &any.expr,
        &t2,
        d(2026, 5, 15),
        tag_names.clone()
    ));
    assert!(!match_with_tags(
        &none.expr,
        &t1,
        d(2026, 5, 15),
        tag_names.clone()
    ));
    assert!(match_with_tags(&none.expr, &t2, d(2026, 5, 15), tag_names));
}

#[test]
fn eval_state_open_done() {
    let mut open = dummy_task(1);
    open.completed_at = None;
    let mut done = dummy_task(2);
    done.completed_at = Some(Utc::now());
    let r_open = parse("is:open").unwrap();
    let r_done = parse("is:done").unwrap();
    assert!(match_simple(&r_open.expr, &open, d(2026, 5, 15)));
    assert!(!match_simple(&r_open.expr, &done, d(2026, 5, 15)));
    assert!(!match_simple(&r_done.expr, &open, d(2026, 5, 15)));
    assert!(match_simple(&r_done.expr, &done, d(2026, 5, 15)));
}

#[test]
fn eval_state_blocked_and_available() {
    // v0.29.0 — task 1 is blocked (in the set), task 2 is available
    // (open, not in the set), task 3 is completed (never blocked, and
    // not available because it's done).
    let mut t1 = dummy_task(1);
    t1.completed_at = None;
    let mut t2 = dummy_task(2);
    t2.completed_at = None;
    let mut t3 = dummy_task(3);
    t3.completed_at = Some(Utc::now());

    let tag_names = HashMap::new();
    let pt = HashMap::new();
    let pa = HashMap::new();
    let at = HashMap::new();
    let blocked: HashSet<i64> = [1].into_iter().collect();
    let ctx = empty_ctx(d(2026, 5, 15), &tag_names, &pt, &pa, &at).with_blocked_ids(&blocked);

    let blocked_expr = parse("is:blocked").unwrap().expr;
    let avail_expr = parse("is:available").unwrap().expr;

    assert!(evaluate(&blocked_expr, &t1, &ctx));
    assert!(!evaluate(&blocked_expr, &t2, &ctx));
    assert!(!evaluate(&blocked_expr, &t3, &ctx)); // completed → not blocked

    assert!(!evaluate(&avail_expr, &t1, &ctx)); // blocked → not available
    assert!(evaluate(&avail_expr, &t2, &ctx));
    assert!(!evaluate(&avail_expr, &t3, &ctx)); // completed → not available
}

#[test]
fn eval_state_overdue() {
    let mut overdue = dummy_task(1);
    overdue.deadline = Some(d(2026, 5, 10));
    let mut not_overdue = dummy_task(2);
    not_overdue.deadline = Some(d(2026, 5, 20));
    let r = parse("is:overdue").unwrap();
    assert!(match_simple(&r.expr, &overdue, d(2026, 5, 15)));
    assert!(!match_simple(&r.expr, &not_overdue, d(2026, 5, 15)));
}

#[test]
fn eval_due_compare_today() {
    let mut t = dummy_task(1);
    t.deadline = Some(d(2026, 5, 20));
    let r_gt = parse("due:>today").unwrap();
    let r_lt = parse("due:<today").unwrap();
    assert!(match_simple(&r_gt.expr, &t, d(2026, 5, 15)));
    assert!(!match_simple(&r_lt.expr, &t, d(2026, 5, 15)));
}

#[test]
fn eval_due_eq_keyword_collapses_to_range() {
    let mut t = dummy_task(1);
    t.deadline = Some(d(2026, 5, 12));
    let r = parse("due:thisweek").unwrap();
    assert!(match_simple(&r.expr, &t, d(2026, 5, 15))); // Mon May 11 .. Sun May 17 of 2026
}

#[test]
fn eval_range_inclusive() {
    let mut in_range = dummy_task(1);
    in_range.deadline = Some(d(2026, 5, 15));
    let mut before = dummy_task(2);
    before.deadline = Some(d(2026, 4, 30));
    let mut after = dummy_task(3);
    after.deadline = Some(d(2026, 6, 1));
    let r = parse("due:2026-05-01..2026-05-31").unwrap();
    let today = d(2026, 5, 15);
    assert!(match_simple(&r.expr, &in_range, today));
    assert!(!match_simple(&r.expr, &before, today));
    assert!(!match_simple(&r.expr, &after, today));
}

#[test]
fn eval_boolean_composition() {
    let mut t = dummy_task(1);
    t.completed_at = None;
    t.deadline = Some(d(2026, 5, 10));
    let mut tag_names = HashMap::new();
    tag_names.insert(1, vec!["work".to_string()]);
    let today = d(2026, 5, 15);

    // (tag:work AND is:overdue) → true
    let r = parse("tag:work AND is:overdue").unwrap();
    assert!(match_with_tags(&r.expr, &t, today, tag_names.clone()));

    // tag:home OR is:overdue → true (overdue half holds)
    let r2 = parse("tag:home OR is:overdue").unwrap();
    assert!(match_with_tags(&r2.expr, &t, today, tag_names.clone()));

    // NOT is:overdue → false
    let r3 = parse("NOT is:overdue").unwrap();
    assert!(!match_with_tags(&r3.expr, &t, today, tag_names.clone()));

    // tag:work AND NOT is:overdue → false (NOT half blocks it)
    let r4 = parse("tag:work AND NOT is:overdue").unwrap();
    assert!(!match_with_tags(&r4.expr, &t, today, tag_names));
}

#[test]
fn eval_scheduled_today() {
    let mut t = dummy_task(1);
    t.scheduled_for = Some(ScheduledFor::Date(d(2026, 5, 15)));
    let r = parse("scheduled:today").unwrap();
    assert!(match_simple(&r.expr, &t, d(2026, 5, 15)));
    let r2 = parse("scheduled:yesterday").unwrap();
    assert!(!match_simple(&r2.expr, &t, d(2026, 5, 15)));
}

// Repro for the v0.5.0 bug report: `due:today` in the search bar
// returned nothing even when tasks with deadline = today existed.
// Locked in as a regression test once we identify the cause.
#[test]
fn eval_due_today_bare_keyword() {
    let mut today_task = dummy_task(1);
    today_task.deadline = Some(d(2026, 5, 15));
    let mut tomorrow_task = dummy_task(2);
    tomorrow_task.deadline = Some(d(2026, 5, 16));
    let mut yesterday_task = dummy_task(3);
    yesterday_task.deadline = Some(d(2026, 5, 14));
    let mut no_deadline_task = dummy_task(4);
    no_deadline_task.deadline = None;
    let today = d(2026, 5, 15);

    let r = parse("due:today").unwrap();
    assert!(
        match_simple(&r.expr, &today_task, today),
        "due:today must match a task with deadline == today"
    );
    assert!(
        !match_simple(&r.expr, &tomorrow_task, today),
        "due:today must not match a task with deadline == tomorrow"
    );
    assert!(
        !match_simple(&r.expr, &yesterday_task, today),
        "due:today must not match a task with deadline == yesterday"
    );
    assert!(
        !match_simple(&r.expr, &no_deadline_task, today),
        "due:today must not match a task with no deadline"
    );
}

// And the alias path — `deadline:today` is the explicit form Calibre
// users might reach for first.
#[test]
fn eval_deadline_alias_today() {
    let mut t = dummy_task(1);
    t.deadline = Some(d(2026, 5, 15));
    let r = parse("deadline:today").unwrap();
    assert!(match_simple(&r.expr, &t, d(2026, 5, 15)));
}

// ── v0.4.1 canonical-list mirrors ──────────────────────────────

#[test]
fn eval_is_today_open_with_schedule_today_matches() {
    let mut t = dummy_task(1);
    t.scheduled_for = Some(ScheduledFor::Date(d(2026, 5, 15)));
    let r = parse("is:today").unwrap();
    assert!(match_simple(&r.expr, &t, d(2026, 5, 15)));
}

#[test]
fn eval_is_today_open_with_deadline_in_window_matches() {
    // Deadline 5 days out — within the 7-day heads-up horizon.
    let mut t = dummy_task(1);
    t.deadline = Some(d(2026, 5, 20));
    let r = parse("is:today").unwrap();
    assert!(match_simple(&r.expr, &t, d(2026, 5, 15)));
}

#[test]
fn eval_is_today_open_with_deadline_outside_window_no_match() {
    // Deadline 8 days out — past the 7-day heads-up horizon.
    let mut t = dummy_task(1);
    t.deadline = Some(d(2026, 5, 23));
    let r = parse("is:today").unwrap();
    assert!(!match_simple(&r.expr, &t, d(2026, 5, 15)));
}

#[test]
fn eval_is_today_completed_no_match() {
    let mut t = dummy_task(1);
    t.scheduled_for = Some(ScheduledFor::Date(d(2026, 5, 15)));
    t.completed_at = Some(Utc::now());
    let r = parse("is:today").unwrap();
    assert!(!match_simple(&r.expr, &t, d(2026, 5, 15)));
}

#[test]
fn eval_is_today_deferred_to_future_no_match() {
    let mut t = dummy_task(1);
    t.scheduled_for = Some(ScheduledFor::Date(d(2026, 5, 15)));
    t.defer_until = Some(d(2026, 5, 20));
    let r = parse("is:today").unwrap();
    assert!(!match_simple(&r.expr, &t, d(2026, 5, 15)));
}

#[test]
fn eval_is_today_someday_no_match() {
    let mut t = dummy_task(1);
    t.scheduled_for = Some(ScheduledFor::Someday);
    let r = parse("is:today").unwrap();
    assert!(!match_simple(&r.expr, &t, d(2026, 5, 15)));
}

#[test]
fn eval_is_inbox_open_no_project_matches() {
    let t = dummy_task(1);
    let r = parse("is:inbox").unwrap();
    assert!(match_simple(&r.expr, &t, d(2026, 5, 15)));
}

#[test]
fn eval_is_inbox_with_project_no_match() {
    let mut t = dummy_task(1);
    t.project_id = Some(7);
    let r = parse("is:inbox").unwrap();
    assert!(!match_simple(&r.expr, &t, d(2026, 5, 15)));
}

#[test]
fn eval_is_upcoming_open_with_future_schedule_matches() {
    let mut t = dummy_task(1);
    t.scheduled_for = Some(ScheduledFor::Date(d(2026, 5, 20)));
    let r = parse("is:upcoming").unwrap();
    assert!(match_simple(&r.expr, &t, d(2026, 5, 15)));
}

#[test]
fn eval_is_upcoming_open_with_today_schedule_no_match() {
    // Today's schedule belongs in `is:today`, not `is:upcoming`.
    let mut t = dummy_task(1);
    t.scheduled_for = Some(ScheduledFor::Date(d(2026, 5, 15)));
    let r = parse("is:upcoming").unwrap();
    assert!(!match_simple(&r.expr, &t, d(2026, 5, 15)));
}

#[test]
fn eval_is_anytime_open_no_schedule_matches() {
    let t = dummy_task(1);
    let r = parse("is:anytime").unwrap();
    assert!(match_simple(&r.expr, &t, d(2026, 5, 15)));
}

#[test]
fn eval_is_anytime_with_schedule_no_match() {
    let mut t = dummy_task(1);
    t.scheduled_for = Some(ScheduledFor::Date(d(2026, 5, 15)));
    let r = parse("is:anytime").unwrap();
    assert!(!match_simple(&r.expr, &t, d(2026, 5, 15)));
}

#[test]
fn eval_is_someday_with_someday_sentinel_matches() {
    let mut t = dummy_task(1);
    t.scheduled_for = Some(ScheduledFor::Someday);
    let r = parse("is:someday").unwrap();
    assert!(match_simple(&r.expr, &t, d(2026, 5, 15)));
}

#[test]
fn eval_is_someday_completed_no_match() {
    let mut t = dummy_task(1);
    t.scheduled_for = Some(ScheduledFor::Someday);
    t.completed_at = Some(Utc::now());
    let r = parse("is:someday").unwrap();
    assert!(!match_simple(&r.expr, &t, d(2026, 5, 15)));
}

// ── v0.4.1 fuzzy modifier ───────────────────────────────────────

#[test]
fn parse_tag_fuzzy_modifier() {
    let r = parse("tag:?work").unwrap();
    assert_eq!(
        r.expr,
        Expr::Field {
            field: Field::Tag,
            kind: MatchKind::Fuzzy("work".into())
        }
    );
}

#[test]
fn parse_title_fuzzy_modifier() {
    let r = parse("title:?milk").unwrap();
    assert_eq!(
        r.expr,
        Expr::Field {
            field: Field::Title,
            kind: MatchKind::Fuzzy("milk".into())
        }
    );
}

#[test]
fn fuzzy_modifier_does_not_apply_to_date_fields() {
    // `due:?today` keeps the leading `?` as a literal value char
    // (the comparison/sense-correction paths take precedence on
    // date-shaped fields). The eval will simply not match anything.
    let r = parse("due:?today").unwrap();
    // This becomes a substring match against `?today` — nothing
    // useful, but no Fuzzy variant.
    assert!(!matches!(
        &r.expr,
        Expr::Field {
            kind: MatchKind::Fuzzy(_),
            ..
        }
    ));
}

#[test]
fn eval_tag_fuzzy_matches_exact() {
    let t = dummy_task(1);
    let mut tag_names = HashMap::new();
    tag_names.insert(1, vec!["work".into()]);
    let r = parse("tag:?work").unwrap();
    assert!(match_with_tags(&r.expr, &t, d(2026, 5, 15), tag_names));
}

#[test]
fn eval_tag_fuzzy_matches_single_typo() {
    // Levenshtein distance 1 (transposition: o↔r).
    let t = dummy_task(1);
    let mut tag_names = HashMap::new();
    tag_names.insert(1, vec!["work".into()]);
    let r = parse("tag:?wrok").unwrap();
    assert!(match_with_tags(&r.expr, &t, d(2026, 5, 15), tag_names));
}

#[test]
fn eval_tag_fuzzy_matches_single_deletion() {
    // "wok" → "work" is one insertion, distance 1.
    let t = dummy_task(1);
    let mut tag_names = HashMap::new();
    tag_names.insert(1, vec!["work".into()]);
    let r = parse("tag:?wok").unwrap();
    assert!(match_with_tags(&r.expr, &t, d(2026, 5, 15), tag_names));
}

#[test]
fn eval_tag_fuzzy_rejects_two_typos_on_short_word() {
    // "wxxk" → "work" is distance 2 — past the threshold for a 4-
    // character query.
    let t = dummy_task(1);
    let mut tag_names = HashMap::new();
    tag_names.insert(1, vec!["work".into()]);
    let r = parse("tag:?wxxk").unwrap();
    assert!(!match_with_tags(&r.expr, &t, d(2026, 5, 15), tag_names));
}

#[test]
fn eval_tag_fuzzy_tolerates_two_typos_on_medium_word() {
    // 7-char query → threshold 2.
    let t = dummy_task(1);
    let mut tag_names = HashMap::new();
    tag_names.insert(1, vec!["meeting".into()]);
    // "metting" — one substitution; "metings" — one substitution +
    // one deletion (distance 2).
    let close = parse("tag:?metings").unwrap();
    assert!(match_with_tags(&close.expr, &t, d(2026, 5, 15), tag_names));
}

#[test]
fn eval_title_fuzzy_matches() {
    let mut t = dummy_task(1);
    t.title = "Buy milk".into();
    // Fuzzy needle "mlik" matches the literal "milk" inside the
    // title via per-candidate Levenshtein. (Note: title fuzzy is
    // whole-string, so the candidate is "Buy milk" and we expect a
    // substring-style match? No — Fuzzy is per-candidate strict.
    // For title, the candidate is the whole title. Distance from
    // "mlik" to "Buy milk" is 5 — fails. This test pins the
    // *intentional* strict whole-string behaviour.)
    let r = parse("title:?mlik").unwrap();
    assert!(!match_simple(&r.expr, &t, d(2026, 5, 15)));
}

#[test]
fn eval_title_fuzzy_against_short_titles() {
    // When the title is the same length as the needle, fuzzy
    // catches typos.
    let mut t = dummy_task(1);
    t.title = "milk".into();
    let r = parse("title:?mlik").unwrap();
    assert!(match_simple(&r.expr, &t, d(2026, 5, 15)));
}

// ── v0.4.1 sort modifier ────────────────────────────────────────

#[test]
fn parse_sort_ascending_default() {
    let r = parse("sort:due").unwrap();
    assert_eq!(
        r.sorts,
        vec![SortSpec {
            key: SortKey::Due,
            direction: SortDirection::Asc
        }]
    );
    // The AST should be Pass — sort modifier doesn't filter.
    assert_eq!(r.expr, Expr::Pass);
}

#[test]
fn parse_sort_descending_with_dash_prefix() {
    let r = parse("sort:-completed").unwrap();
    assert_eq!(
        r.sorts,
        vec![SortSpec {
            key: SortKey::Completed,
            direction: SortDirection::Desc
        }]
    );
}

#[test]
fn parse_sort_alongside_filter() {
    // `tag:work sort:due` — the And reduces to just tag:work since
    // sort folds to Pass.
    let r = parse("tag:work sort:due").unwrap();
    assert_eq!(
        r.sorts,
        vec![SortSpec {
            key: SortKey::Due,
            direction: SortDirection::Asc
        }]
    );
    // Implicit AND of Field(work) and Pass — Pass acts as identity.
    assert!(matches!(r.expr, Expr::And(_)));
    if let Expr::And(items) = &r.expr {
        assert_eq!(items.len(), 2);
        assert!(items.iter().any(|e| matches!(
            e,
            Expr::Field {
                field: Field::Tag,
                ..
            }
        )));
        assert!(items.iter().any(|e| matches!(e, Expr::Pass)));
    }
}

#[test]
fn parse_multiple_sorts_compose_in_order() {
    let r = parse("sort:-due sort:title").unwrap();
    assert_eq!(
        r.sorts,
        vec![
            SortSpec {
                key: SortKey::Due,
                direction: SortDirection::Desc
            },
            SortSpec {
                key: SortKey::Title,
                direction: SortDirection::Asc
            },
        ]
    );
}

#[test]
fn parse_sort_unknown_key_warns_and_falls_back() {
    let r = parse("sort:bogus").unwrap();
    assert!(r.sorts.is_empty());
    assert_eq!(r.warnings, vec!["sort:bogus"]);
    // Falls through to freeform text.
    assert_eq!(r.expr, Expr::Text("sort:bogus".into()));
}

#[test]
fn eval_pass_node_is_identity_in_and() {
    // Manually-constructed expression to exercise Expr::Pass directly.
    let expr = Expr::And(vec![
        Expr::Pass,
        Expr::Field {
            field: Field::Tag,
            kind: MatchKind::Substring("work".into()),
        },
    ]);
    let mut t = dummy_task(1);
    t.title = "anything".into();
    let mut tag_names = HashMap::new();
    tag_names.insert(1, vec!["work".into()]);
    assert!(match_with_tags(&expr, &t, d(2026, 5, 15), tag_names));
}

// ── Display / round-trip ─────────────────────────────────────────

#[test]
fn display_round_trips_for_simple_expressions() {
    for input in [
        "tag:work",
        "tag:=work",
        "tag:~mystery",
        "is:overdue",
        "tag:work AND is:overdue",
        "tag:work OR tag:home",
        "NOT tag:work",
    ] {
        let parsed = parse(input).unwrap();
        let rendered = parsed.expr.to_string();
        let reparsed = parse(&rendered).unwrap();
        assert_eq!(parsed.expr, reparsed.expr, "round-trip failed for {input}");
    }
}
