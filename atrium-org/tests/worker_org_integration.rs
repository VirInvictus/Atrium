// SPDX-License-Identifier: MIT
//! End-to-end tests that exercise atrium-org against atrium-core's
//! single-writer worker. Extracted from
//! `atrium-core/src/db/worker_tests.rs` at v0.9.0 alongside the
//! `atrium-org` crate split — these tests need both crates and
//! belong on this side of the boundary.
//!
//! Coverage:
//!
//! - `import_org_file` round-trips a fixture file through the worker.
//! - `import_org_directory` walks an `<area>/<project>.org` vault tree
//!   (and warns about sub-area subdirectories).
//! - `--dry-run` import path doesn't insert.
//! - `spawn_worker_with_vault` + `spawn_org_vault` end-to-end: a
//!   project + task creation auto-flushes the project's `.org` file
//!   within the debounce window.

use atrium_core::db::open;
use atrium_core::db::read_pool::ReadPool;
use atrium_core::{NewProject, NewTask, spawn_worker, spawn_worker_with_vault};

fn fresh_conn(label: &str) -> (rusqlite::Connection, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("atrium-org-int-{}-{}", label, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("atrium.db");
    let conn = open(&db_path).unwrap();
    (conn, dir)
}

#[tokio::test]
async fn import_org_file_round_trips_to_db() {
    use atrium_org::org::import_org_file;

    let (conn, dir) = fresh_conn("import-file");
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

    let (handle, _changes_rx, _library_rx) = spawn_worker(conn);
    let summary = import_org_file(&handle, &path, false).await.unwrap();
    assert_eq!(summary.tasks_created, 3);
    assert_eq!(summary.headings_skipped, 1);
    assert!(summary.project_id.is_some());
    assert_eq!(summary.project_title.as_deref(), Some("Errands"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn import_org_directory_walks_areas_and_files() {
    use atrium_org::org::import_org_directory;

    let (conn, dir) = fresh_conn("import-dir");

    std::fs::write(dir.join("Inbox.org"), "* TODO Triage\n").unwrap();

    std::fs::create_dir_all(dir.join("Personal")).unwrap();
    std::fs::write(
        dir.join("Personal").join("Errands.org"),
        "* TODO Buy milk\n",
    )
    .unwrap();

    std::fs::create_dir_all(dir.join(".atrium")).unwrap();
    std::fs::write(dir.join(".atrium").join("config.toml"), "").unwrap();

    std::fs::create_dir_all(dir.join("Personal").join("subarea")).unwrap();
    std::fs::write(
        dir.join("Personal").join("subarea").join("nested.org"),
        "* TODO ignored\n",
    )
    .unwrap();

    let (handle, _changes_rx, _library_rx) = spawn_worker(conn);
    let summaries = import_org_directory(&handle, &dir, false).await.unwrap();

    let imported_titles: Vec<String> = summaries
        .iter()
        .filter_map(|s| s.project_title.clone())
        .collect();
    assert!(imported_titles.contains(&"Inbox".to_string()));
    assert!(imported_titles.contains(&"Errands".to_string()));
    assert!(!imported_titles.contains(&"nested".to_string()));

    let any_warning = summaries.iter().any(|s| {
        s.lossy
            .iter()
            .any(|note| note.contains("sub-area directory"))
    });
    assert!(any_warning, "expected sub-area warning in summaries");

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn import_org_file_dry_run_creates_nothing() {
    use atrium_org::org::import_org_file;

    let (conn, dir) = fresh_conn("import-dry");
    let path = dir.join("Sample.org");
    std::fs::write(&path, "* TODO One\n* TODO Two\n").unwrap();

    let (handle, _changes_rx, _library_rx) = spawn_worker(conn);
    let summary = import_org_file(&handle, &path, true).await.unwrap();
    assert_eq!(summary.tasks_created, 2);
    assert!(summary.project_id.is_none(), "dry-run must not insert");

    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn spawn_with_vault_writes_org_file_on_task_create() {
    let (conn, scratch) = fresh_conn("vault-spawn");

    // The vault writes land in the same scratch dir as the DB so
    // we can poke the .org file through plain `fs::read_to_string`.
    let pool = ReadPool::new(scratch.join("atrium.db"), 4);
    let vault_config = atrium_org::spawn_org_vault(scratch.clone(), pool);
    let (handle, _changes_rx, _library_rx) = spawn_worker_with_vault(conn, Some(vault_config));

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

    let _ = std::fs::remove_dir_all(&scratch);
}

#[tokio::test]
async fn spawn_with_vault_emits_sidecar_after_tag_change() {
    use atrium_core::NewTag;

    let (conn, scratch) = fresh_conn("sidecar");
    let pool = ReadPool::new(scratch.join("atrium.db"), 4);
    let vault_config = atrium_org::spawn_org_vault(scratch.clone(), pool);
    let (handle, _changes_rx, _library_rx) = spawn_worker_with_vault(conn, Some(vault_config));

    // Seed a project + task so the writer has a flush trigger.
    let project = handle
        .create_project(NewProject {
            title: "Sidecar test".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    let _ = handle
        .create_task(NewTask {
            title: "first".to_string(),
            project_id: Some(project.id),
            ..Default::default()
        })
        .await
        .unwrap();

    // Add a tag with a colour. The tag CRUD doesn't itself trigger
    // a project flush, but a subsequent task touch will, and the
    // writer's flush_due path refreshes the sidecar from DB.
    let tag = handle
        .create_tag(NewTag {
            name: "work".to_string(),
            color: Some("#3584e4".to_string()),
        })
        .await
        .unwrap();
    let _ = handle
        .create_task(NewTask {
            title: "second".to_string(),
            project_id: Some(project.id),
            ..Default::default()
        })
        .await
        .unwrap();
    let _ = handle.set_task_tags(1, vec![tag.id]).await.ok();

    // Wait long enough for debounce + tick + sidecar refresh.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let sidecar_path = scratch.join(".atrium").join("config.toml");
    assert!(
        sidecar_path.exists(),
        "expected sidecar at {}",
        sidecar_path.display()
    );
    let text = std::fs::read_to_string(&sidecar_path).unwrap();
    assert!(text.contains("[tags]"), "got: {text}");
    assert!(
        text.contains("work = \"#3584e4\""),
        "expected tag colour in sidecar; got: {text}"
    );

    let _ = std::fs::remove_dir_all(&scratch);
}
