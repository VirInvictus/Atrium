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
/// v0.2.0 ends the v0.1 schema freeze. Versions 4 and 5 (Phase 15.75,
/// v0.5.0) add `area.color` and `perspective.{renderer, renderer_config}`
/// for the beauty pass and the kanban-board renderer respectively.
/// Version 6 (v0.7.4) adds `task.last_reviewed_at` for the canonical
/// Review page's task-level Mark Reviewed action. Version 7
/// (v0.7.12) adds `task.orig_keyword` so the Org importer can stash
/// non-canonical keywords (WAITING, BLOCKED, etc.) for round-trip
/// preservation by the writer. Version 8 (v0.14.0, Phase 18.5
/// Tier-1) adds `task.deadline_warn_days` so a per-task warning
/// suffix on the Org DEADLINE cookie (`-Nd` / `--Nd`) can override
/// the global TODAY_DEADLINE_WINDOW_DAYS for that task only.
/// Version 9 (v0.17.0, Phase 18.5 Tier-1) adds the
/// `task_clock_entry` side table for actual-time tracking —
/// distinct from `task.estimated_minutes` (intent). Round-trips
/// through Org's `:LOGBOOK:` drawer. Version 10 (v0.18.0,
/// Phase 18.5 Tier-1) adds the `quick_entry_template` table so
/// the Quick Entry modal can carry multiple templates (the
/// most-cited Org feature in the Worg survey). Version 11
/// (v0.19.0, Phase 18.5 Tier-2) adds `task.scheduled_time` —
/// `HH:MM` companion to the existing date-only `scheduled_for`,
/// closing the Todoist mapper's `DroppedTimeOfDay` lossy entry.
/// Version 12 (v0.20.0, Phase 19.5) adds `task.reminder_at` for
/// time-based notifications; the reminder service polls
/// `next_pending_reminder` and fires `gio::Notification`.
/// Version 13 (v0.21.0, maintenance) adds `created_at` / `modified_at`
/// to `task_clock_entry` — the only table that shipped without them
/// (see 0009). Closes an audit-trail gap surfaced by the v0.21.0
/// maintenance audit. Version 14 (v0.24.0, Post-v0.22.0 Tier 1)
/// adds `task.extra_properties` — a JSON object stashing unmodeled
/// `:PROPERTIES:` drawer keys (anything outside the modeled set of
/// ID / CREATED / MODIFIED / DEFER_UNTIL / EFFORT / RRULE /
/// ORIG_KEYWORD) so custom user keys survive an Org round-trip
/// verbatim. Closes the documented gap pinned by
/// `documented_limit_org_importer_drops_custom_property_keys`
/// and reinforces spec §7.3.3 rule 1 for property drawers.
/// Version 15 (v0.28.0, Post-v0.22.0 Tier 3) adds
/// `area.default_review_interval_days` so an area can set a default
/// Review cadence that cascades to its projects via
/// `COALESCE(project.review_interval_days, area.default_review_interval_days)`
/// in the Review query — a project's own value still wins, the area
/// default only fills in where the project leaves it NULL.
/// Version 16 (v0.29.0, Post-v0.22.0 Tier 2) adds the
/// `task_dependency` join table for `blocked_by` task dependencies:
/// a row `(task_id, blocked_by_id)` means `task_id` is blocked by
/// `blocked_by_id`. FK CASCADE both ends; the worker enforces
/// no-self-dependency and no-cycles. Powers `is:blocked` /
/// `is:available` and the GUI "Blocked" pill.
/// Version 17 (v0.33.0, Phase 19.5) adds `task_template` +
/// `task_template_item` for reusable multi-task project templates
/// (distinct from the single-line `quick_entry_template`); the
/// item table's index-based `parent_index` carries a nested task
/// tree with no task ids, resolved at instantiate time.
/// Version 18 (v0.41.0, Phase 19.5 follow-up) adds
/// `task_reminder_fired`, a side table recording that a reminder
/// fired for a `(task, reminder_at)` pair so the service can fire
/// overdue reminders on launch without re-firing, and so toggling
/// notifications off no longer permanently swallows one.
const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("0001_initial.sql")),
    (2, include_str!("0002_perspectives.sql")),
    (3, include_str!("0003_repeat_mode.sql")),
    (4, include_str!("0004_area_color.sql")),
    (5, include_str!("0005_perspective_renderer.sql")),
    (6, include_str!("0006_task_last_reviewed_at.sql")),
    (7, include_str!("0007_task_orig_keyword.sql")),
    (8, include_str!("0008_task_deadline_warn_days.sql")),
    (9, include_str!("0009_task_clock_entry.sql")),
    (10, include_str!("0010_quick_entry_template.sql")),
    (11, include_str!("0011_task_scheduled_time.sql")),
    (12, include_str!("0012_task_reminder_at.sql")),
    (13, include_str!("0013_task_clock_entry_timestamps.sql")),
    (14, include_str!("0014_task_extra_properties.sql")),
    (15, include_str!("0015_area_default_review_interval.sql")),
    (16, include_str!("0016_task_dependency.sql")),
    (17, include_str!("0017_task_template.sql")),
    (18, include_str!("0018_task_reminder_fired.sql")),
    (19, include_str!("0019_board_card_position.sql")),
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
