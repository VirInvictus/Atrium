// SPDX-License-Identifier: MIT
//! Integration tests for the search module — parse → evaluate
//! round-trips against synthetic Task fixtures.

use std::collections::HashMap;

use chrono::{NaiveDate, Utc};

use crate::domain::{ScheduledFor, Task};
use crate::test_support::dummy_task;

use super::ast::{Comparator, DateKeyword, Expr, Field, MatchKind, State, Value};
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
