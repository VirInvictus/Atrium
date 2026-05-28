// SPDX-License-Identifier: MIT
//! Integration tests for the todo.txt importer: file → parse →
//! import_todotxt → DB read-back via list_all_tasks.

use std::path::Path;

use crate::import::todotxt::mapper::{LossyKind, import_todotxt};
use crate::import::todotxt::parser::parse_document;

fn fresh_file_db(label: &str) -> (rusqlite::Connection, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("atrium-todotxt-{}-{}", label, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("atrium.db");
    let conn = atrium_core::db::open(&db_path).unwrap();
    (conn, db_path)
}

fn read_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/todotxt")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("fixture {} unreadable: {e}", path.display()))
}

#[tokio::test]
async fn basic_fixture_round_trips_modeled_subset() {
    let text = read_fixture("basic.txt");
    let parsed = parse_document(&text);
    assert_eq!(parsed.len(), 1);

    let (writer_conn, db_path) = fresh_file_db("basic");
    let (handle, _changes, _library) = atrium_core::spawn_worker(writer_conn);

    let summary = import_todotxt(&handle, &parsed, "Errands", false)
        .await
        .unwrap();
    assert_eq!(summary.tasks_created, 1);
    assert!(summary.project_id.is_some());
    // home + errands + priority-1 = 3 tags.
    assert_eq!(summary.tags_created, 3);
    // +groceries should surface as a single DroppedInlineProject entry.
    assert!(
        summary
            .lossy
            .iter()
            .any(|l| l.kind == LossyKind::DroppedInlineProject),
    );

    drop(handle);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let read_conn = atrium_core::db::open(&db_path).unwrap();
    let tasks = atrium_core::db::read::list_all_tasks(&read_conn).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].title, "Buy milk");
    assert!(tasks[0].deadline.is_some());

    let _ = std::fs::remove_dir_all(db_path.parent().unwrap());
}

#[tokio::test]
async fn mixed_fixture_handles_completed_and_threshold_paths() {
    let text = read_fixture("mixed.txt");
    let parsed = parse_document(&text);
    // Six non-comment lines in the fixture.
    assert_eq!(parsed.len(), 6);

    let (writer_conn, db_path) = fresh_file_db("mixed");
    let (handle, _changes, _library) = atrium_core::spawn_worker(writer_conn);

    let summary = import_todotxt(&handle, &parsed, "Mixed", false)
        .await
        .unwrap();
    assert_eq!(summary.tasks_created, 6);

    // (D) Low-priority cleanup → PriorityBelowC lossy entry.
    assert!(
        summary
            .lossy
            .iter()
            .any(|l| l.kind == LossyKind::PriorityBelowC),
    );
    // `http://example.com` → DroppedKeyValue (the `http://...` token
    // parses as a key:value extension; the mapper drops it).
    assert!(
        summary
            .lossy
            .iter()
            .any(|l| l.kind == LossyKind::DroppedKeyValue),
    );

    drop(handle);
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    let read_conn = atrium_core::db::open(&db_path).unwrap();
    let tasks = atrium_core::db::read::list_all_tasks(&read_conn).unwrap();
    assert_eq!(tasks.len(), 6);

    let completed = tasks
        .iter()
        .find(|t| t.title == "File taxes")
        .expect("completed task should land");
    assert!(completed.completed_at.is_some());

    let deferred = tasks
        .iter()
        .find(|t| t.title == "Plan vacation")
        .expect("threshold-deferred task should land");
    assert!(deferred.defer_until.is_some());

    let _ = std::fs::remove_dir_all(db_path.parent().unwrap());
}

#[tokio::test]
async fn parse_document_skips_comments_and_blanks() {
    let text = "# header\n\n(A) one\n\n(B) two\n";
    let parsed = parse_document(text);
    assert_eq!(parsed.len(), 2);
}
