// SPDX-License-Identifier: MIT
//! v0.32.0 — database backup + retention.
//!
//! [`backup_now`] writes a defragmented single-file snapshot via
//! SQLite's `VACUUM INTO`, run on a fresh read-only connection so it
//! never contends with the single-writer worker (`VACUUM INTO` is
//! documented to work even on a read-only database and never mutates
//! the source). [`prune`] keeps the newest N snapshots. Restore is a
//! plain file copy queued by the GUI for the next launch (see
//! [`crate::paths::restore_marker_path`]); a VACUUMed file is
//! internally consistent, so the copy needs no special handling.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::{Connection, OpenFlags};

use crate::error::DbError;

/// Write a timestamped snapshot of `db_path` into `dir` and return its
/// path. File name `atrium.<UTC>.db` (filesystem-safe, sortable).
/// Creates `dir` if absent.
pub fn backup_now(db_path: &Path, dir: &Path) -> Result<PathBuf, DbError> {
    fs::create_dir_all(dir)?;
    let stamp = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    // Second-resolution stamps can collide when two backups land in
    // the same second (VACUUM INTO refuses to overwrite). Disambiguate
    // with a `~N` suffix; `~` sorts after `.` so same-second backups
    // still order chronologically and still match the snapshot glob.
    let mut target = dir.join(format!("atrium.{stamp}.db"));
    let mut n = 1;
    while target.exists() {
        target = dir.join(format!("atrium.{stamp}~{n}.db"));
        n += 1;
    }
    // Read-only open: VACUUM INTO reads the source and writes a brand
    // new file, so it never touches the live DB the worker owns.
    let conn = Connection::open_with_flags(db_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.execute("VACUUM INTO ?1", [target.to_string_lossy().as_ref()])?;
    Ok(target)
}

/// Delete all but the newest `keep` snapshots in `dir` (by file name,
/// which sorts chronologically thanks to the UTC timestamp). A no-op
/// when `dir` holds `keep` or fewer. Returns the number removed.
pub fn prune(dir: &Path, keep: usize) -> Result<usize, DbError> {
    let mut snaps = snapshots(dir);
    if snaps.len() <= keep {
        return Ok(0);
    }
    snaps.sort();
    let remove_count = snaps.len() - keep;
    let mut removed = 0;
    for p in snaps.into_iter().take(remove_count) {
        if fs::remove_file(&p).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Newest snapshot in `dir`, if any (by sortable file name).
pub fn latest_backup(dir: &Path) -> Option<PathBuf> {
    let mut snaps = snapshots(dir);
    snaps.sort();
    snaps.pop()
}

fn snapshots(dir: &Path) -> Vec<PathBuf> {
    match fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| is_snapshot(p))
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn is_snapshot(p: &Path) -> bool {
    p.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("atrium.") && n.ends_with(".db"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "atrium-backup-{tag}-{}-{}",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn make_db(path: &Path) {
        let mut conn = Connection::open(path).unwrap();
        crate::db::migrations::migrate(&mut conn).unwrap();
    }

    #[test]
    fn backup_writes_a_readable_snapshot() {
        let root = unique_dir("write");
        let db = root.join("atrium.db");
        make_db(&db);
        let backups = root.join("backups");

        let snap = backup_now(&db, &backups).unwrap();
        assert!(snap.exists());
        // The snapshot opens and carries the schema (task table present).
        let conn = Connection::open_with_flags(&snap, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='task'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn prune_keeps_newest_n() {
        let dir = unique_dir("prune");
        // Five snapshots with sortable, distinct names.
        for stamp in [
            "20260101T000000Z",
            "20260102T000000Z",
            "20260103T000000Z",
            "20260104T000000Z",
            "20260105T000000Z",
        ] {
            fs::write(dir.join(format!("atrium.{stamp}.db")), b"x").unwrap();
        }
        // A non-snapshot file must be left untouched.
        fs::write(dir.join("notes.txt"), b"keep me").unwrap();

        let removed = prune(&dir, 2).unwrap();
        assert_eq!(removed, 3);
        let mut left = snapshots(&dir);
        left.sort();
        let names: Vec<String> = left
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec!["atrium.20260104T000000Z.db", "atrium.20260105T000000Z.db"]
        );
        assert!(dir.join("notes.txt").exists());
        assert_eq!(
            latest_backup(&dir).unwrap().file_name().unwrap(),
            "atrium.20260105T000000Z.db"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn prune_noop_under_limit() {
        let dir = unique_dir("noop");
        fs::write(dir.join("atrium.20260101T000000Z.db"), b"x").unwrap();
        assert_eq!(prune(&dir, 10).unwrap(), 0);
        fs::remove_dir_all(&dir).ok();
    }
}
