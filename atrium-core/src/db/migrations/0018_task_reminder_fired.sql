-- 0018_task_reminder_fired.sql — v0.41.0
--
-- Phase 19.5 follow-up — make reminders trustworthy. The v0.20.0
-- service only ever looked for reminders strictly in the future, so a
-- reminder that came due while Atrium was closed (or while the master
-- notifications toggle was off) was silently missed forever.
--
-- This side table records that a reminder fired for a given
-- (task, reminder_at) pair. With it, the service fires OVERDUE
-- reminders on launch (catch-up) without re-firing them on every poll,
-- and turning notifications off no longer permanently swallows a
-- reminder (it stays unrecorded, so it catches up when re-enabled).
--
-- A side table rather than a task column on purpose: stamping a fire
-- must NOT bump task.modified_at (the AFTER UPDATE trigger would),
-- which would pollute sort:modified and churn the Org vault on every
-- reminder. ON DELETE CASCADE keeps it tidy with the task.
--
-- The (task_id, reminder_at) shape re-arms correctly: if the user
-- moves a reminder to a new time, the stored reminder_at no longer
-- matches t.reminder_at and the reminder fires again. One row per task
-- (PK task_id); the worker does INSERT OR REPLACE on each fire.
--
-- Backfill: mark every existing PAST reminder as already handled so
-- upgrading doesn't blast the user with a burst of historical
-- reminders. Future reminders stay unrecorded (still armed). The
-- boundary uses the same rusqlite '+00:00' shape reminder_at is stored
-- in; sub-second fuzz at the boundary is irrelevant for "is it past."
--
-- Backwards-compatible additive change. user_version 17 → 18.

CREATE TABLE task_reminder_fired (
    task_id     INTEGER NOT NULL PRIMARY KEY REFERENCES task(id) ON DELETE CASCADE,
    reminder_at TEXT    NOT NULL,
    fired_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

INSERT INTO task_reminder_fired (task_id, reminder_at, fired_at)
SELECT id, reminder_at, reminder_at
  FROM task
 WHERE reminder_at IS NOT NULL
   AND reminder_at <= strftime('%Y-%m-%dT%H:%M:%S+00:00', 'now');
