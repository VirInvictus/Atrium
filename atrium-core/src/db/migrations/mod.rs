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
/// rewrite a shipped migration. v0.1.0 ships with version 1 (the
/// OmniFocus superset); version 2 (Phase 14, v0.1.17) adds the
/// `perspective` table for saved filter views — purely additive, no
/// changes to v0.1's tables. Version 3 (Phase 15, v0.2.0) adds the
/// `repeat_mode` column to `task` for Org-style repeater semantics —
/// the first migration to alter an existing table, allowed because
/// v0.2.0 ends the v0.1 schema freeze.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("0001_initial.sql")),
    (2, include_str!("0002_perspectives.sql")),
    (3, include_str!("0003_repeat_mode.sql")),
];

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
