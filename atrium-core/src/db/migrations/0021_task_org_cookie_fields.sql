-- 0021_task_org_cookie_fields.sql — v0.72.0
--
-- Phase 24 (sweep follow-ups 405): two cookie fragments a user can
-- hand-author in Emacs had no storage, so the writer dropped them on
-- every re-emit (spec §7.3.3 rule 1 violation):
--
--   scheduled_warning_days  INTEGER NULL
--       The `-Nd` / `--Nd` warning suffix on the SCHEDULED cookie.
--       Mirrors `deadline_warn_days` (0008), which already stored
--       the DEADLINE-side warning; Atrium normalises both Org
--       prefixes onto one value, so only the `Nd` number is kept.
--       Only meaningful when `scheduled_for` is a date.
--
--   deadline_repeater       TEXT NULL
--       The repeater fragment on the DEADLINE cookie (`+1m`,
--       `++1w`, `.+3d`) stored verbatim as the cookie text. The
--       writer parses it back onto the emitted cookie; the SCHEDULED
--       side keeps using canonical `:RRULE:` projection (spec
--       §7.3.3 rule 3) — a deadline repeater is a round-trip
--       fidelity field, not a recurrence engine input.
--
-- Backwards-compatible additive change; existing tasks default NULL.
-- user_version 20 → 21.

ALTER TABLE task ADD COLUMN scheduled_warning_days INTEGER NULL;
ALTER TABLE task ADD COLUMN deadline_repeater TEXT NULL;
