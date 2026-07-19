-- 0020_swatch_kanagawa.sql
-- v0.62.0 — Phase 22 C9. Recolour the six built-in tag / area swatches
-- from the old adwaita palette to their Kanagawa Dragon counterparts,
-- in lockstep with the owned stylesheet (atrium/src/ui/theme.rs) and the
-- hex->class lookups (widgets.rs::swatch_class_for_hex,
-- task_list.rs::area_accent_class_for_hex).
--
-- UPDATE-only and append-safe: the colour picker only ever wrote these
-- six exact hexes, so any other value is a user-set colour we leave
-- untouched. No schema change; nothing new to round-trip to the vault
-- (the next project write regenerates the sidecar from the DB, so the
-- new hexes reach the sidecar without a stale re-import).
UPDATE tag  SET color = '#8ba4b0' WHERE color = '#3584e4'; -- blue   -> dragonBlue2
UPDATE tag  SET color = '#87a987' WHERE color = '#33d17a'; -- green  -> dragonGreen
UPDATE tag  SET color = '#c4b28a' WHERE color = '#e5a50a'; -- yellow -> dragonYellow
UPDATE tag  SET color = '#b6927b' WHERE color = '#ff7800'; -- orange -> dragonOrange
UPDATE tag  SET color = '#c4746e' WHERE color = '#e01b24'; -- red    -> dragonRed
UPDATE tag  SET color = '#8992a7' WHERE color = '#9141ac'; -- purple -> dragonViolet

UPDATE area SET color = '#8ba4b0' WHERE color = '#3584e4';
UPDATE area SET color = '#87a987' WHERE color = '#33d17a';
UPDATE area SET color = '#c4b28a' WHERE color = '#e5a50a';
UPDATE area SET color = '#b6927b' WHERE color = '#ff7800';
UPDATE area SET color = '#c4746e' WHERE color = '#e01b24';
UPDATE area SET color = '#8992a7' WHERE color = '#9141ac';
