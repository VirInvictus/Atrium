-- 0004_area_color.sql — Phase 15.75 (v0.5.0)
--
-- Adds a `color` column to the `area` table so each area can carry an
-- optional accent. Mirrors the v0.3.0 `tag.color` shape — a hex string
-- like `#3584e4`, NULL for areas with no chosen accent.
--
-- The Slice B "per-area accent" treatment renders a 3 px left border on
-- task rows that belong to a project under a coloured area, so users
-- can see at a glance which area a cross-list task came from. The
-- column lands here with no UI consumer yet; Slice B wires it through.
--
-- Backwards-compatible additive change. Existing INSERT INTO area
-- statements (worker `create_area`) still work unchanged because the
-- column is nullable; the worker is updated alongside this migration
-- to write the column when supplied.

ALTER TABLE area ADD COLUMN color TEXT;
