// SPDX-License-Identifier: MIT
//! SQLite storage layer.
//!
//! - [`open`] opens (or creates) the database, applies pragmas, runs
//!   pending migrations.
//! - [`worker::spawn`] starts the single-writer task (spec §3.2);
//!   [`worker::WorkerHandle`] is the UI-side façade.
//! - [`read_pool::ReadPool`] is the read-only connection pool the UI
//!   uses for list refreshes; [`read`] holds free read functions
//!   composable with both the writer's connection and pool ones.
//! - [`changes::TaskChanges`] is the delta type the worker emits.

pub mod changes;
pub mod command;
pub mod fixtures;
// `pub` (not `pub(crate)`) since v0.9.0 — atrium-org's tests
// reach in for fresh-DB setup. Production code never calls this
// directly; `db::open` is the public entry point.
pub mod migrations;
pub mod read;
pub mod read_pool;
pub mod vault_hook;
pub mod worker;

use std::path::Path;

use rusqlite::Connection;
use tracing::{debug, info};

use crate::error::DbError;

/// v0.6.0 → v0.7.2 — filter expression for the canonical Review
/// page's "This week" weekly walk section. Matches anything the
/// user should look at this week per spec §4.2 + §4.3: overdue
/// items, anything scheduled this week, deadlines reaching next
/// week (heads-up window), and tasks just freed from a defer.
///
/// Originally seeded as a saved Perspective named "Weekly Review";
/// v0.7.2 retired the seed (Brandon's "Review and Weekly Review
/// showed two different things — confusing" feedback). The
/// canonical Review page now renders this same content as its
/// secondary section, alongside the project-review queue. The
/// constant survives so the GUI window-side fetch can use the
/// same expression atrium_search would parse.
pub const REVIEW_WEEKLY_WALK_FILTER: &str = "is:overdue OR scheduled:thisweek OR (is:deadline AND due:nextweek) OR (is:deferred AND defer:<=today)";

/// Open the Atrium database at `path`, applying pragmas and migrations.
///
/// Creates the parent directory if absent. Configures WAL mode,
/// foreign keys, and the perf-budget pragmas per spec §3.2 and
/// roadmap.md Phase 1.
///
/// `:memory:` is accepted (handy for tests) — no parent dir is created
/// in that case.
pub fn open(path: &Path) -> Result<Connection, DbError> {
    if path != Path::new(":memory:")
        && let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }

    info!(path = %path.display(), "opening atrium database");
    let mut conn = Connection::open(path)?;
    configure_pragmas(&conn)?;
    migrations::migrate(&mut conn)?;
    // v0.6.0 → v0.7.2 — the Weekly Review perspective is no
    // longer auto-seeded. The canonical Review page now renders
    // the same content as its weekly-walk section; seeding a
    // duplicate as a saved Perspective just created the
    // confusion Brandon flagged. Existing user DBs keep their
    // row (we don't delete data); fresh DBs land clean.
    Ok(conn)
}

/// Apply pragmas to a connection (writable or read-only). Per spec §3.2
/// and roadmap.md Phase 1.
pub fn configure_pragmas(conn: &Connection) -> Result<(), DbError> {
    // WAL: many readers + one writer; the discipline the worker pattern depends on.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    // NORMAL: durable across power loss when paired with WAL; faster than FULL.
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    // Memory-backed temp tables for FTS5 sort scratch.
    conn.pragma_update(None, "temp_store", "MEMORY")?;
    // 256 MB mmap window for read paths.
    conn.pragma_update(None, "mmap_size", 268_435_456_i64)?;
    // SQLite ships foreign-key checks off by default; we always want them on.
    conn.pragma_update(None, "foreign_keys", "ON")?;
    debug!(
        "pragmas configured: WAL, synchronous=NORMAL, temp_store=MEMORY, mmap_size=256MB, foreign_keys=ON"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn fresh_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrations::migrate(&mut conn).unwrap();
        conn
    }

    #[test]
    fn migration_applies_cleanly() {
        let conn = fresh_db();
        let v: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 14);
    }

    #[test]
    fn migration_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        migrations::migrate(&mut conn).unwrap();
        migrations::migrate(&mut conn).unwrap();
        let v: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 14);
    }

    #[test]
    fn all_user_tables_exist() {
        let conn = fresh_db();
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master \
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' \
                   AND name NOT LIKE 'task_fts%' \
                 ORDER BY name",
            )
            .unwrap();
        let tables: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(
            tables,
            vec![
                "area",
                "heading",
                "perspective",
                "project",
                // v0.18.0 — Phase 18.5 Tier-1 Quick Entry templates.
                "quick_entry_template",
                "tag",
                "task",
                // v0.17.0 — Phase 18.5 Tier-1 CLOCK time tracking.
                "task_clock_entry",
                "task_tag"
            ]
        );
    }

    #[test]
    fn fts_sync_on_insert() {
        let conn = fresh_db();
        conn.execute(
            "INSERT INTO task (uuid, title, note, position) VALUES (?, ?, ?, ?)",
            params!["abc", "Buy milk", "from the store", 1.0],
        )
        .unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM task_fts WHERE task_fts MATCH 'milk'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn fts_sync_on_delete() {
        let conn = fresh_db();
        conn.execute(
            "INSERT INTO task (uuid, title, position) VALUES (?, ?, ?)",
            params!["abc", "Buy milk", 1.0],
        )
        .unwrap();
        conn.execute("DELETE FROM task WHERE uuid = ?", params!["abc"])
            .unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM task_fts WHERE task_fts MATCH 'milk'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn fts_sync_on_update() {
        let conn = fresh_db();
        conn.execute(
            "INSERT INTO task (uuid, title, position) VALUES (?, ?, ?)",
            params!["abc", "Buy milk", 1.0],
        )
        .unwrap();
        conn.execute(
            "UPDATE task SET title = ? WHERE uuid = ?",
            params!["Buy bread", "abc"],
        )
        .unwrap();
        let milk: i64 = conn
            .query_row(
                "SELECT count(*) FROM task_fts WHERE task_fts MATCH 'milk'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let bread: i64 = conn
            .query_row(
                "SELECT count(*) FROM task_fts WHERE task_fts MATCH 'bread'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(milk, 0);
        assert_eq!(bread, 1);
    }

    #[test]
    fn modified_at_trigger_fires() {
        let conn = fresh_db();
        conn.execute(
            "INSERT INTO task (uuid, title, position) VALUES (?, ?, ?)",
            params!["abc", "first", 1.0],
        )
        .unwrap();
        let mod1: String = conn
            .query_row(
                "SELECT modified_at FROM task WHERE uuid = ?",
                params!["abc"],
                |r| r.get(0),
            )
            .unwrap();
        // strftime('%f') has millisecond resolution — sleep past one tick.
        std::thread::sleep(std::time::Duration::from_millis(5));
        conn.execute(
            "UPDATE task SET title = ? WHERE uuid = ?",
            params!["second", "abc"],
        )
        .unwrap();
        let mod2: String = conn
            .query_row(
                "SELECT modified_at FROM task WHERE uuid = ?",
                params!["abc"],
                |r| r.get(0),
            )
            .unwrap();
        assert_ne!(mod1, mod2, "modified_at trigger did not fire");
    }

    #[test]
    fn explicit_modified_at_survives_trigger() {
        // Setting modified_at explicitly (e.g., during import preserving
        // original timestamps) must not be clobbered by the trigger.
        let conn = fresh_db();
        conn.execute(
            "INSERT INTO task (uuid, title, position) VALUES (?, ?, ?)",
            params!["abc", "first", 1.0],
        )
        .unwrap();
        conn.execute(
            "UPDATE task SET title = ?, modified_at = ? WHERE uuid = ?",
            params!["second", "2020-01-01T00:00:00.000Z", "abc"],
        )
        .unwrap();
        let m: String = conn
            .query_row(
                "SELECT modified_at FROM task WHERE uuid = ?",
                params!["abc"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(m, "2020-01-01T00:00:00.000Z");
    }

    #[test]
    fn foreign_keys_enforced() {
        let conn = fresh_db();
        let result = conn.execute(
            "INSERT INTO task (uuid, title, position, project_id) VALUES (?, ?, ?, ?)",
            params!["abc", "test", 1.0, 999],
        );
        assert!(result.is_err());
    }

    #[test]
    fn tag_name_is_case_insensitive_unique() {
        let conn = fresh_db();
        conn.execute(
            "INSERT INTO tag (uuid, name) VALUES (?, ?)",
            params!["t1", "Errand"],
        )
        .unwrap();
        let result = conn.execute(
            "INSERT INTO tag (uuid, name) VALUES (?, ?)",
            params!["t2", "errand"],
        );
        assert!(result.is_err(), "NOCASE uniqueness not enforced");
    }

    #[test]
    fn project_cascade_deletes_tasks() {
        let conn = fresh_db();
        conn.execute(
            "INSERT INTO project (uuid, title, position) VALUES (?, ?, ?)",
            params!["p1", "Project 1", 1.0],
        )
        .unwrap();
        let pid: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO task (uuid, title, position, project_id) VALUES (?, ?, ?, ?)",
            params!["t1", "Task 1", 1.0, pid],
        )
        .unwrap();
        conn.execute("DELETE FROM project WHERE id = ?", params![pid])
            .unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM task", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn area_set_null_on_delete() {
        let conn = fresh_db();
        conn.execute(
            "INSERT INTO area (uuid, title, position) VALUES (?, ?, ?)",
            params!["a1", "Area 1", 1.0],
        )
        .unwrap();
        let aid: i64 = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO project (uuid, title, position, area_id) VALUES (?, ?, ?, ?)",
            params!["p1", "P1", 1.0, aid],
        )
        .unwrap();
        conn.execute("DELETE FROM area WHERE id = ?", params![aid])
            .unwrap();
        let area_id: Option<i64> = conn
            .query_row(
                "SELECT area_id FROM project WHERE uuid = ?",
                params!["p1"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(area_id, None);
    }

    #[test]
    fn open_creates_parent_dir_and_migrates() {
        // Use a tmpdir-style path. cargo test runs each test in CARGO_TARGET_DIR;
        // we tuck the test DB into target/tmp/ so it's gitignored.
        let tmp = std::env::temp_dir().join(format!("atrium-test-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&tmp);
        let conn = open(&tmp).unwrap();
        let v: i64 = conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(v, 14);
        drop(conn);
        let _ = std::fs::remove_file(&tmp);
        let _ = std::fs::remove_file(tmp.with_extension("db-shm"));
        let _ = std::fs::remove_file(tmp.with_extension("db-wal"));
    }

    // ── v0.6.0 → v0.7.2: Weekly Review seed retired ───────────────
    //
    // The four `seed_weekly_review_*` tests that lived here covered
    // the seeded Weekly Review perspective. v0.7.2 removed the seed
    // (the canonical Review page now renders the same content as
    // its weekly-walk section; the saved-search duplicate was the
    // source of Brandon's "Review and Weekly Review show two
    // different things" confusion). The constants and the
    // seed_initial_perspectives helper are gone; only
    // REVIEW_WEEKLY_WALK_FILTER survives, used by the GUI window
    // when fetching the weekly walk.
}
