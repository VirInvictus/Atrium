-- 0019_board_card_position.sql
-- v0.46.0 — persisted intra-column ordering for kanban boards
-- (kanban maturity part 2d).
--
-- Columns stay a projection of task fields (tag or Org status); this
-- side table only records the manual within-column order a user set by
-- dragging, keyed by (perspective, column value, task). It carries no
-- Org meaning (like reminders / the fired-reminder table), so it never
-- round-trips to the vault. Rows are cleaned up automatically when a
-- perspective or a task is deleted (FK CASCADE both ends).
--
-- `column_key` is the column value (tag name or status keyword) stored
-- lowercased by the writer, matching the case-insensitive column match
-- used everywhere else. The PK doubles as the lookup index: ordering a
-- column is a prefix scan on (perspective_id, column_key).
CREATE TABLE IF NOT EXISTS board_card_position (
    perspective_id INTEGER NOT NULL REFERENCES perspective(id) ON DELETE CASCADE,
    column_key     TEXT    NOT NULL,
    task_id        INTEGER NOT NULL REFERENCES task(id) ON DELETE CASCADE,
    position       INTEGER NOT NULL,
    PRIMARY KEY (perspective_id, column_key, task_id)
);
