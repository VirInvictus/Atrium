-- 0013_task_clock_entry_timestamps.sql — v0.21.0
--
-- Maintenance pass — close an audit-trail gap that landed in v0.17.0.
-- Every other table in the schema (task, project, area, tag, heading,
-- perspective, quick_entry_template) carries `created_at` /
-- `modified_at` columns plus an AFTER UPDATE trigger that bumps
-- `modified_at` on every change. `task_clock_entry` shipped without
-- them (see migration 0009) — there's no in-DB trail for "when did
-- Atrium last touch this clock row?". Watcher edits via
-- import_clock_entry, GUI edits, and CLI edits all leave the same
-- (invisible) signal.
--
-- Two new columns + one trigger close the gap:
--
--   created_at   TEXT NULL  ISO datetime — when the row was first
--                            inserted. Backfilled to `started_at` for
--                            existing rows (the closest approximation
--                            of the original create event we have).
--   modified_at  TEXT NULL  ISO datetime — bumped by trigger on every
--                            UPDATE. Backfilled to ended_at when set,
--                            else started_at.
--
-- Columns are nullable rather than NOT NULL because SQLite's
-- ALTER TABLE ADD COLUMN doesn't allow function-valued DEFAULTs
-- (strftime(...) is rejected). The worker is the single writer and
-- always stamps these on INSERT; readers never check for NULL.
--
-- Trigger uses the same `WHEN old = new` guard pattern as every other
-- table's modified_at trigger (see 0001_initial.sql) — prevents
-- recursion and lets explicit caller-provided timestamps survive
-- (important for import paths that thread the source file's
-- timestamps directly through).
--
-- Backwards-compatible additive change. v0.20.x binaries reading a
-- v0.21.0 DB ignore the columns. user_version 12 → 13.

ALTER TABLE task_clock_entry ADD COLUMN created_at TEXT;
ALTER TABLE task_clock_entry ADD COLUMN modified_at TEXT;

-- Backfill existing rows. created_at = started_at (best approximation);
-- modified_at = ended_at when set, otherwise started_at.
UPDATE task_clock_entry
   SET created_at = started_at,
       modified_at = COALESCE(ended_at, started_at)
 WHERE created_at IS NULL;

CREATE TRIGGER clock_entry_modified_at AFTER UPDATE ON task_clock_entry
WHEN old.modified_at = new.modified_at
BEGIN
    UPDATE task_clock_entry SET modified_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE id = new.id;
END;
