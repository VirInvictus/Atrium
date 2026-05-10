-- 0012_task_reminder_at.sql — v0.20.0
--
-- Phase 19.5 — system notifications / time-based reminders.
-- Each task can carry an optional reminder timestamp; when the
-- wall clock passes it AND the task is still open, the GUI
-- fires `gio::Notification` with the task title.
--
-- One new column on `task`:
--
--   reminder_at   TEXT NULL
--                 ISO datetime; NULL means "no reminder set."
--                 Independent of `scheduled_for` and `deadline`
--                 — a reminder can fire on a task without any
--                 schedule (a "ping me at 3 PM about this thing"
--                 capture). Single per task in v0.20.0; multiple
--                 reminders or recurring reminders are deferred
--                 until users ask.
--
-- The reminder service (atrium/src/reminders.rs) reads
-- `next_pending_reminder` to find the soonest unfired reminder
-- and sleeps until that wall-clock time. Re-queries on every
-- TaskChanges so newly-set reminders take effect immediately
-- without a service restart.
--
-- Backwards-compatible additive change. v0.19.x binaries reading
-- a v0.20.0 DB ignore the column. user_version 11 → 12.

ALTER TABLE task ADD COLUMN reminder_at TEXT NULL;

-- Partial index on open tasks with a future reminder. Speeds
-- up the `next_pending_reminder` query — it only ever scans
-- this slice, not the full task table.
CREATE INDEX idx_task_reminder_at_open
    ON task(reminder_at)
    WHERE reminder_at IS NOT NULL AND completed_at IS NULL;
