// SPDX-License-Identifier: MIT
//! Tests for atrium-core/src/db/worker.rs.
//!
//! Loaded as the worker module's tests submodule via
//! `#[cfg(test)] #[path = "worker_tests.rs"] mod tests;`.
//! Extracted from worker.rs in v0.8.0's maintenance pass to keep
//! the production code path under 1500 lines for review focus.

use super::*;
use crate::db;
use std::time::Duration;

fn fresh_conn() -> Connection {
    let mut conn = Connection::open_in_memory().unwrap();
    db::configure_pragmas(&conn).unwrap();
    crate::db::migrations::migrate(&mut conn).unwrap();
    conn
}

#[tokio::test]
async fn create_task_honors_caller_provided_uuid() {
    // the Org importer relies on this. Passing a
    // UUID through NewTask must round-trip into the row.
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let provided = "11111111-2222-3333-4444-555555555555";
    let new = NewTask {
        title: "imported".to_string(),
        uuid: Some(provided.to_string()),
        ..Default::default()
    };
    let task = handle.create_task(new).await.unwrap();
    assert_eq!(task.uuid, provided);
}

#[tokio::test]
async fn create_task_falls_back_to_generated_uuid_for_empty_string() {
    // Defensive: an empty-string UUID is treated as "absent"
    // and the worker generates one. Avoids a foot-gun where a
    // caller might pass `Some(String::new())` and end up with
    // a row whose uuid is the empty string (would fail FK and
    // round-trip checks elsewhere).
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let new = NewTask {
        title: "with empty uuid".to_string(),
        uuid: Some(String::new()),
        ..Default::default()
    };
    let task = handle.create_task(new).await.unwrap();
    assert!(!task.uuid.is_empty());
    assert_ne!(task.uuid, "");
}

// Phase 16 Org/vault end-to-end tests moved to
// atrium-org/tests/worker_org_integration.rs at v0.9.0 (when the
// org parser/emitter + VaultWriter task moved into the atrium-org
// crate). The tests cover the same surface; just on the right
// side of the crate boundary.

#[tokio::test]
async fn ensure_area_creates_then_dedupes_case_insensitive() {
    // idempotent area create-by-name. First call
    // creates a row; second call with a differently-cased name
    // returns the same row.
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let first = handle.ensure_area("Personal".to_string()).await.unwrap();
    assert_eq!(first.title, "Personal");

    let second = handle.ensure_area("personal".to_string()).await.unwrap();
    assert_eq!(second.id, first.id, "case-insensitive match expected");

    let third = handle.ensure_area("PERSONAL".to_string()).await.unwrap();
    assert_eq!(third.id, first.id);

    // A truly different name creates a new row.
    let work = handle.ensure_area("Work".to_string()).await.unwrap();
    assert_ne!(work.id, first.id);
}

#[tokio::test]
async fn create_project_honors_caller_provided_uuid() {
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let provided = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
    let new = NewProject {
        title: "imported project".to_string(),
        uuid: Some(provided.to_string()),
        ..Default::default()
    };
    let project = handle.create_project(new).await.unwrap();
    assert_eq!(project.uuid, provided);
}

#[tokio::test]
async fn create_task_round_trip() {
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle
        .create_task(NewTask::inbox("buy milk"))
        .await
        .unwrap();
    assert_eq!(task.title, "buy milk");
    assert!(task.id > 0);
    assert!(!task.uuid.is_empty());
    assert!(task.completed_at.is_none());

    let changes = changes_rx.recv().await.unwrap();
    assert_eq!(changes.created.len(), 1);
    assert_eq!(changes.created[0].id, task.id);
}

#[tokio::test]
async fn update_task_changes_title_keeps_other_fields() {
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle.create_task(NewTask::inbox("first")).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();

    let updated = handle
        .update_task(TaskUpdate::new(task.id).title("second"))
        .await
        .unwrap();
    assert_eq!(updated.title, "second");
    assert_eq!(updated.uuid, task.uuid);
    assert_eq!(updated.id, task.id);

    let changes = changes_rx.recv().await.unwrap();
    assert_eq!(changes.updated.len(), 1);
    assert_eq!(changes.updated[0].title, "second");
}

#[tokio::test]
async fn update_task_sets_and_clears_schedule() {
    use crate::domain::ScheduledFor;
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle
        .create_task(NewTask::inbox("schedule me"))
        .await
        .unwrap();
    let _ = changes_rx.recv().await.unwrap();

    // Set to a specific date.
    let date = chrono::NaiveDate::from_ymd_opt(2026, 5, 25).unwrap();
    let scheduled = handle
        .update_task(TaskUpdate::new(task.id).schedule(Some(ScheduledFor::Date(date))))
        .await
        .unwrap();
    assert_eq!(scheduled.scheduled_for, Some(ScheduledFor::Date(date)));
    let _ = changes_rx.recv().await.unwrap();

    // Move to Someday.
    let someday = handle
        .update_task(TaskUpdate::new(task.id).schedule(Some(ScheduledFor::Someday)))
        .await
        .unwrap();
    assert_eq!(someday.scheduled_for, Some(ScheduledFor::Someday));
    let _ = changes_rx.recv().await.unwrap();

    // Clear it back to Inbox-equivalent.
    let cleared = handle
        .update_task(TaskUpdate::new(task.id).schedule(None))
        .await
        .unwrap();
    assert_eq!(cleared.scheduled_for, None);
}

#[tokio::test]
async fn update_task_sets_and_clears_deadline() {
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle
        .create_task(NewTask::inbox("by friday"))
        .await
        .unwrap();
    let _ = changes_rx.recv().await.unwrap();

    let date = chrono::NaiveDate::from_ymd_opt(2026, 6, 5).unwrap();
    let with_dl = handle
        .update_task(TaskUpdate::new(task.id).deadline_value(Some(date)))
        .await
        .unwrap();
    assert_eq!(with_dl.deadline, Some(date));
    let _ = changes_rx.recv().await.unwrap();

    let cleared = handle
        .update_task(TaskUpdate::new(task.id).deadline_value(None))
        .await
        .unwrap();
    assert_eq!(cleared.deadline, None);
}

#[tokio::test]
async fn update_task_sets_and_clears_defer_until() {
    // Phase 11 — defer_until set/clear round-trip via the
    // TaskUpdate::defer_value builder.
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle
        .create_task(NewTask::inbox("deferred"))
        .await
        .unwrap();
    let _ = changes_rx.recv().await.unwrap();

    let date = chrono::NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
    let with_defer = handle
        .update_task(TaskUpdate::new(task.id).defer_value(Some(date)))
        .await
        .unwrap();
    assert_eq!(with_defer.defer_until, Some(date));
    let _ = changes_rx.recv().await.unwrap();

    let cleared = handle
        .update_task(TaskUpdate::new(task.id).defer_value(None))
        .await
        .unwrap();
    assert_eq!(cleared.defer_until, None);
}

#[tokio::test]
async fn update_task_sets_and_clears_estimated_minutes() {
    // Phase 11 — estimated_minutes set/clear via the
    // TaskUpdate::estimated_minutes_value builder.
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle.create_task(NewTask::inbox("timed")).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();

    let with_est = handle
        .update_task(TaskUpdate::new(task.id).estimated_minutes_value(Some(45)))
        .await
        .unwrap();
    assert_eq!(with_est.estimated_minutes, Some(45));
    let _ = changes_rx.recv().await.unwrap();

    let cleared = handle
        .update_task(TaskUpdate::new(task.id).estimated_minutes_value(None))
        .await
        .unwrap();
    assert_eq!(cleared.estimated_minutes, None);
}

// v0.17.0 — Phase 18.5 Tier-1 CLOCK time tracking. Single-
// active-clock invariant + clock_out idempotency + delete-cascade.
#[tokio::test]
async fn clock_in_creates_open_entry() {
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle.create_task(NewTask::inbox("work")).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();
    let entry = handle.clock_in(task.id, String::new()).await.unwrap();
    assert_eq!(entry.task_id, task.id);
    assert!(entry.is_running());
    assert!(entry.note.is_empty());
}

#[tokio::test]
async fn clock_in_on_different_task_auto_closes_previous() {
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task_a = handle.create_task(NewTask::inbox("a")).await.unwrap();
    let task_b = handle.create_task(NewTask::inbox("b")).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();
    let _ = changes_rx.recv().await.unwrap();

    let _opened_a = handle.clock_in(task_a.id, String::new()).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();

    // Now clock in on B. The single-active-clock invariant
    // closes A's running entry before opening B's.
    let opened_b = handle.clock_in(task_b.id, String::new()).await.unwrap();
    assert!(opened_b.is_running());
    let _ = changes_rx.recv().await.unwrap();

    // A's old entry was auto-closed; clock_out on A is now a
    // no-op (returns None).
    let nothing = handle.clock_out(task_a.id).await.unwrap();
    assert!(
        nothing.is_none(),
        "task A's clock should already be closed by the auto-close"
    );
}

#[tokio::test]
async fn clock_out_is_idempotent_on_no_open_clock() {
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle.create_task(NewTask::inbox("t")).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();
    let result = handle.clock_out(task.id).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn clock_in_then_out_records_duration() {
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle.create_task(NewTask::inbox("t")).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();
    handle.clock_in(task.id, String::new()).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();
    let closed = handle.clock_out(task.id).await.unwrap().unwrap();
    assert!(closed.ended_at.is_some());
    // Duration is at least 0 (the test runs in microseconds; the
    // i64 minutes math floors to 0).
    assert!(closed.duration_minutes().unwrap() >= 0);
}

// v0.20.0 — Phase 19.5 reminder_at set/clear + next_pending_reminder ordering.
#[tokio::test]
async fn update_task_sets_and_clears_reminder_at() {
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle.create_task(NewTask::inbox("ping me")).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();

    let when = chrono::Utc::now() + chrono::Duration::hours(1);
    let with_reminder = handle
        .update_task(TaskUpdate::new(task.id).reminder_at_value(Some(when)))
        .await
        .unwrap();
    assert_eq!(with_reminder.reminder_at, Some(when));
    let _ = changes_rx.recv().await.unwrap();

    let cleared = handle
        .update_task(TaskUpdate::new(task.id).reminder_at_value(None))
        .await
        .unwrap();
    assert_eq!(cleared.reminder_at, None);
}

// Detailed next_pending_reminder ordering coverage lives in
// read.rs's test module (it can construct a Connection with
// canned inserts directly).

// v0.19.0 — Phase 18.5 Tier-2 scheduled_time set/clear.
#[tokio::test]
async fn update_task_sets_and_clears_scheduled_time() {
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle.create_task(NewTask::inbox("standup")).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();

    let t = chrono::NaiveTime::from_hms_opt(9, 30, 0).unwrap();
    let with_time = handle
        .update_task(TaskUpdate::new(task.id).scheduled_time_value(Some(t)))
        .await
        .unwrap();
    assert_eq!(with_time.scheduled_time, Some(t));
    let _ = changes_rx.recv().await.unwrap();

    let cleared = handle
        .update_task(TaskUpdate::new(task.id).scheduled_time_value(None))
        .await
        .unwrap();
    assert_eq!(cleared.scheduled_time, None);
}

// v0.18.0 — Phase 18.5 Tier-1 Quick Entry templates.
#[tokio::test]
async fn create_quick_entry_template_round_trips() {
    use crate::NewQuickEntryTemplate;
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let template = handle
        .create_quick_entry_template(NewQuickEntryTemplate {
            name: "Capture".into(),
            shortcut_key: Some("c".into()),
            target_project_id: None,
            prefix: "[capture] ".into(),
            default_tags: vec!["work".into(), "focus".into()],
        })
        .await
        .unwrap();
    assert_eq!(template.name, "Capture");
    assert_eq!(template.shortcut_key.as_deref(), Some("c"));
    assert_eq!(template.prefix, "[capture] ");
    assert_eq!(
        template.default_tags,
        vec!["work".to_string(), "focus".to_string()]
    );
}

#[tokio::test]
async fn create_quick_entry_template_rejects_multi_char_shortcut() {
    use crate::NewQuickEntryTemplate;
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let err = handle
        .create_quick_entry_template(NewQuickEntryTemplate {
            name: "Bad".into(),
            shortcut_key: Some("xy".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    // Surfaces as DbError::Domain via the InvalidShortcutKey variant.
    let msg = format!("{err}");
    assert!(
        msg.contains("shortcut_key"),
        "expected shortcut_key error; got: {msg}"
    );
}

#[tokio::test]
async fn create_quick_entry_template_rejects_non_alnum_shortcut() {
    use crate::NewQuickEntryTemplate;
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let err = handle
        .create_quick_entry_template(NewQuickEntryTemplate {
            name: "Bad".into(),
            shortcut_key: Some("!".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("shortcut_key"), "got: {msg}");
}

#[tokio::test]
async fn delete_quick_entry_template_removes_row() {
    use crate::NewQuickEntryTemplate;
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let template = handle
        .create_quick_entry_template(NewQuickEntryTemplate {
            name: "Tmp".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    handle
        .delete_quick_entry_template(template.id)
        .await
        .unwrap();
    let err = handle
        .delete_quick_entry_template(template.id)
        .await
        .unwrap_err();
    // Already deleted → NotFound.
    assert!(matches!(err, crate::DbError::NotFound), "got: {err:?}");
}

#[tokio::test]
async fn update_quick_entry_template_changes_fields() {
    use crate::{NewQuickEntryTemplate, QuickEntryTemplateUpdate};
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let template = handle
        .create_quick_entry_template(NewQuickEntryTemplate {
            name: "Original".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let updated = handle
        .update_quick_entry_template(
            QuickEntryTemplateUpdate::new(template.id)
                .name("Renamed")
                .shortcut_key(Some("r".into()))
                .prefix("[r] ")
                .default_tags(vec!["a".into(), "b".into()]),
        )
        .await
        .unwrap();
    assert_eq!(updated.name, "Renamed");
    assert_eq!(updated.shortcut_key.as_deref(), Some("r"));
    assert_eq!(updated.prefix, "[r] ");
    assert_eq!(updated.default_tags, vec!["a".to_string(), "b".to_string()]);
}

#[tokio::test]
async fn update_task_sets_and_clears_deadline_warn_days() {
    // v0.14.0 — Phase 18.5 Tier-1: per-task DEADLINE warning
    // window set/clear via the TaskUpdate builder. NULL is the
    // "fall back to global default" sentinel.
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle
        .create_task(NewTask::inbox("sensitive"))
        .await
        .unwrap();
    let _ = changes_rx.recv().await.unwrap();
    assert_eq!(task.deadline_warn_days, None);

    let with_warn = handle
        .update_task(TaskUpdate::new(task.id).deadline_warn_days_value(Some(14)))
        .await
        .unwrap();
    assert_eq!(with_warn.deadline_warn_days, Some(14));
    let _ = changes_rx.recv().await.unwrap();

    let cleared = handle
        .update_task(TaskUpdate::new(task.id).deadline_warn_days_value(None))
        .await
        .unwrap();
    assert_eq!(cleared.deadline_warn_days, None);
}

#[tokio::test]
async fn update_task_sets_and_clears_repeat_rule() {
    // Phase 15 — repeat_rule + repeat_mode set/clear via the
    // TaskUpdate builder. Validates that round-trip survives.
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle.create_task(NewTask::inbox("repeat")).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();

    let with_rule = handle
        .update_task(
            TaskUpdate::new(task.id)
                .repeat_rule_value(Some("FREQ=WEEKLY".into()))
                .repeat_mode_value(Some("NEXT".into())),
        )
        .await
        .unwrap();
    assert_eq!(with_rule.repeat_rule.as_deref(), Some("FREQ=WEEKLY"));
    assert_eq!(with_rule.repeat_mode.as_deref(), Some("NEXT"));
    let _ = changes_rx.recv().await.unwrap();

    let cleared = handle
        .update_task(
            TaskUpdate::new(task.id)
                .repeat_rule_value(None)
                .repeat_mode_value(None),
        )
        .await
        .unwrap();
    assert!(cleared.repeat_rule.is_none());
    assert!(cleared.repeat_mode.is_none());
}

#[tokio::test]
async fn update_task_rejects_malformed_repeat_rule() {
    // Phase 15 — bad RRULE text is rejected up front.
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle.create_task(NewTask::inbox("bad")).await.unwrap();
    let result = handle
        .update_task(TaskUpdate::new(task.id).repeat_rule_value(Some("not a rrule".into())))
        .await;
    match result {
        Err(DbError::BadRepeatRule(_)) => {}
        other => panic!("expected BadRepeatRule, got {other:?}"),
    }
}

#[tokio::test]
async fn create_task_rejects_malformed_repeat_rule() {
    // Phase 15 — same validation runs on insert.
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let result = handle
        .create_task(NewTask {
            title: "bad".into(),
            repeat_rule: Some("FREQ=GARBAGE".into()),
            ..Default::default()
        })
        .await;
    match result {
        Err(DbError::BadRepeatRule(_)) => {}
        other => panic!("expected BadRepeatRule, got {other:?}"),
    }
}

#[tokio::test]
async fn complete_repeating_task_spawns_next_instance() {
    // Phase 15 — completing a task with a repeat_rule spawns a
    // follow-up with shifted scheduled_for. The original stays
    // completed; the new instance is open with the next date.
    use crate::domain::ScheduledFor;
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let original = handle
        .create_task(NewTask {
            title: "weekly dishes".into(),
            scheduled_for: Some(ScheduledFor::Date(
                chrono::NaiveDate::from_ymd_opt(2026, 1, 5).unwrap(),
            )),
            repeat_rule: Some("FREQ=WEEKLY".into()),
            repeat_mode: Some("CUMULATIVE".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    let _ = changes_rx.recv().await.unwrap();

    let toggled = handle.toggle_complete(original.id).await.unwrap();
    assert!(toggled.is_completed());
    let changes = changes_rx.recv().await.unwrap();
    // Toggled appears in updated; new instance appears in created.
    assert_eq!(changes.updated.len(), 1);
    assert_eq!(changes.created.len(), 1);
    assert_eq!(changes.status_changed, vec![original.id]);

    let next = &changes.created[0];
    assert_ne!(next.id, original.id);
    assert!(next.completed_at.is_none());
    assert_eq!(next.title, "weekly dishes");
    assert_eq!(next.repeat_rule.as_deref(), Some("FREQ=WEEKLY"));
    // Cumulative jump from 2026-01-05 with completion ~today
    // (2026-05-07 in this conversation) skips weeks ahead, so
    // next.scheduled_for is strictly after both 2026-01-05 and
    // today. Only assert the type + future-ness, not the exact
    // date (today moves forward as the test environment ages).
    match next.scheduled_for {
        Some(ScheduledFor::Date(d)) => {
            assert!(d > chrono::NaiveDate::from_ymd_opt(2026, 1, 5).unwrap());
        }
        _ => panic!(
            "expected Date schedule on follow-up, got {:?}",
            next.scheduled_for
        ),
    }
}

#[tokio::test]
async fn complete_repeating_task_preserves_project_membership() {
    // Phase 15 — the spawned follow-up inherits project / parent
    // / note / repeat_rule / repeat_mode. Tag carry-forward is
    // covered by the SQL-level test in `db::read::tests` (the
    // tag map join exercises the same row).
    use crate::domain::{NewProject, ScheduledFor};
    let (handle, mut changes_rx, mut library_rx) = spawn(fresh_conn());
    let project = handle
        .create_project(NewProject {
            title: "groceries".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();

    let original = handle
        .create_task(NewTask {
            title: "shop".into(),
            note: "milk + eggs".into(),
            project_id: Some(project.id),
            scheduled_for: Some(ScheduledFor::Date(
                chrono::NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
            )),
            repeat_rule: Some("FREQ=DAILY".into()),
            repeat_mode: Some("NEXT".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    let _ = changes_rx.recv().await.unwrap();

    let _ = handle.toggle_complete(original.id).await.unwrap();
    let changes = changes_rx.recv().await.unwrap();
    let next = &changes.created[0];
    assert_eq!(next.project_id, Some(project.id));
    assert_eq!(next.note, "milk + eggs");
    assert_eq!(next.repeat_rule.as_deref(), Some("FREQ=DAILY"));
    assert_eq!(next.repeat_mode.as_deref(), Some("NEXT"));
}

#[tokio::test]
async fn complete_non_repeating_task_does_not_spawn() {
    // Phase 15 — sanity check: a task without repeat_rule
    // toggles cleanly without producing a created delta.
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle
        .create_task(NewTask::inbox("one-shot"))
        .await
        .unwrap();
    let _ = changes_rx.recv().await.unwrap();

    let _ = handle.toggle_complete(task.id).await.unwrap();
    let changes = changes_rx.recv().await.unwrap();
    assert!(changes.created.is_empty());
    assert_eq!(changes.updated.len(), 1);
    assert_eq!(changes.status_changed, vec![task.id]);
}

#[tokio::test]
async fn complete_repeating_task_with_count_terminator() {
    // Phase 15 — COUNT=2 means the original is occurrence 1,
    // the spawned follow-up is occurrence 2. Completing the
    // follow-up exhausts the rule and produces no further
    // instance.
    //
    // Use BASIC mode so the test is anchor-relative and doesn't
    // depend on what today's date is when the test runs (CI
    // could be days, months, or years past the synthetic
    // anchor — CUMULATIVE would skip past all in-rule
    // occurrences in that case and report "no next occurrence"
    // even on the first cycle).
    use crate::domain::ScheduledFor;
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let original = handle
        .create_task(NewTask {
            title: "twice only".into(),
            scheduled_for: Some(ScheduledFor::Date(
                chrono::NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
            )),
            repeat_rule: Some("FREQ=DAILY;COUNT=2".into()),
            repeat_mode: Some("BASIC".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    let _ = changes_rx.recv().await.unwrap();

    // First completion → spawns occurrence 2.
    let _ = handle.toggle_complete(original.id).await.unwrap();
    let first_changes = changes_rx.recv().await.unwrap();
    assert_eq!(first_changes.created.len(), 1);
    let second = first_changes.created[0].clone();

    // Second completion → no further occurrences.
    let _ = handle.toggle_complete(second.id).await.unwrap();
    let second_changes = changes_rx.recv().await.unwrap();
    assert!(
        second_changes.created.is_empty(),
        "COUNT=2 rule should not spawn a third instance"
    );
}

#[tokio::test]
async fn weekly_repeat_survives_one_year_horizon() {
    // Phase 15 — synthetic 52-week horizon. Complete a weekly
    // task one cycle at a time and check it produces the right
    // sequence of dates. Uses BASIC mode so the test is anchor-
    // relative regardless of when CI runs.
    use crate::domain::ScheduledFor;
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let start = chrono::NaiveDate::from_ymd_opt(2026, 1, 5).unwrap(); // Mon
    let mut current = handle
        .create_task(NewTask {
            title: "weekly".into(),
            scheduled_for: Some(ScheduledFor::Date(start)),
            repeat_rule: Some("FREQ=WEEKLY".into()),
            repeat_mode: Some("BASIC".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    let _ = changes_rx.recv().await.unwrap();

    for week in 1..=52 {
        let _ = handle.toggle_complete(current.id).await.unwrap();
        let changes = changes_rx.recv().await.unwrap();
        assert_eq!(
            changes.created.len(),
            1,
            "week {week}: expected a follow-up to spawn"
        );
        let next = &changes.created[0];
        let expected_date = start + chrono::Duration::weeks(week as i64);
        match next.scheduled_for {
            Some(ScheduledFor::Date(d)) => assert_eq!(
                d, expected_date,
                "week {week}: expected {expected_date}, got {d}"
            ),
            _ => panic!("week {week}: missing schedule"),
        }
        current = next.clone();
    }
}

#[tokio::test]
async fn monthly_repeat_skips_short_months_at_end_of_month() {
    // Phase 15 — Jan 31 + monthly: Feb has no 31, RFC 5545
    // skips the month rather than clamp. Worker carries the
    // shifted date forward whatever rrule decides.
    use crate::domain::ScheduledFor;
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle
        .create_task(NewTask {
            title: "month-end".into(),
            scheduled_for: Some(ScheduledFor::Date(
                chrono::NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
            )),
            repeat_rule: Some("FREQ=MONTHLY".into()),
            repeat_mode: Some("BASIC".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    let _ = changes_rx.recv().await.unwrap();

    let _ = handle.toggle_complete(task.id).await.unwrap();
    let changes = changes_rx.recv().await.unwrap();
    let next = &changes.created[0];
    match next.scheduled_for {
        Some(ScheduledFor::Date(d)) => assert_eq!(
            d,
            chrono::NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
            "Feb skipped, next is March 31"
        ),
        _ => panic!("missing schedule"),
    }
}

#[tokio::test]
async fn reopen_does_not_spawn_follow_up() {
    // Phase 15 — toggling a *completed* task to open is a pure
    // reopen, never a regenerate.
    use crate::domain::ScheduledFor;
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle
        .create_task(NewTask {
            title: "weekly".into(),
            scheduled_for: Some(ScheduledFor::Date(
                chrono::NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
            )),
            repeat_rule: Some("FREQ=WEEKLY".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    let _ = changes_rx.recv().await.unwrap();

    let _ = handle.toggle_complete(task.id).await.unwrap(); // complete (spawns)
    let _ = changes_rx.recv().await.unwrap();
    let _ = handle.toggle_complete(task.id).await.unwrap(); // reopen
    let reopen_changes = changes_rx.recv().await.unwrap();
    assert!(
        reopen_changes.created.is_empty(),
        "reopening should not spawn a new instance"
    );
}

#[tokio::test]
async fn toggle_complete_flips_completed_at() {
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle.create_task(NewTask::inbox("flip me")).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();

    let completed = handle.toggle_complete(task.id).await.unwrap();
    assert!(completed.is_completed());

    let changes = changes_rx.recv().await.unwrap();
    assert_eq!(changes.status_changed, vec![task.id]);
    assert_eq!(changes.updated.len(), 1);

    let reopened = handle.toggle_complete(task.id).await.unwrap();
    assert!(!reopened.is_completed());

    let changes = changes_rx.recv().await.unwrap();
    assert_eq!(changes.status_changed, vec![task.id]);
}

#[tokio::test]
async fn delete_task_emits_deleted_id() {
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle.create_task(NewTask::inbox("doomed")).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();

    handle.delete_task(task.id).await.unwrap();
    let changes = changes_rx.recv().await.unwrap();
    assert_eq!(changes.deleted, vec![task.id]);
}

#[tokio::test]
async fn delete_missing_returns_not_found() {
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let result = handle.delete_task(9999).await;
    assert!(matches!(result, Err(DbError::NotFound)));
}

#[tokio::test]
async fn worker_shuts_down_when_handle_dropped() {
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    drop(handle);
    let result = tokio::time::timeout(Duration::from_secs(1), changes_rx.recv()).await;
    assert!(matches!(result, Ok(None)));
}

#[tokio::test]
async fn position_increments_for_inbox_tasks() {
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let a = handle.create_task(NewTask::inbox("a")).await.unwrap();
    let b = handle.create_task(NewTask::inbox("b")).await.unwrap();
    let c = handle.create_task(NewTask::inbox("c")).await.unwrap();
    assert!(a.position < b.position);
    assert!(b.position < c.position);
}

#[tokio::test]
async fn create_with_someday_round_trips() {
    use crate::domain::ScheduledFor;
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle
        .create_task(NewTask {
            title: "later".into(),
            scheduled_for: Some(ScheduledFor::Someday),
            ..NewTask::default()
        })
        .await
        .unwrap();
    assert_eq!(task.scheduled_for, Some(ScheduledFor::Someday));
}

// ── Phase 5b: areas / projects ─────────────────────────────────

#[tokio::test]
async fn create_area_emits_library_change() {
    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let area = handle
        .create_area(NewArea {
            title: "Personal".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(area.title, "Personal");

    let lib = library_rx.recv().await.unwrap();
    assert_eq!(lib.areas_created.len(), 1);
    assert_eq!(lib.areas_created[0].id, area.id);
}

#[tokio::test]
async fn rename_area_round_trip() {
    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let area = handle
        .create_area(NewArea {
            title: "Old".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();
    let renamed = handle
        .update_area(AreaUpdate::new(area.id).title("New"))
        .await
        .unwrap();
    assert_eq!(renamed.title, "New");
    let lib = library_rx.recv().await.unwrap();
    assert_eq!(lib.areas_updated.len(), 1);
}

#[tokio::test]
async fn area_default_review_interval_round_trips() {
    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    // Create with a default cadence set.
    let area = handle
        .create_area(NewArea {
            title: "Work".into(),
            default_review_interval_days: Some(7),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(area.default_review_interval_days, Some(7));
    let _ = library_rx.recv().await.unwrap();

    // Update it to a different value.
    let updated = handle
        .update_area(AreaUpdate::new(area.id).default_review_interval_days(Some(14)))
        .await
        .unwrap();
    assert_eq!(updated.default_review_interval_days, Some(14));
    let _ = library_rx.recv().await.unwrap();

    // Clear it back to NULL via Some(None).
    let cleared = handle
        .update_area(AreaUpdate::new(area.id).default_review_interval_days(None))
        .await
        .unwrap();
    assert_eq!(cleared.default_review_interval_days, None);
}

// ── Task dependencies (v0.29.0) ─────────────────────────────────

#[tokio::test]
async fn add_dependency_round_trip_and_duplicate_is_noop() {
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let a = handle.create_task(NewTask::inbox("a")).await.unwrap();
    let b = handle.create_task(NewTask::inbox("b")).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();
    let _ = changes_rx.recv().await.unwrap();

    handle.add_dependency(a.id, b.id).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();
    // Duplicate edge is absorbed by the UNIQUE constraint, not an error.
    handle.add_dependency(a.id, b.id).await.unwrap();
}

#[tokio::test]
async fn self_dependency_is_rejected() {
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let a = handle.create_task(NewTask::inbox("a")).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();
    let err = handle.add_dependency(a.id, a.id).await.unwrap_err();
    assert!(matches!(
        err,
        DbError::Domain(crate::error::DomainError::DependencyCycle { .. })
    ));
}

#[tokio::test]
async fn direct_dependency_cycle_is_rejected() {
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let a = handle.create_task(NewTask::inbox("a")).await.unwrap();
    let b = handle.create_task(NewTask::inbox("b")).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();
    let _ = changes_rx.recv().await.unwrap();
    // a blocked by b is fine; b blocked by a would close a cycle.
    handle.add_dependency(a.id, b.id).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();
    let err = handle.add_dependency(b.id, a.id).await.unwrap_err();
    assert!(matches!(
        err,
        DbError::Domain(crate::error::DomainError::DependencyCycle { .. })
    ));
}

#[tokio::test]
async fn transitive_dependency_cycle_is_rejected() {
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let a = handle.create_task(NewTask::inbox("a")).await.unwrap();
    let b = handle.create_task(NewTask::inbox("b")).await.unwrap();
    let c = handle.create_task(NewTask::inbox("c")).await.unwrap();
    for _ in 0..3 {
        let _ = changes_rx.recv().await.unwrap();
    }
    // a → b → c (a depends on b, b depends on c). Closing c → a is a cycle.
    handle.add_dependency(a.id, b.id).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();
    handle.add_dependency(b.id, c.id).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();
    let err = handle.add_dependency(c.id, a.id).await.unwrap_err();
    assert!(matches!(
        err,
        DbError::Domain(crate::error::DomainError::DependencyCycle { .. })
    ));
}

#[tokio::test]
async fn remove_dependency_is_noop_when_absent() {
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let a = handle.create_task(NewTask::inbox("a")).await.unwrap();
    let b = handle.create_task(NewTask::inbox("b")).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();
    let _ = changes_rx.recv().await.unwrap();
    // No edge yet — removing one is a clean no-op.
    handle.remove_dependency(a.id, b.id).await.unwrap();
}

#[tokio::test]
async fn delete_area_unfiles_projects() {
    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let area = handle
        .create_area(NewArea {
            title: "Soon Gone".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();
    let project = handle
        .create_project(NewProject::in_area("Filed", area.id))
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();
    assert_eq!(project.area_id, Some(area.id));

    handle.delete_area(area.id).await.unwrap();
    let lib = library_rx.recv().await.unwrap();
    assert_eq!(lib.areas_deleted, vec![area.id]);
    assert_eq!(lib.projects_updated.len(), 1, "FK SET NULL fired");
    assert!(lib.projects_updated[0].area_id.is_none());
}

#[tokio::test]
async fn create_project_round_trip() {
    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let project = handle
        .create_project(NewProject::unfiled("Q3"))
        .await
        .unwrap();
    assert_eq!(project.title, "Q3");
    assert!(project.area_id.is_none());
    assert!(!project.sequential);
    let lib = library_rx.recv().await.unwrap();
    assert_eq!(lib.projects_created.len(), 1);
}

#[tokio::test]
async fn mark_reviewed_stamps_last_reviewed_at_and_emits_library_change() {
    // Phase 13 — Review queue's Mark Reviewed handler.
    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let project = handle
        .create_project(NewProject::unfiled("Quarterly OKRs"))
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();
    assert!(project.last_reviewed_at.is_none());

    let reviewed = handle.mark_reviewed(project.id).await.unwrap();
    assert!(reviewed.last_reviewed_at.is_some());
    assert_eq!(reviewed.id, project.id);

    let lib = library_rx.recv().await.unwrap();
    assert_eq!(lib.projects_updated.len(), 1);
    assert_eq!(lib.projects_updated[0].id, project.id);
    assert!(lib.projects_updated[0].last_reviewed_at.is_some());
}

#[tokio::test]
async fn mark_reviewed_unknown_id_is_not_found() {
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let result = handle.mark_reviewed(9999).await;
    assert!(matches!(result, Err(DbError::NotFound)));
}

#[tokio::test]
async fn mark_task_reviewed_stamps_last_reviewed_at_and_emits_task_change() {
    // task-level Mark Reviewed handler.
    let (handle, mut changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle
        .create_task(NewTask::inbox("Audit the API"))
        .await
        .unwrap();
    let _ = changes_rx.recv().await.unwrap();
    assert!(task.last_reviewed_at.is_none());

    let reviewed = handle.mark_task_reviewed(task.id).await.unwrap();
    assert!(reviewed.last_reviewed_at.is_some());
    assert_eq!(reviewed.id, task.id);

    let changes = changes_rx.recv().await.unwrap();
    assert_eq!(changes.updated.len(), 1);
    assert_eq!(changes.updated[0].id, task.id);
    assert!(changes.updated[0].last_reviewed_at.is_some());
}

#[tokio::test]
async fn mark_task_reviewed_unknown_id_is_not_found() {
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let result = handle.mark_task_reviewed(9999).await;
    assert!(matches!(result, Err(DbError::NotFound)));
}

#[tokio::test]
async fn archive_project_completes_open_tasks() {
    let (handle, mut changes_rx, mut library_rx) = spawn(fresh_conn());
    let project = handle
        .create_project(NewProject::unfiled("Almost done"))
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();
    let mut new = NewTask::inbox("an open task");
    new.project_id = Some(project.id);
    let _t = handle.create_task(new).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();

    let archived = handle.archive_project(project.id).await.unwrap();
    assert!(archived.archived_at.is_some());
    let lib = library_rx.recv().await.unwrap();
    assert_eq!(lib.projects_updated.len(), 1);
    let task_changes = changes_rx.recv().await.unwrap();
    assert_eq!(task_changes.status_changed.len(), 1);
    assert_eq!(task_changes.updated.len(), 1);
    assert!(task_changes.updated[0].is_completed());
}

#[tokio::test]
async fn delete_project_cascades_tasks() {
    let (handle, mut changes_rx, mut library_rx) = spawn(fresh_conn());
    let project = handle
        .create_project(NewProject::unfiled("Doomed"))
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();
    let mut new = NewTask::inbox("orphan-to-be");
    new.project_id = Some(project.id);
    let _t = handle.create_task(new).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();

    handle.delete_project(project.id).await.unwrap();
    let lib = library_rx.recv().await.unwrap();
    assert_eq!(lib.projects_deleted, vec![project.id]);
    let task_changes = changes_rx.recv().await.unwrap();
    assert_eq!(task_changes.deleted.len(), 1);
}

// ── Phase 6a: tags ────────────────────────────────────────────

#[tokio::test]
async fn create_tag_round_trip() {
    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let tag = handle
        .create_tag(NewTag {
            name: "errand".into(),
            color: None,
        })
        .await
        .unwrap();
    assert_eq!(tag.name, "errand");
    let lib = library_rx.recv().await.unwrap();
    assert_eq!(lib.tags_created.len(), 1);
}

#[tokio::test]
async fn rename_tag_round_trip() {
    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let tag = handle
        .create_tag(NewTag {
            name: "old".into(),
            color: None,
        })
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();
    let renamed = handle
        .update_tag(TagUpdate::new(tag.id).name("new"))
        .await
        .unwrap();
    assert_eq!(renamed.name, "new");
    let lib = library_rx.recv().await.unwrap();
    assert_eq!(lib.tags_updated.len(), 1);
}

#[tokio::test]
async fn delete_tag_emits_library_change() {
    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let tag = handle
        .create_tag(NewTag {
            name: "doomed".into(),
            color: None,
        })
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();
    handle.delete_tag(tag.id).await.unwrap();
    let lib = library_rx.recv().await.unwrap();
    assert_eq!(lib.tags_deleted, vec![tag.id]);
}

// ── Perspectives (Phase 14) ────────────────────────────────

#[tokio::test]
async fn create_perspective_round_trip_emits_library_change() {
    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let p = handle
        .create_perspective(NewPerspective {
            name: "Q3 work overdue".into(),
            icon: None,
            filter_expr: "tag:work due:overdue".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(p.name, "Q3 work overdue");
    assert_eq!(p.filter_expr, "tag:work due:overdue");
    assert!(p.icon.is_none());
    assert!(!p.uuid.is_empty());

    let lib = library_rx.recv().await.unwrap();
    assert_eq!(lib.perspectives_created.len(), 1);
    assert_eq!(lib.perspectives_created[0].id, p.id);
}

#[tokio::test]
async fn update_perspective_round_trip() {
    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let p = handle
        .create_perspective(NewPerspective {
            name: "Old name".into(),
            icon: None,
            filter_expr: "tag:work".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();

    let renamed = handle
        .update_perspective(
            PerspectiveUpdate::new(p.id)
                .name("New name")
                .filter_expr("tag:work is:overdue"),
        )
        .await
        .unwrap();
    assert_eq!(renamed.name, "New name");
    assert_eq!(renamed.filter_expr, "tag:work is:overdue");
    let lib = library_rx.recv().await.unwrap();
    assert_eq!(lib.perspectives_updated.len(), 1);
}

#[tokio::test]
async fn delete_perspective_emits_library_change() {
    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let p = handle
        .create_perspective(NewPerspective {
            name: "Doomed".into(),
            icon: None,
            filter_expr: "is:done".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();

    handle.delete_perspective(p.id).await.unwrap();
    let lib = library_rx.recv().await.unwrap();
    assert_eq!(lib.perspectives_deleted, vec![p.id]);
}

#[tokio::test]
async fn duplicate_tag_name_rejected() {
    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let _ = handle
        .create_tag(NewTag {
            name: "Errand".into(),
            color: None,
        })
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();
    // Schema enforces NOCASE-unique; "errand" should collide.
    let result = handle
        .create_tag(NewTag {
            name: "errand".into(),
            color: None,
        })
        .await;
    assert!(result.is_err(), "duplicate tag name should fail");
}

#[tokio::test]
async fn move_task_to_project_via_update_task() {
    let (handle, mut changes_rx, mut library_rx) = spawn(fresh_conn());
    let project = handle
        .create_project(NewProject::unfiled("Target"))
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();
    let task = handle.create_task(NewTask::inbox("orphan")).await.unwrap();
    let _ = changes_rx.recv().await.unwrap();
    assert!(task.project_id.is_none());

    let moved = handle
        .update_task(TaskUpdate::new(task.id).project(Some(project.id)))
        .await
        .unwrap();
    assert_eq!(moved.project_id, Some(project.id));
}

// ── Domain invariants ─────────────────────────────────────

#[tokio::test]
async fn create_task_rejects_cross_project_parent() {
    use crate::error::DomainError;

    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let p1 = handle
        .create_project(NewProject::unfiled("P1"))
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();
    let p2 = handle
        .create_project(NewProject::unfiled("P2"))
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();
    let parent = handle
        .create_task(NewTask {
            title: "Parent".into(),
            project_id: Some(p1.id),
            ..Default::default()
        })
        .await
        .unwrap();

    let result = handle
        .create_task(NewTask {
            title: "Cross-project child".into(),
            project_id: Some(p2.id),
            parent_id: Some(parent.id),
            ..Default::default()
        })
        .await;

    match result {
        Err(DbError::Domain(DomainError::ParentProjectMismatch {
            parent_task,
            parent_project,
            claimed_project,
        })) => {
            assert_eq!(parent_task, parent.id);
            assert_eq!(parent_project, Some(p1.id));
            assert_eq!(claimed_project, Some(p2.id));
        }
        other => panic!("expected ParentProjectMismatch, got: {other:?}"),
    }
}

#[tokio::test]
async fn create_task_accepts_same_project_parent() {
    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let project = handle
        .create_project(NewProject::unfiled("P"))
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();
    let parent = handle
        .create_task(NewTask {
            title: "Parent".into(),
            project_id: Some(project.id),
            ..Default::default()
        })
        .await
        .unwrap();
    let child = handle
        .create_task(NewTask {
            title: "Child".into(),
            project_id: Some(project.id),
            parent_id: Some(parent.id),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(child.parent_id, Some(parent.id));
}

#[tokio::test]
async fn update_task_rejects_move_orphaning_parent() {
    use crate::error::DomainError;

    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let p1 = handle
        .create_project(NewProject::unfiled("P1"))
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();
    let p2 = handle
        .create_project(NewProject::unfiled("P2"))
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();

    let parent = handle
        .create_task(NewTask {
            title: "Parent".into(),
            project_id: Some(p1.id),
            ..Default::default()
        })
        .await
        .unwrap();
    let child = handle
        .create_task(NewTask {
            title: "Child".into(),
            project_id: Some(p1.id),
            parent_id: Some(parent.id),
            ..Default::default()
        })
        .await
        .unwrap();

    // Try to move just the child to p2 — would orphan it across
    // the project boundary from its parent.
    let result = handle
        .update_task(TaskUpdate::new(child.id).project(Some(p2.id)))
        .await;

    assert!(matches!(
        result,
        Err(DbError::Domain(DomainError::ParentProjectMismatch { .. }))
    ));
}

#[tokio::test]
async fn update_task_reparents_within_project() {
    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let project = handle
        .create_project(NewProject::unfiled("P"))
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();
    let parent = handle
        .create_task(NewTask {
            title: "Parent".into(),
            project_id: Some(project.id),
            ..Default::default()
        })
        .await
        .unwrap();
    let loose = handle
        .create_task(NewTask {
            title: "Loose".into(),
            project_id: Some(project.id),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(loose.parent_id, None);

    let reparented = handle
        .update_task(TaskUpdate::new(loose.id).reparent(Some(parent.id)))
        .await
        .unwrap();
    assert_eq!(reparented.parent_id, Some(parent.id));

    // Promote back to top level.
    let promoted = handle
        .update_task(TaskUpdate::new(loose.id).reparent(None))
        .await
        .unwrap();
    assert_eq!(promoted.parent_id, None);
}

#[tokio::test]
async fn update_task_rejects_self_parent() {
    use crate::error::DomainError;

    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let t = handle.create_task(NewTask::inbox("Self")).await.unwrap();
    let result = handle
        .update_task(TaskUpdate::new(t.id).reparent(Some(t.id)))
        .await;
    assert!(matches!(
        result,
        Err(DbError::Domain(DomainError::ParentCycle { .. }))
    ));
}

#[tokio::test]
async fn update_task_rejects_descendant_cycle() {
    use crate::error::DomainError;

    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    // a -> b -> c (c is grandchild of a). Reparenting a under c would
    // make a its own descendant.
    let a = handle.create_task(NewTask::inbox("A")).await.unwrap();
    let b = handle
        .create_task(NewTask {
            title: "B".into(),
            parent_id: Some(a.id),
            ..Default::default()
        })
        .await
        .unwrap();
    let c = handle
        .create_task(NewTask {
            title: "C".into(),
            parent_id: Some(b.id),
            ..Default::default()
        })
        .await
        .unwrap();
    let result = handle
        .update_task(TaskUpdate::new(a.id).reparent(Some(c.id)))
        .await;
    assert!(matches!(
        result,
        Err(DbError::Domain(DomainError::ParentCycle { .. }))
    ));
}

#[tokio::test]
async fn create_perspective_rejects_empty_filter() {
    use crate::domain::NewPerspective;
    use crate::error::DomainError;

    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let result = handle
        .create_perspective(NewPerspective {
            name: "Blank".into(),
            filter_expr: "   ".into(),
            ..Default::default()
        })
        .await;
    assert!(matches!(
        result,
        Err(DbError::Domain(DomainError::EmptyFilterExpr))
    ));
}

#[tokio::test]
async fn update_perspective_rejects_emptying_filter() {
    use crate::domain::{NewPerspective, PerspectiveUpdate};
    use crate::error::DomainError;

    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let p = handle
        .create_perspective(NewPerspective {
            name: "Real".into(),
            filter_expr: "is:open".into(),
            ..Default::default()
        })
        .await
        .unwrap();

    let result = handle
        .update_perspective(PerspectiveUpdate::new(p.id).filter_expr(""))
        .await;
    assert!(matches!(
        result,
        Err(DbError::Domain(DomainError::EmptyFilterExpr))
    ));
}

// ── ensure_heading (Phase 18 / v0.12.0) ──────────────────

#[tokio::test]
async fn ensure_heading_creates_when_absent() {
    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let project = handle
        .create_project(NewProject::unfiled("Errands"))
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();

    let h = handle
        .ensure_heading(project.id, "Sunday: Prep".to_string())
        .await
        .unwrap();
    assert_eq!(h.project_id, project.id);
    assert_eq!(h.title, "Sunday: Prep");
    assert!(h.position > 0.0);
}

#[tokio::test]
async fn ensure_heading_is_idempotent_per_project_and_title() {
    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let project = handle
        .create_project(NewProject::unfiled("Errands"))
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();

    let h1 = handle
        .ensure_heading(project.id, "Monday".to_string())
        .await
        .unwrap();
    let h2 = handle
        .ensure_heading(project.id, "monday".to_string()) // case-insensitive
        .await
        .unwrap();
    assert_eq!(h1.id, h2.id, "case-insensitive lookup must dedupe");
}

#[tokio::test]
async fn ensure_heading_scoped_to_project() {
    // Same heading title in two different projects should
    // produce two distinct headings.
    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let p1 = handle
        .create_project(NewProject::unfiled("Project A"))
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();
    let p2 = handle
        .create_project(NewProject::unfiled("Project B"))
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();

    let h1 = handle
        .ensure_heading(p1.id, "Backlog".to_string())
        .await
        .unwrap();
    let h2 = handle
        .ensure_heading(p2.id, "Backlog".to_string())
        .await
        .unwrap();
    assert_ne!(
        h1.id, h2.id,
        "headings in different projects must not collide"
    );
}

#[tokio::test]
async fn ensure_heading_increments_position() {
    let (handle, _changes_rx, mut library_rx) = spawn(fresh_conn());
    let project = handle
        .create_project(NewProject::unfiled("Errands"))
        .await
        .unwrap();
    let _ = library_rx.recv().await.unwrap();

    let h1 = handle
        .ensure_heading(project.id, "First".to_string())
        .await
        .unwrap();
    let h2 = handle
        .ensure_heading(project.id, "Second".to_string())
        .await
        .unwrap();
    assert!(
        h2.position > h1.position,
        "successive headings should sort after"
    );
}

#[tokio::test]
async fn create_task_persists_extra_properties() {
    // v0.24.0 — Post-v0.22.0 Tier 1. NewTask.extra_properties
    // round-trips through the JSON column.
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let mut extras = std::collections::BTreeMap::new();
    extras.insert("CATEGORY".to_string(), "Q3-deliverables".to_string());
    extras.insert("CLIENT".to_string(), "Acme Corp".to_string());
    let new = NewTask {
        title: "task with extras".to_string(),
        extra_properties: extras.clone(),
        ..Default::default()
    };
    let task = handle.create_task(new).await.unwrap();
    assert_eq!(task.extra_properties, extras);
}

#[tokio::test]
async fn create_task_empty_extras_round_trips_as_empty() {
    // Empty BTreeMap normalises to NULL in the column; the
    // read boundary turns it back into an empty map. The
    // distinction matters for diff-checking: a freshly
    // created task with no extras should be `==` to one
    // round-tripped through the column.
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let task = handle
        .create_task(NewTask {
            title: "no extras".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(task.extra_properties.is_empty());
}

#[tokio::test]
async fn update_task_replaces_extra_properties_whole_map() {
    // TaskUpdate.extra_properties is a whole-map replace —
    // calling extra_properties_value with a new map
    // overwrites everything in the column.
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let mut initial = std::collections::BTreeMap::new();
    initial.insert("CATEGORY".to_string(), "Old".to_string());
    initial.insert("CLIENT".to_string(), "Acme".to_string());
    let task = handle
        .create_task(NewTask {
            title: "to update".to_string(),
            extra_properties: initial,
            ..Default::default()
        })
        .await
        .unwrap();

    let mut replacement = std::collections::BTreeMap::new();
    replacement.insert("CATEGORY".to_string(), "New".to_string());
    replacement.insert("URL".to_string(), "https://example.com".to_string());
    let updated = handle
        .update_task(TaskUpdate::new(task.id).extra_properties_value(replacement.clone()))
        .await
        .unwrap();
    assert_eq!(updated.extra_properties, replacement);
    assert!(
        !updated.extra_properties.contains_key("CLIENT"),
        "whole-map replace drops keys not in the replacement"
    );
}

#[tokio::test]
async fn update_task_clears_extras_with_empty_map() {
    // Calling extra_properties_value(BTreeMap::new()) clears
    // the column back to NULL (read boundary normalises to
    // empty map).
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let mut initial = std::collections::BTreeMap::new();
    initial.insert("CATEGORY".to_string(), "Q3".to_string());
    let task = handle
        .create_task(NewTask {
            title: "to clear".to_string(),
            extra_properties: initial,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(!task.extra_properties.is_empty());

    let updated = handle
        .update_task(
            TaskUpdate::new(task.id).extra_properties_value(std::collections::BTreeMap::new()),
        )
        .await
        .unwrap();
    assert!(updated.extra_properties.is_empty());
}

// ── Task templates (v0.33.0) ────────────────────────────────────

#[tokio::test]
async fn create_and_instantiate_task_template() {
    use crate::domain::{NewTaskTemplate, NewTaskTemplateItem};
    let (handle, mut changes_rx, mut library_rx) = spawn(fresh_conn());

    let tmpl = handle
        .create_task_template(NewTaskTemplate {
            name: "Trip".into(),
            project_title_seed: "Weekend Trip".into(),
            note: "Pre-trip checklist".into(),
            tags: vec!["travel".into()],
            items: vec![
                NewTaskTemplateItem {
                    title: "Pack".into(),
                    parent_index: None,
                    estimated_minutes: Some(30),
                    default_tags: vec![],
                },
                NewTaskTemplateItem {
                    title: "Socks".into(),
                    parent_index: Some(0),
                    estimated_minutes: None,
                    default_tags: vec!["clothing".into()],
                },
            ],
        })
        .await
        .unwrap();
    assert_eq!(tmpl.name, "Trip");
    assert_eq!(tmpl.tags, vec!["travel".to_string()]);

    let project = handle.instantiate_template(tmpl.id).await.unwrap();
    assert_eq!(project.title, "Weekend Trip");

    // Project-created library delta.
    let lib = library_rx.recv().await.unwrap();
    assert_eq!(lib.projects_created.len(), 1);
    assert_eq!(lib.projects_created[0].id, project.id);

    // Two tasks, the second nested under the first.
    let changes = changes_rx.recv().await.unwrap();
    assert_eq!(changes.created.len(), 2);
    let pack = changes.created.iter().find(|t| t.title == "Pack").unwrap();
    let socks = changes.created.iter().find(|t| t.title == "Socks").unwrap();
    assert_eq!(pack.parent_id, None);
    assert_eq!(pack.estimated_minutes, Some(30));
    assert_eq!(socks.parent_id, Some(pack.id));
    assert_eq!(pack.project_id, Some(project.id));
}

#[tokio::test]
async fn delete_task_template_then_instantiate_is_not_found() {
    use crate::domain::{NewTaskTemplate, NewTaskTemplateItem};
    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let tmpl = handle
        .create_task_template(NewTaskTemplate {
            name: "Throwaway".into(),
            items: vec![NewTaskTemplateItem {
                title: "x".into(),
                ..Default::default()
            }],
            ..Default::default()
        })
        .await
        .unwrap();
    handle.delete_task_template(tmpl.id).await.unwrap();
    // The template (and its CASCADE-deleted items) are gone.
    let err = handle.instantiate_template(tmpl.id).await.unwrap_err();
    assert!(matches!(err, DbError::NotFound));
}
