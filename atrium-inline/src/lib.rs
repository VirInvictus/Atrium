// SPDX-License-Identifier: MIT
//! Inline-syntax parser shared by every Atrium capture surface.
//!
//! Lifted out of `atrium-core::quick_entry` in v0.13.0 (atrium-inline
//! Slice 3) so future surfaces (the post-1.0 `atrium-tui`, the
//! optional `atriumd` capture daemon, any future inline-rename
//! variant) can speak the same vocabulary without depending on the
//! storage layer. atrium-core stays inline-syntax-agnostic; the
//! extraction goes one way, atrium-inline → atrium-core.
//!
//! Used by:
//!
//! - The bottom-of-list entry (Phase 6b).
//! - The Quick Entry modal (Phase 6c).
//! - The CLI's `capture` subcommand.
//! - The GTK inline-rename surface (v0.13 Slice 1+).
//!
//! Supported syntax (per spec.md §6):
//!
//! - `#errand` — attach the tag named `errand` (case-insensitive,
//!   created on first use by the calling code).
//! - `@today` / `@tomorrow` / `@someday` — set `scheduled_for`.
//! - `@yyyy-mm-dd` — set `scheduled_for` to a specific date.
//! - `@<weekday>` — set `scheduled_for` to the next occurrence of
//!   that weekday on or after `today`. Both 3-letter (`@mon`) and
//!   full-name (`@monday`) forms are accepted, case-insensitive.
//!   v0.13 Slice 2.
//! - `@deadline yyyy-mm-dd` — set `deadline`.
//! - `!1` / `!2` / `!3` — set `priority` to the named level
//!   (1 = high, 3 = low — matching Todoist's convention; 4 is the
//!   default "no priority" and emits no token). Single-valued —
//!   the surface decides whether to emit one `priority-N` tag or
//!   write a numeric column when Phase 19.5 lands. v0.13 Slice 2.
//!
//! Anything else is title text. Unrecognised `@foo` / `!foo`
//! strings stay in the title verbatim — no silent data loss.

pub mod completions;

use chrono::{Datelike, Local, NaiveDate, Weekday};

use atrium_core::ScheduledFor;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParsedEntry {
    pub title: String,
    pub tag_names: Vec<String>,
    pub scheduled_for: Option<ScheduledFor>,
    pub deadline: Option<NaiveDate>,
    /// v0.13 Slice 2 — Todoist-style priority level from a `!N`
    /// token (`!1` = high, `!2` = medium, `!3` = low). `None` when
    /// no `!N` appeared in the input. The level stays single-valued
    /// even if the user typed multiple `!N` tokens (last one wins,
    /// matching how `@today` overwrites `@tomorrow`).
    ///
    /// Until Phase 19.5 ships a numeric `priority` column, surfaces
    /// project this onto a `priority-N` tag. The atomic enum value
    /// stays so the rename surface can swap one priority tag for
    /// another atomically (single-valued semantics) without losing
    /// the user's free-form `#tag` set.
    pub priority: Option<u8>,
}

impl ParsedEntry {
    /// True when the parsed input carries no inline-syntax artefacts
    /// — the user typed plain title text and nothing else. Lets a
    /// surface that uses [`parse`] take a fast path identical to the
    /// pre-parser behaviour (a single title-only update with no tag
    /// or schedule side effects). Used by the v0.13 inline-rename
    /// path in the GTK binary so a rename of plain text behaves
    /// exactly as it did before quick_entry was wired in.
    pub fn is_plain_title(&self) -> bool {
        self.tag_names.is_empty()
            && self.scheduled_for.is_none()
            && self.deadline.is_none()
            && self.priority.is_none()
    }

    /// `tag_names` augmented with a `priority-N` projection when
    /// `priority` is set. Capture-flavoured surfaces (Quick Entry
    /// modal, bottom-of-list entry, CLI `capture`) use this so the
    /// `!1` token shows up as a `priority-1` tag on the new task
    /// without callers having to know about the projection rule.
    /// The rename surface deliberately doesn't use this — it
    /// reads `priority` directly so it can swap out any existing
    /// `priority-*` tag for the new one (single-valued semantics).
    pub fn projected_tag_names(&self) -> Vec<String> {
        let mut out = self.tag_names.clone();
        if let Some(level) = self.priority {
            let proj = format!("priority-{level}");
            if !out.iter().any(|t| t.eq_ignore_ascii_case(&proj)) {
                out.push(proj);
            }
        }
        out
    }
}

/// True when `name` is a `priority-N` tag for a Todoist-style
/// priority level (1, 2, or 3). Used by the rename surface to
/// strip a stale priority tag before installing the new one when
/// the parsed input carries an explicit `!N`.
pub fn is_priority_tag_name(name: &str) -> bool {
    name.strip_prefix("priority-")
        .and_then(|rest| rest.parse::<u8>().ok())
        .is_some_and(|n| (1..=3).contains(&n))
}

pub fn parse(input: &str) -> ParsedEntry {
    parse_with_today(input, Local::now().date_naive())
}

/// `parse` with an injectable "today" so tests are deterministic.
pub fn parse_with_today(input: &str, today: NaiveDate) -> ParsedEntry {
    let mut title_parts: Vec<&str> = Vec::new();
    let mut tag_names: Vec<String> = Vec::new();
    let mut scheduled_for: Option<ScheduledFor> = None;
    let mut deadline: Option<NaiveDate> = None;
    let mut priority: Option<u8> = None;

    let words: Vec<&str> = input.split_whitespace().collect();
    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        if let Some(tag) = word.strip_prefix('#') {
            if !tag.is_empty() {
                tag_names.push(tag.to_string());
            } else {
                title_parts.push(word);
            }
        } else if let Some(rest) = word.strip_prefix('!') {
            // v0.13 Slice 2 — `!1` / `!2` / `!3`. Anything else
            // (`!none`, `!4`, `!9`, `!high`) stays in the title
            // verbatim. The strict "1..=3" range matches Todoist's
            // user-facing priority levels (4 is "no priority").
            if let Some(level) = parse_priority_level(rest) {
                priority = Some(level);
            } else {
                title_parts.push(word);
            }
        } else if word == "@today" {
            scheduled_for = Some(ScheduledFor::Date(today));
        } else if word == "@tomorrow" {
            scheduled_for = Some(ScheduledFor::Date(today + chrono::Duration::days(1)));
        } else if word == "@someday" {
            scheduled_for = Some(ScheduledFor::Someday);
        } else if word == "@deadline" {
            // Look ahead one word for the date.
            if let Some(next) = words.get(i + 1)
                && let Ok(d) = NaiveDate::parse_from_str(next, "%Y-%m-%d")
            {
                deadline = Some(d);
                i += 1;
            } else {
                title_parts.push(word);
            }
        } else if let Some(after_at) = word.strip_prefix('@') {
            // ISO date wins (so `@2026-05-15` always parses as a
            // date even if "2026-05-15" might somehow tokenise as
            // a weekday). Then weekday names. Anything else falls
            // through as title text — Slice 1's "unknown @foo
            // stays in title" contract carries forward unchanged.
            if let Ok(d) = NaiveDate::parse_from_str(after_at, "%Y-%m-%d") {
                scheduled_for = Some(ScheduledFor::Date(d));
            } else if let Some(w) = parse_weekday(after_at) {
                scheduled_for = Some(ScheduledFor::Date(next_weekday(today, w)));
            } else {
                title_parts.push(word);
            }
        } else {
            title_parts.push(word);
        }
        i += 1;
    }

    ParsedEntry {
        title: title_parts.join(" "),
        tag_names,
        scheduled_for,
        deadline,
        priority,
    }
}

/// Parse the text after a `!` token. Returns `Some(1)` / `Some(2)`
/// / `Some(3)` for the three Todoist-style priority levels, `None`
/// for anything else (so the source `!foo` falls through to the
/// title). Strict — exactly one digit, no leading zeros, no
/// trailing characters.
fn parse_priority_level(rest: &str) -> Option<u8> {
    if rest.len() != 1 {
        return None;
    }
    match rest.chars().next()? {
        '1' => Some(1),
        '2' => Some(2),
        '3' => Some(3),
        _ => None,
    }
}

/// Lower-cased weekday names accepted after `@`. Order matters
/// only for the human-readable `WEEKDAY_NAMES` test fixture; the
/// lookup walks the array linearly.
const WEEKDAY_NAMES: &[(&str, Weekday)] = &[
    ("monday", Weekday::Mon),
    ("mon", Weekday::Mon),
    ("tuesday", Weekday::Tue),
    ("tues", Weekday::Tue),
    ("tue", Weekday::Tue),
    ("wednesday", Weekday::Wed),
    ("weds", Weekday::Wed),
    ("wed", Weekday::Wed),
    ("thursday", Weekday::Thu),
    ("thurs", Weekday::Thu),
    ("thur", Weekday::Thu),
    ("thu", Weekday::Thu),
    ("friday", Weekday::Fri),
    ("fri", Weekday::Fri),
    ("saturday", Weekday::Sat),
    ("sat", Weekday::Sat),
    ("sunday", Weekday::Sun),
    ("sun", Weekday::Sun),
];

fn parse_weekday(s: &str) -> Option<Weekday> {
    let lower = s.to_ascii_lowercase();
    WEEKDAY_NAMES
        .iter()
        .find(|(name, _)| *name == lower.as_str())
        .map(|(_, w)| *w)
}

/// Next occurrence of `target` on or after `today`. Same shape as
/// the Todoist importer's helper — when today's weekday matches
/// the target, returns today rather than skipping a week. The
/// "you typed `@mon` on a Monday, you probably mean today" call.
fn next_weekday(today: NaiveDate, target: Weekday) -> NaiveDate {
    let today_off = today.weekday().num_days_from_monday() as i64;
    let target_off = target.num_days_from_monday() as i64;
    let mut delta = target_off - today_off;
    if delta < 0 {
        delta += 7;
    }
    today + chrono::Duration::days(delta)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn t() -> NaiveDate {
        d(2026, 5, 15)
    }

    #[test]
    fn plain_title() {
        let p = parse_with_today("Buy milk", t());
        assert_eq!(p.title, "Buy milk");
        assert!(p.tag_names.is_empty());
        assert!(p.scheduled_for.is_none());
        assert!(p.deadline.is_none());
    }

    #[test]
    fn is_plain_title_branches() {
        // Pure title — fast-path eligible.
        assert!(parse_with_today("Buy milk", t()).is_plain_title());
        // Empty input — also plain (no syntax artefacts).
        assert!(parse_with_today("", t()).is_plain_title());
        // Tag — extended path.
        assert!(!parse_with_today("Buy milk #errand", t()).is_plain_title());
        // Scheduled — extended path.
        assert!(!parse_with_today("Call dentist @today", t()).is_plain_title());
        // Deadline — extended path.
        assert!(!parse_with_today("File taxes @deadline 2026-04-15", t()).is_plain_title());
        // Unrecognised `@foo` stays in the title — still plain (no
        // side effects produced by the parse).
        assert!(parse_with_today("Email @foo", t()).is_plain_title());
    }

    #[test]
    fn single_tag() {
        let p = parse_with_today("Buy milk #errand", t());
        assert_eq!(p.title, "Buy milk");
        assert_eq!(p.tag_names, vec!["errand"]);
    }

    #[test]
    fn multiple_tags() {
        let p = parse_with_today("Buy milk #errand #urgent", t());
        assert_eq!(p.title, "Buy milk");
        assert_eq!(p.tag_names, vec!["errand", "urgent"]);
    }

    #[test]
    fn at_today() {
        let p = parse_with_today("Call dentist @today", t());
        assert_eq!(p.title, "Call dentist");
        assert_eq!(p.scheduled_for, Some(ScheduledFor::Date(t())));
    }

    #[test]
    fn at_tomorrow() {
        let p = parse_with_today("Call dentist @tomorrow", t());
        assert_eq!(p.scheduled_for, Some(ScheduledFor::Date(d(2026, 5, 16))));
    }

    #[test]
    fn at_someday() {
        let p = parse_with_today("Learn Welsh @someday", t());
        assert_eq!(p.scheduled_for, Some(ScheduledFor::Someday));
    }

    #[test]
    fn at_iso_date() {
        let p = parse_with_today("Send report @2026-06-15", t());
        assert_eq!(p.scheduled_for, Some(ScheduledFor::Date(d(2026, 6, 15))));
    }

    #[test]
    fn at_deadline() {
        let p = parse_with_today("File taxes @deadline 2026-04-15", t());
        assert_eq!(p.title, "File taxes");
        assert_eq!(p.deadline, Some(d(2026, 4, 15)));
    }

    #[test]
    fn unknown_at_word_stays_in_title() {
        let p = parse_with_today("Email @brandon about Q3", t());
        assert_eq!(p.title, "Email @brandon about Q3");
        assert!(p.scheduled_for.is_none());
    }

    #[test]
    fn lone_hash_stays_in_title() {
        let p = parse_with_today("Fix # symbol rendering", t());
        assert!(p.tag_names.is_empty());
        assert_eq!(p.title, "Fix # symbol rendering");
    }

    #[test]
    fn combined_syntax() {
        let p = parse_with_today("Buy milk #errand #grocery @today @deadline 2026-05-20", t());
        assert_eq!(p.title, "Buy milk");
        assert_eq!(p.tag_names, vec!["errand", "grocery"]);
        assert_eq!(p.scheduled_for, Some(ScheduledFor::Date(t())));
        assert_eq!(p.deadline, Some(d(2026, 5, 20)));
    }

    #[test]
    fn whitespace_collapsed() {
        let p = parse_with_today("  Buy   milk    ", t());
        assert_eq!(p.title, "Buy milk");
    }

    // ── v0.13 Slice 2: !priority ────────────────────────────────

    #[test]
    fn priority_levels_one_two_three() {
        for (token, expected) in [("!1", 1u8), ("!2", 2), ("!3", 3)] {
            let p = parse_with_today(&format!("Task {token}"), t());
            assert_eq!(p.title, "Task");
            assert_eq!(p.priority, Some(expected), "{token}");
        }
    }

    #[test]
    fn priority_extended_path_not_plain() {
        assert!(!parse_with_today("Task !1", t()).is_plain_title());
    }

    #[test]
    fn priority_invalid_levels_stay_in_title() {
        // 0 / 4 / 9 aren't part of the Todoist 1-3 contract.
        for token in ["!0", "!4", "!9", "!12", "!high", "!none", "!"] {
            let input = format!("Task {token}");
            let p = parse_with_today(&input, t());
            assert!(
                p.priority.is_none(),
                "{token} shouldn't set priority; got {:?}",
                p.priority
            );
            assert!(
                p.title.contains(token),
                "{token} should stay in the title; got {:?}",
                p.title
            );
        }
    }

    #[test]
    fn priority_last_token_wins() {
        // Multiple `!N` tokens — the last one survives, matching
        // the @today/@tomorrow override semantics.
        let p = parse_with_today("Task !1 !3", t());
        assert_eq!(p.priority, Some(3));
        assert_eq!(p.title, "Task");
    }

    #[test]
    fn priority_with_other_tokens() {
        let p = parse_with_today("Pay rent #urgent !1 @today", t());
        assert_eq!(p.title, "Pay rent");
        assert_eq!(p.tag_names, vec!["urgent"]);
        assert_eq!(p.priority, Some(1));
        assert_eq!(p.scheduled_for, Some(ScheduledFor::Date(t())));
    }

    // ── v0.13 Slice 2: @weekday ─────────────────────────────────

    #[test]
    fn weekday_three_letter_resolves_to_next_occurrence() {
        // t() = 2026-05-15 (Friday). @mon → next Monday = May 18.
        let p = parse_with_today("Plan week @mon", t());
        assert_eq!(p.title, "Plan week");
        assert_eq!(p.scheduled_for, Some(ScheduledFor::Date(d(2026, 5, 18))),);
    }

    #[test]
    fn weekday_full_name_resolves_to_next_occurrence() {
        let p = parse_with_today("Plan week @monday", t());
        assert_eq!(p.scheduled_for, Some(ScheduledFor::Date(d(2026, 5, 18))),);
    }

    #[test]
    fn weekday_today_returns_today() {
        // t() is Friday; @fri should return today, not a week out.
        let p = parse_with_today("Same-day prep @fri", t());
        assert_eq!(p.scheduled_for, Some(ScheduledFor::Date(t())));
    }

    #[test]
    fn weekday_case_insensitive() {
        for token in ["@MON", "@Mon", "@mOn", "@MONDAY", "@Monday"] {
            let p = parse_with_today(&format!("Task {token}"), t());
            assert_eq!(
                p.scheduled_for,
                Some(ScheduledFor::Date(d(2026, 5, 18))),
                "{token}"
            );
        }
    }

    #[test]
    fn weekday_all_seven_days() {
        // t() = Fri 2026-05-15. Walk all seven weekdays starting from Sat.
        let cases = [
            ("@sat", d(2026, 5, 16)),
            ("@sun", d(2026, 5, 17)),
            ("@mon", d(2026, 5, 18)),
            ("@tue", d(2026, 5, 19)),
            ("@wed", d(2026, 5, 20)),
            ("@thu", d(2026, 5, 21)),
            ("@fri", t()), // today
        ];
        for (token, expected) in cases {
            let p = parse_with_today(&format!("Task {token}"), t());
            assert_eq!(
                p.scheduled_for,
                Some(ScheduledFor::Date(expected)),
                "{token}",
            );
        }
    }

    #[test]
    fn weekday_aliases_resolve_correctly() {
        // Alternate spellings the parser accepts.
        for (token, expected) in [
            ("@tues", Weekday::Tue),
            ("@weds", Weekday::Wed),
            ("@thur", Weekday::Thu),
            ("@thurs", Weekday::Thu),
        ] {
            let p = parse_with_today(&format!("X {token}"), t());
            assert_eq!(
                p.scheduled_for,
                Some(ScheduledFor::Date(next_weekday(t(), expected))),
                "{token}",
            );
        }
    }

    #[test]
    fn iso_date_beats_weekday_lookalike() {
        // Defensive check: ISO date format always wins over weekday
        // parsing even though no weekday name parses as YYYY-MM-DD.
        let p = parse_with_today("Plan @2026-05-18", t());
        assert_eq!(p.scheduled_for, Some(ScheduledFor::Date(d(2026, 5, 18))),);
    }

    #[test]
    fn unknown_at_word_still_falls_through_to_title() {
        // Regression guard for Slice 1's unknown-@foo contract.
        // Names that aren't weekdays (and aren't recognised tokens)
        // still stay in the title verbatim.
        let p = parse_with_today("Email @brandon about Q3", t());
        assert_eq!(p.title, "Email @brandon about Q3");
        assert!(p.scheduled_for.is_none());
    }

    // ── projected_tag_names + is_priority_tag_name ──────────────

    #[test]
    fn projected_tag_names_appends_priority() {
        let p = parse_with_today("Pay rent #urgent !1", t());
        assert_eq!(
            p.projected_tag_names(),
            vec!["urgent".to_string(), "priority-1".to_string()]
        );
    }

    #[test]
    fn projected_tag_names_no_double_priority() {
        // If the user types `#priority-1 !1`, the projection
        // shouldn't push a duplicate. Case-insensitive dedup.
        let p = parse_with_today("Task #PRIORITY-1 !1", t());
        let projected = p.projected_tag_names();
        let count = projected
            .iter()
            .filter(|t| t.eq_ignore_ascii_case("priority-1"))
            .count();
        assert_eq!(count, 1, "got {projected:?}");
    }

    #[test]
    fn projected_tag_names_no_priority_means_unchanged() {
        let p = parse_with_today("Buy milk #errand", t());
        assert_eq!(p.projected_tag_names(), vec!["errand"]);
    }

    #[test]
    fn is_priority_tag_name_matches_levels() {
        assert!(is_priority_tag_name("priority-1"));
        assert!(is_priority_tag_name("priority-2"));
        assert!(is_priority_tag_name("priority-3"));
    }

    #[test]
    fn is_priority_tag_name_rejects_other_shapes() {
        assert!(!is_priority_tag_name("priority"));
        assert!(!is_priority_tag_name("priority-"));
        assert!(!is_priority_tag_name("priority-0"));
        assert!(!is_priority_tag_name("priority-4"));
        assert!(!is_priority_tag_name("priority-9"));
        assert!(!is_priority_tag_name("priority-12"));
        assert!(!is_priority_tag_name("priority-high"));
        assert!(!is_priority_tag_name("urgent"));
        assert!(!is_priority_tag_name(""));
    }
}
