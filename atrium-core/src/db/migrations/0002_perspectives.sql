-- 0002_perspectives.sql
-- Phase 14 — Perspectives (saved filter expressions).
--
-- The first post-v0.1.0 migration. Purely additive: a new table,
-- no changes to existing tables, no column additions, no type
-- shifts. Spec §4.4's "no mid-v0.1 schema changes" rule was
-- written about breaking changes (column drops, type swaps,
-- semantics shifts on existing rows) — adding a fresh table that
-- existing code doesn't even know about is in a different
-- category. The v0.1.0 git tag still represents Simple Mode's
-- contract correctly; this migration runs cleanly on any v0.1.0
-- database that gets opened by a v0.1.17+ binary.
--
-- A Perspective is a named filter expression (the Phase 7d
-- mini-language: `tag:NAME`, `is:open`, `is:done`, `is:overdue`,
-- `due:today`, plus freeform FTS5 text). v1.0's import/export
-- stories will lean on the `uuid` column for round-trippable
-- portability; the rest of the metadata (icon, custom sort,
-- custom grouping) is carry-forward space for Phase 14.x polish.

CREATE TABLE perspective (
    id           INTEGER PRIMARY KEY,
    uuid         TEXT NOT NULL UNIQUE,
    name         TEXT NOT NULL,
    -- Symbolic icon name (e.g., "starred-symbolic"). NULL → fall
    -- back to the default Perspective icon at render time.
    icon         TEXT,
    -- The filter expression in Phase 7d's mini-language. Stored
    -- verbatim so future versions of the parser can re-evaluate
    -- without DB migrations.
    filter_expr  TEXT NOT NULL,
    -- Custom sort spec. NULL = default order (relevance for FTS5,
    -- position otherwise). Reserved for Phase 14.x polish.
    sort_order   TEXT,
    -- Custom grouping spec. NULL = no grouping. Reserved.
    grouping     TEXT,
    position     REAL NOT NULL,
    created_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    modified_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- modified_at auto-update trigger. The `WHEN old.modified_at =
-- new.modified_at` clause prevents recursion AND lets explicit
-- modified_at writes survive (matches the pattern from
-- 0001_initial.sql for area / project / task / heading).
CREATE TRIGGER perspective_modified_at AFTER UPDATE ON perspective
WHEN old.modified_at = new.modified_at
BEGIN
    UPDATE perspective SET modified_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE id = NEW.id;
END;
