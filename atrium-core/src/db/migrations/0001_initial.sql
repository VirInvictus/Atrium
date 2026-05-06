-- 0001_initial.sql
-- Atrium v0.1 schema. The OmniFocus superset — every Builder Mode
-- column exists from day one, even though Simple Mode hides them. See
-- spec.md §4 for the contract and docs/schema.md for the rationale.
--
-- This migration ships once and stays: no mid-v0.1 schema changes
-- (CLAUDE.md commitment). Backwards-compatible migrations begin at v0.2.

-- ============================================================================
-- Areas — top-level grouping. Projects belong to one area or are unfiled.
-- ============================================================================
CREATE TABLE area (
    id          INTEGER PRIMARY KEY,
    uuid        TEXT NOT NULL UNIQUE,
    title       TEXT NOT NULL,
    position    REAL NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    modified_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- ============================================================================
-- Projects — live in an area or unfiled (area_id NULL). Carry GTD state
-- (sequential, review interval, archived). Builder fields exist from v0.1
-- but are hidden in Simple Mode.
-- ============================================================================
CREATE TABLE project (
    id                   INTEGER PRIMARY KEY,
    uuid                 TEXT NOT NULL UNIQUE,
    title                TEXT NOT NULL,
    note                 TEXT NOT NULL DEFAULT '',
    area_id              INTEGER REFERENCES area(id) ON DELETE SET NULL,
    sequential           INTEGER NOT NULL DEFAULT 0,
    review_interval_days INTEGER,
    last_reviewed_at     TEXT,                                            -- ISO datetime
    archived_at          TEXT,                                            -- ISO datetime; NULL = active
    position             REAL NOT NULL,
    created_at           TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    modified_at          TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- ============================================================================
-- Headings — project subdivisions. Builder UI exposes editing in v0.1;
-- Simple displays them inline as section breaks within a project.
-- ============================================================================
CREATE TABLE heading (
    id          INTEGER PRIMARY KEY,
    uuid        TEXT NOT NULL UNIQUE,
    project_id  INTEGER NOT NULL REFERENCES project(id) ON DELETE CASCADE,
    title       TEXT NOT NULL,
    position    REAL NOT NULL,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    modified_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- ============================================================================
-- Tasks — the central row. project_id NULL → Inbox.
-- scheduled_for: ISO date OR the literal '__someday__' sentinel
--                (spec §4.2 — Someday is a state, not a future date).
-- completed_at: NULL = open task; non-NULL = in Logbook.
-- repeat_rule: RFC 5545 RRULE; impl Phase 15.
-- parent_id: subtasks; Builder-only UI in v0.1 (schema supports any depth).
-- ============================================================================
CREATE TABLE task (
    id                INTEGER PRIMARY KEY,
    uuid              TEXT NOT NULL UNIQUE,
    title             TEXT NOT NULL,
    note              TEXT NOT NULL DEFAULT '',
    project_id        INTEGER REFERENCES project(id) ON DELETE CASCADE,
    parent_id         INTEGER REFERENCES task(id)    ON DELETE CASCADE,
    scheduled_for     TEXT,                                              -- ISO date OR '__someday__'
    deadline          TEXT,                                              -- ISO date
    defer_until       TEXT,                                              -- Builder; ISO date
    estimated_minutes INTEGER,                                           -- Builder
    completed_at      TEXT,                                              -- ISO datetime; NULL = open
    repeat_rule       TEXT,                                              -- RFC 5545 RRULE
    position          REAL NOT NULL,
    created_at        TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    modified_at       TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- ============================================================================
-- Tags — orthogonal to areas/projects. NOCASE merges "Errand" and "errand".
-- ============================================================================
CREATE TABLE tag (
    id          INTEGER PRIMARY KEY,
    uuid        TEXT NOT NULL UNIQUE,
    name        TEXT NOT NULL UNIQUE COLLATE NOCASE,
    color       TEXT,                                                   -- '#RRGGBB' or NULL
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    modified_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- ============================================================================
-- Task ↔ Tag many-to-many.
-- ============================================================================
CREATE TABLE task_tag (
    task_id INTEGER NOT NULL REFERENCES task(id) ON DELETE CASCADE,
    tag_id  INTEGER NOT NULL REFERENCES tag(id)  ON DELETE CASCADE,
    PRIMARY KEY (task_id, tag_id)
);

-- ============================================================================
-- Indexes (per roadmap.md Phase 1).
-- ============================================================================

-- Fast list queries by project + completion state. Inbox uses
-- (project_id IS NULL) which this index supports.
CREATE INDEX idx_task_project_completed ON task(project_id, completed_at);

-- Date-axis queries for open tasks (Today, Upcoming, Forecast, defer).
-- Partial indexes — the open-task subset is what these queries scan.
CREATE INDEX idx_task_scheduled_for_open ON task(scheduled_for) WHERE completed_at IS NULL;
CREATE INDEX idx_task_deadline_open      ON task(deadline)      WHERE completed_at IS NULL;
CREATE INDEX idx_task_defer_until_open   ON task(defer_until)   WHERE completed_at IS NULL;

-- Logbook scan — completed tasks ordered by completion time.
CREATE INDEX idx_task_completed_at ON task(completed_at) WHERE completed_at IS NOT NULL;

-- Subtask traversal.
CREATE INDEX idx_task_parent_id ON task(parent_id) WHERE parent_id IS NOT NULL;

-- Project hierarchy + archive filter.
CREATE INDEX idx_project_area_id    ON project(area_id);
CREATE INDEX idx_project_archived   ON project(archived_at);
CREATE INDEX idx_heading_project_id ON heading(project_id);

-- Reverse tag lookup ("show me all tasks with this tag").
CREATE INDEX idx_task_tag_tag_id ON task_tag(tag_id);

-- ============================================================================
-- Full-text search (FTS5).
-- Content-rowid linked to task.id; unicode61 tokenizer (no stemming) per
-- Phase 1 design call — predictability beats fuzzy matching for task search.
-- ============================================================================
CREATE VIRTUAL TABLE task_fts USING fts5(
    title,
    note,
    content='task',
    content_rowid='id',
    tokenize='unicode61'
);

-- Sync triggers — keep task_fts current with task on every write.
CREATE TRIGGER task_fts_insert AFTER INSERT ON task BEGIN
    INSERT INTO task_fts(rowid, title, note)
    VALUES (new.id, new.title, new.note);
END;

CREATE TRIGGER task_fts_delete AFTER DELETE ON task BEGIN
    INSERT INTO task_fts(task_fts, rowid, title, note)
    VALUES ('delete', old.id, old.title, old.note);
END;

CREATE TRIGGER task_fts_update AFTER UPDATE OF title, note ON task BEGIN
    INSERT INTO task_fts(task_fts, rowid, title, note)
    VALUES ('delete', old.id, old.title, old.note);
    INSERT INTO task_fts(rowid, title, note)
    VALUES (new.id, new.title, new.note);
END;

-- ============================================================================
-- modified_at auto-update triggers.
-- The WHEN clause prevents recursion AND lets explicit modified_at writes
-- (e.g., during import preserving original timestamps) survive untouched.
-- ============================================================================
CREATE TRIGGER task_modified_at AFTER UPDATE ON task
WHEN old.modified_at = new.modified_at
BEGIN
    UPDATE task SET modified_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE id = new.id;
END;

CREATE TRIGGER project_modified_at AFTER UPDATE ON project
WHEN old.modified_at = new.modified_at
BEGIN
    UPDATE project SET modified_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE id = new.id;
END;

CREATE TRIGGER area_modified_at AFTER UPDATE ON area
WHEN old.modified_at = new.modified_at
BEGIN
    UPDATE area SET modified_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE id = new.id;
END;

CREATE TRIGGER tag_modified_at AFTER UPDATE ON tag
WHEN old.modified_at = new.modified_at
BEGIN
    UPDATE tag SET modified_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE id = new.id;
END;

CREATE TRIGGER heading_modified_at AFTER UPDATE ON heading
WHEN old.modified_at = new.modified_at
BEGIN
    UPDATE heading SET modified_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE id = new.id;
END;
