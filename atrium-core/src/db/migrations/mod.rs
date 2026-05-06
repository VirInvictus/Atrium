// SPDX-License-Identifier: MIT
//! Migration runner.
//!
//! `PRAGMA user_version` drives migration state. Each migration ships
//! as embedded SQL via `include_str!`, applied inside a single
//! transaction. `user_version` is one of the few PRAGMAs that
//! participates in transactions, so a failed migration rolls back
//! cleanly without leaving the schema half-applied.

use rusqlite::Connection;
use tracing::info;

use crate::error::DbError;

/// Ordered list of `(version, sql)` migrations. Append-only; never
/// rewrite a shipped migration. v0.1 lives at version 1; v0.2's first
/// migration would land at version 2.
const MIGRATIONS: &[(i64, &str)] = &[(1, include_str!("0001_initial.sql"))];

/// Apply any pending migrations to `conn`.
///
/// Idempotent: running on an already-migrated database is a no-op.
/// Each migration runs inside a transaction; on failure, the schema
/// stays at the previous version.
pub fn migrate(conn: &mut Connection) -> Result<(), DbError> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;

    for (version, sql) in MIGRATIONS {
        if *version > current {
            info!(version, "applying migration");
            let tx = conn.transaction()?;
            tx.execute_batch(sql).map_err(|source| DbError::Migration {
                version: *version,
                source,
            })?;
            tx.pragma_update(None, "user_version", version)?;
            tx.commit()?;
            info!(version, "migration applied");
        }
    }

    Ok(())
}
