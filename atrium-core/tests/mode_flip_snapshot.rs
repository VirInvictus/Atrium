// SPDX-License-Identifier: MIT
//! Phase 10 acceptance — mode-flip snapshot invariant.
//!
//! Spec §5.3 / CLAUDE.md commitment #1: flipping the GSettings
//! `mode` key is a UI re-render, never a DB write. The mechanism
//! that enforces this is architectural — the UI's only path to
//! the database during a mode flip is `ReadPool`, and `ReadPool`
//! sets `PRAGMA query_only = ON` on every connection so writes
//! error at the SQLite engine level.
//!
//! This integration test exercises the data half of that
//! invariant: spawn a real worker, populate fixtures, snapshot
//! every row of every user table, then attempt to dispatch a
//! "write through the read pool" (the worst case if `apply_mode`
//! ever drifted). The write fails. Snapshot again. Assert equal.
//!
//! The UI's pure-render side of the invariant is enforced by code
//! review of `AtriumWindow::apply_mode` — that function only calls
//! GTK setters, `rebuild_dynamic_sidebar` (read-pool reads), and
//! `refresh_active_list` (read-pool reads). It never holds a
//! `WorkerHandle`.

use atrium_core::db::fixtures::{FixtureScale, generate};
use atrium_core::db::read_pool::ReadPool;
use rusqlite::Connection;

/// One-line digest per user table — count + content checksum
/// derived from a deterministic SELECT. The pair captures both
/// "no rows added/removed" and "no rows mutated".
fn snapshot_db(conn: &Connection) -> Vec<(String, i64, String)> {
    let tables = [
        (
            "area",
            "SELECT id, uuid, title, position, created_at, modified_at FROM area ORDER BY id",
        ),
        (
            "project",
            "SELECT id, uuid, title, note, area_id, sequential, review_interval_days, last_reviewed_at, archived_at, position, created_at, modified_at FROM project ORDER BY id",
        ),
        (
            "task",
            "SELECT id, uuid, title, note, project_id, parent_id, scheduled_for, deadline, defer_until, estimated_minutes, completed_at, repeat_rule, position, created_at, modified_at FROM task ORDER BY id",
        ),
        (
            "tag",
            "SELECT id, uuid, name, color, created_at, modified_at FROM tag ORDER BY id",
        ),
        (
            "task_tag",
            "SELECT task_id, tag_id FROM task_tag ORDER BY task_id, tag_id",
        ),
        (
            "heading",
            "SELECT id, uuid, project_id, title, position, created_at, modified_at FROM heading ORDER BY id",
        ),
    ];

    tables
        .iter()
        .map(|(name, sql)| {
            // Count
            let count: i64 = conn
                .query_row(&format!("SELECT count(*) FROM {name}"), [], |r| r.get(0))
                .unwrap();
            // Content fingerprint: concatenate every column of every
            // row, separated. Cheap, deterministic, captures any
            // mutation. SQL is gnarly but reliable.
            let mut stmt = conn.prepare(sql).unwrap();
            let n_cols = stmt.column_count();
            let mut rows = stmt.query([]).unwrap();
            let mut buf = String::new();
            while let Some(row) = rows.next().unwrap() {
                for c in 0..n_cols {
                    use rusqlite::types::ValueRef;
                    let v = row.get_ref(c).unwrap();
                    match v {
                        ValueRef::Null => buf.push_str("\0NULL"),
                        ValueRef::Integer(i) => {
                            buf.push('\0');
                            buf.push_str(&i.to_string());
                        }
                        ValueRef::Real(f) => {
                            buf.push('\0');
                            buf.push_str(&format!("{f:?}"));
                        }
                        ValueRef::Text(t) => {
                            buf.push('\0');
                            buf.push_str(std::str::from_utf8(t).unwrap_or(""));
                        }
                        ValueRef::Blob(_) => buf.push_str("\0BLOB"),
                    }
                }
                buf.push('\n');
            }
            ((*name).to_string(), count, buf)
        })
        .collect()
}

fn fresh_db(path: &std::path::Path) -> Connection {
    // `db::open` configures pragmas and runs migrations.
    atrium_core::db::open(path).expect("open + migrate test DB")
}

#[test]
fn mode_flip_does_not_touch_db() {
    // 1. Fresh temp DB with the canonical schema, populated with the
    //    Small fixture (1K tasks across 50 projects in 5 areas, 20
    //    tags) — enough volume that any accidental write would
    //    almost certainly perturb the snapshot.
    let dir = tempdir_or_skip();
    let db_path = dir.join("atrium-mode-flip-test.db");

    let mut conn = fresh_db(&db_path);
    generate(&mut conn, FixtureScale::Small).unwrap();
    let snapshot_before = snapshot_db(&conn);
    drop(conn);

    // 2. Open a ReadPool against the populated DB — this is the
    //    only DB handle a mode flip ever touches, transitively.
    let pool = ReadPool::new(db_path.clone(), 2);

    // 3. Simulate the read traffic apply_mode triggers via
    //    rebuild_dynamic_sidebar + refresh_active_list. These reads
    //    are what the UI does on every mode flip.
    let _areas = pool.with(atrium_core::db::read::list_areas).unwrap();
    let _projects = pool.with(atrium_core::db::read::list_projects).unwrap();
    let _tags = pool.with(atrium_core::db::read::list_tags).unwrap();
    let today = chrono::Local::now().date_naive();
    let _today_rows = pool
        .with(|c| atrium_core::db::read::list_today(c, today))
        .unwrap();
    let _counts = pool
        .with(|c| atrium_core::db::read::count_open_canonical(c, today))
        .unwrap();

    // 4. Assert the read pool refuses writes — this is the
    //    architectural reason the contract holds. (Mirrors
    //    `read_only_enforcement_blocks_writes` in atrium-core but
    //    against the populated test DB.)
    let write_result = pool.with(|c| {
        // `?` converts rusqlite::Error → DbError via From. When
        // query_only is on, the execute fails; the closure returns
        // Err(DbError). When query_only is off (regression), the
        // closure returns Ok(rows_affected) — the test panics.
        Ok(c.execute("DELETE FROM task WHERE id = 1", [])?)
    });
    match write_result {
        Ok(_n) => panic!("read pool let a DELETE through — query_only is broken"),
        Err(_e) => {
            // Expected: SQLite rejects writes through query_only
            // connections. The exact message varies across versions
            // ("attempt to write a readonly database"), so we just
            // assert the closure failed.
        }
    }
    drop(pool);

    // 5. Reopen the DB and snapshot again. Assert byte-identical to
    //    step 1 — the read traffic + blocked write left zero trace.
    let conn = Connection::open(&db_path).unwrap();
    let snapshot_after = snapshot_db(&conn);
    drop(conn);

    assert_eq!(
        snapshot_before, snapshot_after,
        "mode-flip simulation perturbed the DB — Phase 10 contract broken"
    );

    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
    let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
    let _ = std::fs::remove_dir(dir);
}

fn tempdir_or_skip() -> std::path::PathBuf {
    let base = std::env::temp_dir();
    let dir = base.join(format!("atrium-mode-flip-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir for mode-flip test");
    dir
}
