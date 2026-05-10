-- 0009_task_clock_entry.sql — v0.17.0
--
-- Phase 18.5 Tier-1 — CLOCK time tracking. New side table that
-- records actual time spent on a task across multiple work
-- sessions (distinct from `task.estimated_minutes`, which is
-- intent). Round-trips to / from Org's `:LOGBOOK:` drawer with
-- `CLOCK: [start]--[end] => HH:MM` lines so Emacs users see
-- the same data.
--
-- Schema:
--
--   id          INTEGER PRIMARY KEY
--   task_id     INTEGER NOT NULL FK→task ON DELETE CASCADE
--               (entries die with their task)
--   started_at  TEXT NOT NULL   ISO datetime, when the clock
--                                started running
--   ended_at    TEXT NULL       ISO datetime when the clock
--                                stopped; NULL = still running
--                                (an open clock). At most one
--                                row in the entire table has
--                                NULL ended_at — the worker
--                                enforces single-active-clock
--                                by closing any other open row
--                                before opening a new one.
--   note        TEXT NOT NULL DEFAULT ''
--                                Optional per-session note (Org's
--                                CLOCK lines support free-form
--                                text after the duration; we
--                                preserve it verbatim).
--
-- Index: (task_id, started_at) supports the inspector log query
-- (entries-per-task ordered by start) and the active-clock
-- lookup. The partial-index variant `WHERE ended_at IS NULL`
-- could narrow the active-clock scan, but the table is small
-- enough that a covering index is overkill — revisit if a
-- power user shows up with thousands of entries.
--
-- Backwards-compatible additive change. v0.16.x binaries reading
-- a v0.17.0 DB ignore the table. user_version 8 → 9.

CREATE TABLE task_clock_entry (
    id         INTEGER PRIMARY KEY,
    task_id    INTEGER NOT NULL REFERENCES task(id) ON DELETE CASCADE,
    started_at TEXT NOT NULL,
    ended_at   TEXT,
    note       TEXT NOT NULL DEFAULT ''
);

CREATE INDEX idx_clock_entry_task_id_started
    ON task_clock_entry(task_id, started_at);
