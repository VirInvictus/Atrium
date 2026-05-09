// SPDX-License-Identifier: MIT
//! Atrium native JSON export — lossless DB snapshot (Phase 16,
//! v0.7.11).
//!
//! Per the roadmap: "Atrium native JSON export ships in this phase
//! too — universal lossless backup format." The JSON snapshot is
//! the format-agnostic complement to the Org vault writer:
//!
//! - **Org vault** is interoperable with Emacs / vim-orgmode /
//!   any Org tool, but lossy on constructs Atrium doesn't model
//!   (custom keywords fold to TODO; project sub-headings are
//!   currently dropped through the writer; etc.).
//! - **JSON snapshot** is Atrium-only but lossless. Every row of
//!   every relevant table lands in the file, keyed by the UUIDs
//!   that travel with the data forever. Useful as a true backup,
//!   for diffing two DB states, and for any future cross-version
//!   migration.
//!
//! The schema is a single top-level [`Snapshot`] struct holding a
//! `Vec<T>` per domain type plus the `task_tag` relation. Versioning
//! is forward-compatible: the exporter writes a `version` string
//! that future tooling can read. New fields appended to domain
//! structs flow through automatically (serde's default-on-missing
//! semantics).
//!
//! v0.7.11 implements **export only**. Re-importing a JSON snapshot
//! into a DB lands as a separate concern (Phase 17 sync work or
//! later — the use case is restore-from-backup, not a hot path).

use std::io;
use std::path::Path;

use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::domain::{Area, Heading, Perspective, Project, Tag, Task};
use crate::error::DbError;
use crate::sync::atomic::write_atomic;

/// Version string for the snapshot schema. Bumped whenever a
/// breaking change to the layout lands so future readers can
/// fail loudly on unsupported formats. v0.7.11 establishes
/// `"1"` as the initial format.
pub const SNAPSHOT_VERSION: &str = "1";

/// Top-level JSON snapshot of an Atrium DB. Every relevant row
/// in every relevant table appears here exactly once, keyed by
/// the database id but UUID-anchored for cross-DB stability.
///
/// `task_tags` carries the join-table contents as
/// `(task_id, tag_id)` pairs — the same shape as the schema's
/// `task_tag` table. A future restore tool would resolve these
/// against the new task / tag rows by UUID since IDs aren't
/// stable across snapshots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: String,
    pub exported_at: DateTime<Utc>,
    pub atrium_version: String,
    pub areas: Vec<Area>,
    pub projects: Vec<Project>,
    pub headings: Vec<Heading>,
    pub tasks: Vec<Task>,
    pub tags: Vec<Tag>,
    pub task_tags: Vec<TaskTagPair>,
    pub perspectives: Vec<Perspective>,
}

/// Single (task, tag) membership row from the `task_tag` table.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct TaskTagPair {
    pub task_id: i64,
    pub tag_id: i64,
}

/// Build a [`Snapshot`] by reading every relevant table from
/// `conn`. Read-only operation; safe to call from the GUI's
/// shared read pool or atrium-cli's read path.
pub fn build_snapshot(conn: &Connection) -> Result<Snapshot, DbError> {
    use crate::db::read;
    let areas = read::list_areas(conn)?;
    // Use list_all_projects so archived projects are included in
    // the backup. The active-projects list_projects filters them.
    let projects = read::list_all_projects(conn)?;
    let headings = read::list_headings(conn)?;
    let tasks = read::list_all_tasks(conn)?;
    let tags = read::list_tags(conn)?;
    let task_tags: Vec<TaskTagPair> = read::list_task_tags(conn)?
        .into_iter()
        .map(|(task_id, tag_id)| TaskTagPair { task_id, tag_id })
        .collect();
    let perspectives = read::list_perspectives(conn)?;

    Ok(Snapshot {
        version: SNAPSHOT_VERSION.to_string(),
        exported_at: Utc::now(),
        atrium_version: env!("CARGO_PKG_VERSION").to_string(),
        areas,
        projects,
        headings,
        tasks,
        tags,
        task_tags,
        perspectives,
    })
}

/// Build a snapshot and serialize it to pretty-printed JSON.
/// Useful for in-memory inspection, tests, and the atrium-cli
/// dry-run path.
pub fn export_db_to_json_text(conn: &Connection) -> Result<String, DbError> {
    let snapshot = build_snapshot(conn)?;
    serde_json::to_string_pretty(&snapshot).map_err(|e| DbError::Sync(e.to_string()))
}

/// Build a snapshot and write it atomically to `path`. Goes
/// through [`crate::sync::atomic::write_atomic`] so a crash mid-
/// write leaves the previous file (if any) intact.
pub fn export_db_to_json_file(conn: &Connection, path: &Path) -> Result<(), JsonExportError> {
    let text = export_db_to_json_text(conn)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| JsonExportError::Io {
            path: parent.display().to_string(),
            source: e,
        })?;
    }
    write_atomic(path, text.as_bytes()).map_err(|e| JsonExportError::Io {
        path: path.display().to_string(),
        source: e,
    })
}

/// Errors specific to the JSON-export flow.
#[derive(Debug, thiserror::Error)]
pub enum JsonExportError {
    #[error("io error writing {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("DB error: {0}")]
    Db(#[from] DbError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_conn() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::configure_pragmas(&conn).unwrap();
        crate::db::migrations::migrate(&mut conn).unwrap();
        conn
    }

    #[test]
    fn export_empty_db_produces_well_formed_snapshot() {
        let conn = fresh_conn();
        let text = export_db_to_json_text(&conn).unwrap();
        // Serde round-trip: parse the text back into a Snapshot
        // and assert the obvious fields.
        let parsed: Snapshot = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed.version, SNAPSHOT_VERSION);
        assert_eq!(parsed.atrium_version, env!("CARGO_PKG_VERSION"));
        assert!(parsed.areas.is_empty());
        assert!(parsed.projects.is_empty());
        assert!(parsed.headings.is_empty());
        assert!(parsed.tasks.is_empty());
        assert!(parsed.tags.is_empty());
        assert!(parsed.task_tags.is_empty());
        assert!(parsed.perspectives.is_empty());
    }

    #[tokio::test]
    async fn snapshot_includes_seeded_rows() {
        use crate::db::worker::spawn;
        use crate::domain::{NewProject, NewTask};

        let dir = std::env::temp_dir().join(format!("atrium-snapshot-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("snap.db");

        // Seed the file-backed DB through the worker, then snapshot
        // a separate read-conn against the same file. We run inside
        // the #[tokio::test] runtime — no nested runtime here.
        let mut writer_conn = Connection::open(&db_path).unwrap();
        crate::db::configure_pragmas(&writer_conn).unwrap();
        crate::db::migrations::migrate(&mut writer_conn).unwrap();

        let (handle, _changes_rx, _library_rx) = spawn(writer_conn);

        let project = handle
            .create_project(NewProject {
                title: "Errands".to_string(),
                ..Default::default()
            })
            .await
            .unwrap();
        let task = handle
            .create_task(NewTask {
                title: "Buy milk".to_string(),
                project_id: Some(project.id),
                ..Default::default()
            })
            .await
            .unwrap();
        let tag = handle.ensure_tag("errand".to_string()).await.unwrap();
        handle.set_task_tags(task.id, vec![tag.id]).await.unwrap();

        let read_conn = Connection::open(&db_path).unwrap();
        crate::db::configure_pragmas(&read_conn).unwrap();
        let snapshot = build_snapshot(&read_conn).unwrap();
        assert_eq!(snapshot.projects.len(), 1);
        assert_eq!(snapshot.projects[0].title, "Errands");
        assert_eq!(snapshot.tasks.len(), 1);
        assert_eq!(snapshot.tasks[0].title, "Buy milk");
        assert_eq!(snapshot.tags.len(), 1);
        assert_eq!(snapshot.tags[0].name, "errand");
        assert_eq!(snapshot.task_tags.len(), 1);
        assert_eq!(snapshot.task_tags[0].task_id, task.id);
        assert_eq!(snapshot.task_tags[0].tag_id, tag.id);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn export_file_writes_atomically_to_disk() {
        let dir = std::env::temp_dir().join(format!("atrium-jsonout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("backup.json");
        let conn = fresh_conn();
        export_db_to_json_file(&conn, &path).unwrap();
        let read = std::fs::read_to_string(&path).unwrap();
        let parsed: Snapshot = serde_json::from_str(&read).unwrap();
        assert_eq!(parsed.version, SNAPSHOT_VERSION);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
