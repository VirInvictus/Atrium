-- 0010_quick_entry_template.sql — v0.18.0
--
-- Phase 18.5 Tier-1 — Quick Entry templates. The most-cited Org
-- feature in the Worg survey (cmdln.org-2024 ships 24 templates;
-- every "how I org" post sets up at least 5). Atrium already
-- ships the Quick Entry modal (`Ctrl+Alt+Space`); this adds
-- template multiplicity so different captures route to different
-- projects with pre-filled prefix + tags.
--
-- Schema:
--
--   id                INTEGER PRIMARY KEY
--   name              TEXT NOT NULL UNIQUE
--                     User-facing label (shown in the picker).
--                     UNIQUE so two templates can't share a name
--                     — the picker would render duplicates as
--                     ambiguous.
--   shortcut_key      TEXT NULL UNIQUE
--                     Single ASCII alphanumeric character; typing
--                     it in the modal selects the template
--                     (Emacs `org-capture` convention). NULL =
--                     no shortcut, picker-button only. UNIQUE
--                     enforces "at most one template per letter."
--   target_project_id INTEGER NULL FK → project ON DELETE SET NULL
--                     Where new captures land. NULL = Inbox
--                     (matches the default Quick Entry behaviour).
--                     SET NULL on delete so removing a project
--                     doesn't cascade and destroy the template.
--   prefix            TEXT NOT NULL DEFAULT ''
--                     Text prepended to the entry's title before
--                     parsing. Useful for routing — e.g. a
--                     "Work" template with prefix "[work] " makes
--                     every captured task title carry the marker.
--   default_tags      TEXT NOT NULL DEFAULT '[]'
--                     JSON array of tag names attached to every
--                     capture from this template (in addition to
--                     any inline `#tag` syntax the user types).
--                     Tags created by name if missing.
--   position          REAL NOT NULL
--                     Display order in the picker. Templates
--                     with lower position render leftmost.
--   created_at, modified_at
--                     ISO datetime; same trigger pattern as
--                     other tables (see 0001_initial.sql).
--
-- Indexes: shortcut_key already has a UNIQUE index (the
-- constraint creates one). No others needed at v0.18.0 — the
-- table stays small (typical user has 5-25 templates) and the
-- picker query is "SELECT * ORDER BY position", which the
-- table scan handles fine.
--
-- modified_at trigger mirrors the WHEN OLD = NEW pattern from
-- the v0.1 schema — explicit modified_at writes survive (the
-- importer + manual edits both want this), recursion is
-- avoided.
--
-- Backwards-compatible additive change. v0.17.x binaries reading
-- a v0.18.0 DB ignore the table. user_version 9 → 10.

CREATE TABLE quick_entry_template (
    id                INTEGER PRIMARY KEY,
    name              TEXT NOT NULL UNIQUE,
    shortcut_key      TEXT UNIQUE,
    target_project_id INTEGER REFERENCES project(id) ON DELETE SET NULL,
    prefix            TEXT NOT NULL DEFAULT '',
    default_tags      TEXT NOT NULL DEFAULT '[]',
    position          REAL NOT NULL,
    created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    modified_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TRIGGER trg_quick_entry_template_modified
AFTER UPDATE ON quick_entry_template
FOR EACH ROW
WHEN OLD.modified_at = NEW.modified_at
BEGIN
    UPDATE quick_entry_template
    SET modified_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE id = NEW.id;
END;
