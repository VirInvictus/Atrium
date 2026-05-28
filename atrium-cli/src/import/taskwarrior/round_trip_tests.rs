// SPDX-License-Identifier: MIT
//! Integration tests: parsed Taskwarrior JSON → DB import →
//! read-back through `db::read::list_all_tasks`. Asserts the
//! modeled subset round-trips losslessly. Lives under
//! `src/import/taskwarrior/` because atrium-cli is a binary
//! crate (no library target).

use std::path::Path;

use crate::args::UdaPolicy;
use crate::import::taskwarrior::mapper::{LossyKind, import_taskwarrior};
use crate::import::taskwarrior::parser::parse_export;

fn fresh_file_db(label: &str) -> (rusqlite::Connection, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "atrium-taskwarrior-{}-{}",
        label,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("atrium.db");
    let conn = atrium_core::db::open(&db_path).unwrap();
    (conn, db_path)
}

fn read_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/taskwarrior")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} unreadable: {e}", path.display()))
}

#[tokio::test]
async fn basic_fixture_round_trips_modeled_subset() {
    let text = read_fixture("basic.json");
    let parsed = parse_export(&text).unwrap();
    assert_eq!(parsed.len(), 1);

    let (writer_conn, db_path) = fresh_file_db("basic");
    let (handle, _changes, _library) = atrium_core::spawn_worker(writer_conn);

    let summary = import_taskwarrior(&handle, &parsed, "Errands", UdaPolicy::Tag, false)
        .await
        .unwrap();
    assert_eq!(summary.tasks_created, 1);
    assert!(summary.project_id.is_some());

    drop(handle);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let read_conn = atrium_core::db::open(&db_path).unwrap();
    let tasks = atrium_core::db::read::list_all_tasks(&read_conn).unwrap();
    assert_eq!(tasks.len(), 1);
    let t = &tasks[0];
    assert_eq!(t.title, "Buy milk");
    assert_eq!(t.uuid, "11111111-2222-3333-4444-555555555555");
    assert!(t.deadline.is_some());
    assert!(t.scheduled_for.is_some());

    let _ = std::fs::remove_dir_all(db_path.parent().unwrap());
}

#[tokio::test]
async fn multi_fixture_lands_status_variants_correctly() {
    let text = read_fixture("multi.json");
    let parsed = parse_export(&text).unwrap();
    // Six rows in the source.
    assert_eq!(parsed.len(), 6);

    let (writer_conn, db_path) = fresh_file_db("multi");
    let (handle, _changes, _library) = atrium_core::spawn_worker(writer_conn);

    let summary = import_taskwarrior(&handle, &parsed, "Mixed", UdaPolicy::Tag, false)
        .await
        .unwrap();
    // pending + waiting + completed + recurring-child land = 4.
    // deleted + recurring-template parent skip.
    assert_eq!(summary.tasks_created, 4);
    let kinds: Vec<LossyKind> = summary.lossy.iter().map(|l| l.kind).collect();
    assert!(kinds.contains(&LossyKind::Deleted));
    assert!(kinds.contains(&LossyKind::DroppedRecurringTemplate));
    assert!(kinds.contains(&LossyKind::DroppedRecurringChild));

    drop(handle);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let read_conn = atrium_core::db::open(&db_path).unwrap();
    let tasks = atrium_core::db::read::list_all_tasks(&read_conn).unwrap();
    assert_eq!(tasks.len(), 4);

    let waiting = tasks
        .iter()
        .find(|t| t.title.starts_with("Wait for invoice"))
        .expect("waiting task should land");
    assert_eq!(waiting.orig_keyword.as_deref(), Some("WAITING"));

    let completed = tasks
        .iter()
        .find(|t| t.title.starts_with("Submit report"))
        .expect("completed task should land");
    assert!(completed.completed_at.is_some());

    let _ = std::fs::remove_dir_all(db_path.parent().unwrap());
}

#[tokio::test]
async fn uda_fixture_tag_policy_creates_uda_tags() {
    let text = read_fixture("uda.json");
    let parsed = parse_export(&text).unwrap();

    let (writer_conn, db_path) = fresh_file_db("uda-tag");
    let (handle, _changes, _library) = atrium_core::spawn_worker(writer_conn);

    let summary = import_taskwarrior(&handle, &parsed, "UDAs", UdaPolicy::Tag, false)
        .await
        .unwrap();
    assert!(summary.tasks_created >= 1);
    // No DroppedUda entries under Tag policy.
    assert!(
        !summary
            .lossy
            .iter()
            .any(|l| l.kind == LossyKind::DroppedUda),
    );

    drop(handle);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let read_conn = atrium_core::db::open(&db_path).unwrap();
    let tags = atrium_core::db::read::list_tags(&read_conn).unwrap();
    let names: Vec<&str> = tags.iter().map(|t| t.name.as_str()).collect();
    // Fixture has "client":"Acme" and "effort":"large" UDAs.
    assert!(
        names.contains(&"client-Acme"),
        "expected client-Acme tag; got {names:?}"
    );
    assert!(
        names.contains(&"effort-large"),
        "expected effort-large tag; got {names:?}"
    );

    let _ = std::fs::remove_dir_all(db_path.parent().unwrap());
}

#[tokio::test]
async fn uda_fixture_note_policy_appends_uda_lines_to_note() {
    let text = read_fixture("uda.json");
    let parsed = parse_export(&text).unwrap();

    let (writer_conn, db_path) = fresh_file_db("uda-note");
    let (handle, _changes, _library) = atrium_core::spawn_worker(writer_conn);

    import_taskwarrior(&handle, &parsed, "UDAs", UdaPolicy::Note, false)
        .await
        .unwrap();

    drop(handle);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let read_conn = atrium_core::db::open(&db_path).unwrap();
    let tasks = atrium_core::db::read::list_all_tasks(&read_conn).unwrap();
    let with_uda = tasks
        .iter()
        .find(|t| t.note.contains("UDA: "))
        .expect("at least one task should carry UDA note lines");
    assert!(with_uda.note.contains("UDA: "));

    let _ = std::fs::remove_dir_all(db_path.parent().unwrap());
}

#[tokio::test]
async fn uda_fixture_drop_policy_emits_dropped_uda_lossy() {
    let text = read_fixture("uda.json");
    let parsed = parse_export(&text).unwrap();

    let (writer_conn, _db_path) = fresh_file_db("uda-drop");
    let (handle, _changes, _library) = atrium_core::spawn_worker(writer_conn);

    let summary = import_taskwarrior(&handle, &parsed, "UDAs", UdaPolicy::Drop, false)
        .await
        .unwrap();
    assert!(
        summary
            .lossy
            .iter()
            .any(|l| l.kind == LossyKind::DroppedUda),
    );
}

#[tokio::test]
async fn recurring_fixture_translates_recur_strings() {
    let text = read_fixture("recurring.json");
    let parsed = parse_export(&text).unwrap();

    let (writer_conn, db_path) = fresh_file_db("recurring");
    let (handle, _changes, _library) = atrium_core::spawn_worker(writer_conn);

    let summary = import_taskwarrior(&handle, &parsed, "Recurring", UdaPolicy::Tag, false)
        .await
        .unwrap();
    // At least one unparseable case in the fixture.
    assert!(
        summary
            .lossy
            .iter()
            .any(|l| l.kind == LossyKind::UnparseableRecurrence),
    );

    drop(handle);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let read_conn = atrium_core::db::open(&db_path).unwrap();
    let tasks = atrium_core::db::read::list_all_tasks(&read_conn).unwrap();
    let with_rrule = tasks.iter().filter(|t| t.repeat_rule.is_some()).count();
    assert!(
        with_rrule >= 1,
        "at least one task should land with an RRULE"
    );
}
