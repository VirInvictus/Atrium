-- 0017_task_template.sql — v0.33.0
--
-- Phase 19.5 — task templates. A reusable project shape (a named set
-- of tasks, optionally nested, with per-item tags + estimates) that
-- instantiates into a fresh project. Distinct from the single-line
-- Quick Entry templates in `quick_entry_template` (v0.18.0): those
-- pre-fill one capture; these stamp out a whole project tree.
--
-- Two tables:
--
--   task_template          one row per named template.
--     project_title_seed   title for the project an instantiate
--                          creates (empty == fall back to `name`).
--     note                 seeds the new project's note.
--     tags_json            JSON array of tag names applied to every
--                          task the template instantiates (merged
--                          with each item's own tags). Mirrors the
--                          `quick_entry_template.default_tags` shape.
--
--   task_template_item     the tasks the template stamps out.
--     parent_index         index (into this template's item list,
--                          ordered by `position`) of this item's
--                          parent, or NULL for a top-level task. The
--                          worker resolves it to a real `parent_id`
--                          at instantiate time. Index-based so the
--                          template is self-contained (no task ids).
--     default_tags_json    JSON array of per-item tag names.
--
-- Backwards-compatible additive change. v0.32.x binaries reading a
-- v0.33.0 DB ignore the tables. user_version 16 → 17.

CREATE TABLE task_template (
    id                 INTEGER PRIMARY KEY,
    uuid               TEXT NOT NULL UNIQUE,
    name               TEXT NOT NULL UNIQUE,
    project_title_seed TEXT NOT NULL DEFAULT '',
    note               TEXT NOT NULL DEFAULT '',
    tags_json          TEXT NOT NULL DEFAULT '[]',
    created_at         TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    modified_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE task_template_item (
    id                INTEGER PRIMARY KEY,
    template_id       INTEGER NOT NULL REFERENCES task_template(id) ON DELETE CASCADE,
    title             TEXT NOT NULL,
    parent_index      INTEGER,
    position          REAL NOT NULL,
    estimated_minutes INTEGER,
    default_tags_json TEXT NOT NULL DEFAULT '[]'
);

CREATE INDEX idx_task_template_item_template ON task_template_item(template_id);

CREATE TRIGGER trg_task_template_modified
AFTER UPDATE ON task_template
FOR EACH ROW
WHEN OLD.modified_at = NEW.modified_at
BEGIN
    UPDATE task_template
    SET modified_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE id = NEW.id;
END;
