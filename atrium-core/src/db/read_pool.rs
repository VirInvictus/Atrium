// SPDX-License-Identifier: MIT
//! Read-only connection pool.
//!
//! The single-writer worker (`db::worker`) holds the writable
//! connection. Read-side queries from the UI thread go through this
//! pool — separate `rusqlite::Connection`s with `PRAGMA query_only =
//! ON`, so SQLite enforces read-only at the engine level.
//!
//! Implementation: lazy on-demand. Connections are opened when the
//! pool is empty and returned on release. The pool caps idle
//! connections at `max_size`; excess connections are dropped instead
//! of pooled. Total open connections are not capped — under heavy
//! concurrent reads we may briefly exceed `max_size`, but WAL allows
//! it and idle excess is reclaimed on release.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use tracing::trace;

use crate::error::DbError;

#[derive(Clone)]
pub struct ReadPool {
    db_path: PathBuf,
    inner: Arc<Mutex<Vec<Connection>>>,
    max_size: usize,
}

impl ReadPool {
    /// Create a pool against `db_path`, retaining up to `max_size`
    /// idle read connections.
    pub fn new(db_path: impl Into<PathBuf>, max_size: usize) -> Self {
        Self {
            db_path: db_path.into(),
            inner: Arc::new(Mutex::new(Vec::new())),
            max_size,
        }
    }

    /// Run `f` with a read-only connection. The connection is
    /// returned to the pool on completion (or dropped if the pool is
    /// full).
    pub fn with<F, R>(&self, f: F) -> Result<R, DbError>
    where
        F: FnOnce(&Connection) -> Result<R, DbError>,
    {
        let conn = self.acquire()?;
        let result = f(&conn);
        self.release(conn);
        result
    }

    /// Number of idle connections currently pooled. Test/debug use.
    pub fn idle_count(&self) -> usize {
        self.inner.lock().unwrap().len()
    }

    pub fn max_size(&self) -> usize {
        self.max_size
    }

    fn acquire(&self) -> Result<Connection, DbError> {
        if let Some(conn) = self.inner.lock().unwrap().pop() {
            trace!("read pool: reused idle connection");
            return Ok(conn);
        }
        trace!(path = %self.db_path.display(), "read pool: opening fresh connection");
        let conn = Connection::open(&self.db_path)?;
        crate::db::configure_pragmas(&conn)?;
        // Engine-level read-only enforcement: prevents accidental writes.
        conn.pragma_update(None, "query_only", "ON")?;
        Ok(conn)
    }

    fn release(&self, conn: Connection) {
        let mut guard = self.inner.lock().unwrap();
        if guard.len() < self.max_size {
            guard.push(conn);
        }
        // else: drop — too many idle.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use std::path::PathBuf;

    fn tmp_db_with_schema() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("atrium-readpool-test-{}.db", uuid::Uuid::new_v4()));
        let _conn = db::open(&path).unwrap();
        path
    }

    fn cleanup(path: &PathBuf) {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(path.with_extension("db-shm"));
        let _ = std::fs::remove_file(path.with_extension("db-wal"));
    }

    #[test]
    fn acquire_release_round_trips() {
        let path = tmp_db_with_schema();
        let pool = ReadPool::new(&path, 4);
        assert_eq!(pool.idle_count(), 0);

        pool.with(|conn| {
            let v: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
            assert_eq!(v, 18);
            Ok(())
        })
        .unwrap();

        assert_eq!(pool.idle_count(), 1, "connection should return to pool");
        cleanup(&path);
    }

    #[test]
    fn read_only_enforcement_blocks_writes() {
        let path = tmp_db_with_schema();
        let pool = ReadPool::new(&path, 4);
        let result = pool.with(|conn| {
            conn.execute(
                "INSERT INTO task (uuid, title, position) VALUES ('x', 'y', 1.0)",
                [],
            )
            .map_err(crate::error::DbError::from)
        });
        assert!(result.is_err(), "query_only should reject writes");
        cleanup(&path);
    }

    #[test]
    fn pool_cap_drops_excess() {
        let path = tmp_db_with_schema();
        let pool = ReadPool::new(&path, 2);
        // Acquire 3 connections, return all of them: only 2 pool.
        let c1 = pool.acquire().unwrap();
        let c2 = pool.acquire().unwrap();
        let c3 = pool.acquire().unwrap();
        pool.release(c1);
        pool.release(c2);
        pool.release(c3);
        assert_eq!(pool.idle_count(), 2);
        cleanup(&path);
    }
}
