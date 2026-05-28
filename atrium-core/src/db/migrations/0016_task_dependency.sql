-- 0016_task_dependency.sql — v0.29.0
--
-- Phase 19.5 / Post-v0.22.0 Tier 2 — task dependencies (`blocked_by`).
-- A task can be blocked by one or more prerequisite tasks; a blocked
-- task is "unavailable" until every prerequisite completes. This is
-- Taskwarrior-parity (the v0.26.0 importer drops `depends` with a
-- lossy-report hint pointing here) and deepens the OmniFocus-superset
-- story.
--
-- One new join table:
--
--   task_dependency(task_id, blocked_by_id)
--       A row (task_id = A, blocked_by_id = B) means "A is blocked by
--       B" — B is a prerequisite of A, and A is unavailable while B is
--       open. CLI spelling: `depend A --on B`.
--
-- FK CASCADE on both ends (mirrors task_clock_entry in 0009): deleting
-- either task drops the dependency rows that reference it. UNIQUE on
-- (task_id, blocked_by_id) makes re-adding the same edge a no-op. The
-- worker enforces the rest (no self-dependency, no cycles) since those
-- can't be expressed cleanly in SQL.
--
-- Indexes both directions: the blocked-state query walks by task_id
-- (a task's prerequisites) and the cascade / available recompute walk
-- by blocked_by_id (a task's dependents).
--
-- Backwards-compatible additive change. v0.28.x binaries reading a
-- v0.29.0 DB ignore the table. user_version 15 → 16.

CREATE TABLE task_dependency (
    id            INTEGER PRIMARY KEY,
    task_id       INTEGER NOT NULL REFERENCES task(id) ON DELETE CASCADE,
    blocked_by_id INTEGER NOT NULL REFERENCES task(id) ON DELETE CASCADE,
    created_at    TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    UNIQUE(task_id, blocked_by_id)
);

CREATE INDEX idx_task_dependency_task       ON task_dependency(task_id);
CREATE INDEX idx_task_dependency_blocked_by ON task_dependency(blocked_by_id);
