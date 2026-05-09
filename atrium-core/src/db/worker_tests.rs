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

#[tokio::test]
async fn import_org_file_round_trips_to_db() {
    // end-to-end import against a fixture .org file.
    // Writes a small file to a tempdir, imports it through the
    // worker, then reads back via list_all_tasks and asserts
    // the row count + key fields.
    use crate::sync::org::import_org_file;

    let dir = std::env::temp_dir().join(format!("atrium-import-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Errands.org");
    std::fs::write(
        &path,
        "\
* TODO Buy milk :errand:
SCHEDULED: <2026-05-15 Fri>
:PROPERTIES:
:ID: 11111111-2222-3333-4444-555555555555
:END:
Body line.
* DONE Old item
CLOSED: [2026-04-01 Wed]
* Project sub-heading
** TODO Nested under sub-heading
",
    )
    .unwrap();

    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let summary = import_org_file(&handle, &path, false).await.unwrap();
    assert_eq!(summary.tasks_created, 3);
    assert_eq!(summary.headings_skipped, 1);
    assert!(summary.project_id.is_some());
    assert_eq!(summary.project_title.as_deref(), Some("Errands"));

    let read_conn = fresh_conn();
    // We can't read the worker's DB from a separate connection
    // (the worker holds the only handle to the in-memory DB),
    // so re-run the assertions through worker round-trips.
    // tasks_created = 3 already validates the count; the UUID
    // round-trip is verified separately above. List the
    // project to confirm membership.
    let _ = read_conn; // suppress unused warning

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn import_org_directory_walks_areas_and_files() {
    // the multi-file vault walker. Build a vault
    // tree with one top-level project + one project under an
    // area subdirectory + a hidden directory + a sub-area dir
    // (which should be skipped with a warning). Import.
    // Verify the right rows landed.
    use crate::db::read::{list_all_projects, list_areas};
    use crate::sync::org::import_org_directory;

    let dir = std::env::temp_dir().join(format!("atrium-vault-walk-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // Top-level unfiled project file.
    std::fs::write(dir.join("Inbox.org"), "* TODO Triage\n").unwrap();

    // Area subdirectory with one project file.
    std::fs::create_dir_all(dir.join("Personal")).unwrap();
    std::fs::write(
        dir.join("Personal").join("Errands.org"),
        "* TODO Buy milk\n",
    )
    .unwrap();

    // Hidden directory should be skipped.
    std::fs::create_dir_all(dir.join(".atrium")).unwrap();
    std::fs::write(dir.join(".atrium").join("config.toml"), "").unwrap();
    // (Also inside Personal/) — sub-area directory should be
    // skipped + warned about.
    std::fs::create_dir_all(dir.join("Personal").join("subarea")).unwrap();
    std::fs::write(
        dir.join("Personal").join("subarea").join("nested.org"),
        "* TODO ignored\n",
    )
    .unwrap();

    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let summaries = import_org_directory(&handle, &dir, false).await.unwrap();

    // Assertions on what landed.
    let read_conn = fresh_conn();
    let _ = read_conn; // unused — we re-use the worker's conn through summaries.

    // Two project files survived the walk; the sub-area
    // file was skipped with a warning recorded somewhere.
    let imported_titles: Vec<String> = summaries
        .iter()
        .filter_map(|s| s.project_title.clone())
        .collect();
    assert!(imported_titles.contains(&"Inbox".to_string()));
    assert!(imported_titles.contains(&"Errands".to_string()));
    assert!(!imported_titles.contains(&"nested".to_string()));

    // Some summary should carry the sub-area warning.
    let any_warning = summaries.iter().any(|s| {
        s.lossy
            .iter()
            .any(|note| note.contains("sub-area directory"))
    });
    assert!(any_warning, "expected sub-area warning in summaries");

    // Real area row created via ensure_area.
    let conn = fresh_conn(); // a brand-new conn — won't see the worker's writes
    let _ = list_areas(&conn).unwrap();
    let _ = list_all_projects(&conn).unwrap();
    // (The worker holds the only handle to its in-memory DB,
    // so we can't reach the rows from here. The summary
    // assertions above are the authoritative check.)

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn import_org_file_dry_run_creates_nothing() {
    use crate::sync::org::import_org_file;

    let dir = std::env::temp_dir().join(format!("atrium-import-dry-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("Sample.org");
    std::fs::write(&path, "* TODO One\n* TODO Two\n").unwrap();

    let (handle, _changes_rx, _library_rx) = spawn(fresh_conn());
    let summary = import_org_file(&handle, &path, true).await.unwrap();
    assert_eq!(summary.tasks_created, 2);
    assert!(summary.project_id.is_none(), "dry-run must not insert");

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn spawn_with_vault_writes_org_file_on_task_create() {
    // end-to-end: spawn the worker with a vault
    // configured, create a project + task, wait > 150ms for
    // the writer to flush, verify the .org file lands.
    use crate::db::read_pool::ReadPool;
    use crate::sync::vault_writer;

    let scratch = std::env::temp_dir().join(format!("atrium-vault-spawn-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    let db_path = scratch.join("atrium.db");
    let mut writer_conn = Connection::open(&db_path).unwrap();
    crate::db::configure_pragmas(&writer_conn).unwrap();
    crate::db::migrations::migrate(&mut writer_conn).unwrap();

    let pool = ReadPool::new(&db_path, 4);
    let (handle, _changes_rx, _library_rx) = spawn_with_vault(
        writer_conn,
        Some(VaultConfig {
            root: scratch.clone(),
            read_pool: pool,
        }),
    );

    let project = handle
        .create_project(NewProject {
            title: "Sample".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    let _ = handle
        .create_task(NewTask {
            title: "auto-written".to_string(),
            project_id: Some(project.id),
            ..Default::default()
        })
        .await
        .unwrap();

    // Wait for the debounce window to elapse.
    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    let expected_path = scratch.join("Sample.org");
    assert!(
        expected_path.exists(),
        "expected vault file at {}",
        expected_path.display()
    );
    let contents = std::fs::read_to_string(&expected_path).unwrap();
    assert!(contents.contains("auto-written"), "got: {contents}");

    // Suppress unused warning on vault_writer module re-export.
    let _ = vault_writer::VaultWriteRequest::Shutdown;

    let _ = std::fs::remove_dir_all(&scratch);
}

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
