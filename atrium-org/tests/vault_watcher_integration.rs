// SPDX-License-Identifier: MIT
//! End-to-end tests for the Phase 17 vault → DB sync watcher
//! (v0.10.0).
//!
//! Three integration scenarios:
//!
//! 1. **External add.** A user appends a new TODO headline to
//!    a vault file. The watcher imports the new task and the
//!    writer rewrites the file with a `:ID:` property.
//! 2. **External edit.** A user changes a headline's keyword
//!    (TODO → DONE). The watcher updates `task.completed_at`.
//! 3. **External delete.** A user removes a headline from a
//!    file. The watcher deletes the matching DB row.
//!
//! Each test seeds a fresh DB + vault, spawns the writer +
//! watcher, mutates the vault file from "outside" (a plain
//! `fs::write` from the test thread; not routed through the
//! writer), and asserts the DB lands in the expected state
//! within the debounce window.

use std::path::{Path, PathBuf};
use std::time::Duration;

use atrium_core::db::open;
use atrium_core::db::read_pool::ReadPool;
use atrium_core::{NewProject, NewTask, spawn_worker_with_vault};

fn fresh_setup(label: &str) -> (rusqlite::Connection, PathBuf, ReadPool) {
    let dir = std::env::temp_dir().join(format!(
        "atrium-watcher-int-{}-{}",
        label,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("atrium.db");
    let conn = open(&db_path).unwrap();
    let pool = ReadPool::new(&db_path, 4);
    (conn, dir, pool)
}

/// Seed a project + one task, then return everything the test
/// needs to drive the watcher end-to-end. Project file lives at
/// `<vault>/<title>.org`. The initial vault state has the seeded
/// task already emitted by the writer.
async fn seed_with_initial_write(
    conn: rusqlite::Connection,
    pool: ReadPool,
    vault: &Path,
) -> (atrium_core::WorkerHandle, tokio::task::JoinHandle<()>, i64) {
    // Spawn writer + watcher sharing one RecentWrites set.
    let recent = std::sync::Arc::new(std::sync::RwLock::new(atrium_org::RecentWrites::new()));
    let notifier = atrium_org::spawn_vault_writer_with_recent(
        vault.to_path_buf(),
        pool.clone(),
        recent.clone(),
    );
    let vault_config = atrium_core::VaultConfig {
        notifier: std::sync::Arc::new(notifier),
    };
    let (handle, _changes_rx, _library_rx) = spawn_worker_with_vault(conn, Some(vault_config));

    // Seed the project + one task BEFORE the watcher spawns so
    // the initial vault file lands cleanly.
    let project = handle
        .create_project(NewProject {
            title: "Errands".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();
    handle
        .create_task(NewTask {
            title: "Buy milk".to_string(),
            project_id: Some(project.id),
            ..Default::default()
        })
        .await
        .unwrap();

    // Wait for the writer's initial flush.
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(
        vault.join("Errands.org").exists(),
        "expected initial vault file at Errands.org"
    );

    // Now spawn the watcher so it picks up subsequent changes.
    let watcher_handle =
        atrium_org::spawn_vault_watcher(vault.to_path_buf(), handle.clone(), pool, recent).unwrap();

    (handle, watcher_handle, project.id)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn external_add_creates_db_task() {
    let (conn, vault, pool) = fresh_setup("ext-add");
    let (handle, _watcher, project_id) = seed_with_initial_write(conn, pool.clone(), &vault).await;

    // Read the current file contents, append a new TODO headline,
    // and write it back. Simulates an Emacs save.
    let project_path = vault.join("Errands.org");
    let existing = std::fs::read_to_string(&project_path).unwrap();
    let appended = format!("{existing}\n* TODO Buy bread\n");
    std::fs::write(&project_path, appended).unwrap();

    // Wait for: inotify event → 200 ms watcher debounce → diff →
    // create_task → 100 ms writer debounce → file rewrite → :ID:
    // appears on the new headline.
    tokio::time::sleep(Duration::from_millis(700)).await;

    // Assert: DB now has two tasks in the project.
    let tasks = pool
        .with(|conn| atrium_core::db::read::list_all_in_project(conn, project_id))
        .unwrap();
    let titles: Vec<&str> = tasks.iter().map(|t| t.title.as_str()).collect();
    assert!(
        titles.contains(&"Buy milk") && titles.contains(&"Buy bread"),
        "expected both tasks; got: {titles:?}"
    );

    // Assert: the rewritten file contains the new task's :ID:
    // (allocated by the watcher, then flushed by the writer).
    let final_text = std::fs::read_to_string(&project_path).unwrap();
    assert!(
        final_text.contains("* TODO Buy bread"),
        "rewritten file lost the new headline:\n{final_text}"
    );
    assert_eq!(
        final_text.matches(":ID:").count(),
        // 1 for the project file-level :ID:, 2 for the two tasks.
        3,
        "expected 3 :ID: properties (project + 2 tasks):\n{final_text}"
    );

    drop(handle);
    let _ = std::fs::remove_dir_all(&vault);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn external_edit_completes_db_task() {
    let (conn, vault, pool) = fresh_setup("ext-edit");
    let (handle, _watcher, project_id) = seed_with_initial_write(conn, pool.clone(), &vault).await;

    // Flip TODO → DONE in the file.
    let project_path = vault.join("Errands.org");
    let text = std::fs::read_to_string(&project_path).unwrap();
    let edited = text.replace(
        "* TODO Buy milk",
        "* DONE Buy milk\nCLOSED: [2026-04-01 Wed 09:00]",
    );
    std::fs::write(&project_path, edited).unwrap();

    tokio::time::sleep(Duration::from_millis(700)).await;

    // Assert: the DB task is now completed (completed_at set).
    let tasks = pool
        .with(|conn| atrium_core::db::read::list_all_in_project(conn, project_id))
        .unwrap();
    let milk = tasks
        .iter()
        .find(|t| t.title == "Buy milk")
        .expect("milk task should still exist");
    assert!(
        milk.completed_at.is_some(),
        "expected completed_at to be set after external DONE flip"
    );

    drop(handle);
    let _ = std::fs::remove_dir_all(&vault);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn external_delete_removes_db_task() {
    let (conn, vault, pool) = fresh_setup("ext-delete");
    let (handle, _watcher, project_id) = seed_with_initial_write(conn, pool.clone(), &vault).await;

    // Add a second task so we have something distinct to remove.
    handle
        .create_task(NewTask {
            title: "Buy bread".to_string(),
            project_id: Some(project_id),
            ..Default::default()
        })
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(250)).await;

    // Now remove the "Buy bread" headline (and its :PROPERTIES:
    // drawer) from the file by hand. This is the trickier case
    // because we have to splice the file, not just append.
    let project_path = vault.join("Errands.org");
    let text = std::fs::read_to_string(&project_path).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    let mut keep: Vec<&str> = Vec::with_capacity(lines.len());
    let mut skipping = false;
    for line in lines {
        if line.starts_with("* TODO Buy bread") || line.starts_with("* DONE Buy bread") {
            skipping = true;
            continue;
        }
        if skipping {
            // Skip the next-headline-or-blank-after-properties.
            if line.starts_with("* ") {
                skipping = false;
                keep.push(line);
            }
            // else swallow this line (still inside the removed
            // task's body / properties drawer).
            continue;
        }
        keep.push(line);
    }
    let edited = format!("{}\n", keep.join("\n"));
    std::fs::write(&project_path, edited).unwrap();

    tokio::time::sleep(Duration::from_millis(700)).await;

    // Assert: the DB only has "Buy milk" left.
    let tasks = pool
        .with(|conn| atrium_core::db::read::list_all_in_project(conn, project_id))
        .unwrap();
    let titles: Vec<&str> = tasks.iter().map(|t| t.title.as_str()).collect();
    assert!(
        titles.contains(&"Buy milk"),
        "milk should survive: {titles:?}"
    );
    assert!(
        !titles.contains(&"Buy bread"),
        "bread should have been deleted: {titles:?}"
    );

    drop(handle);
    let _ = std::fs::remove_dir_all(&vault);
}
