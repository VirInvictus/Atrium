// SPDX-License-Identifier: MIT
//! RFC 5545 RRULE handling — Phase 15.
//!
//! Wraps the [`rrule`] crate with the date-only, mode-aware shape
//! Atrium tasks need. Two persisted fields back the wrapper:
//!
//! - `task.repeat_rule` — RFC 5545 RRULE text (e.g. `FREQ=WEEKLY`).
//! - `task.repeat_mode` — Org-style cookie controlling how the next
//!   anchor is chosen: `BASIC` / `CUMULATIVE` / `NEXT`. `None` falls
//!   back to the default ([`RepeatMode::default`], CUMULATIVE).
//!
//! The wrapper exposes one primary primitive: [`RepeatRule::next_after`].
//! It takes the previous anchor (the date the just-completed task
//! occupied), the completion date, and returns the date the next
//! instance should land on. The worker uses this in
//! `regenerate_on_complete` to spawn the follow-up task with the
//! shifted dates.
//!
//! Atrium tasks are date-only — `scheduled_for`, `deadline`, and
//! `defer_until` are all `NaiveDate`. The [`rrule`] crate works in
//! `DateTime<rrule::Tz>` and its parser only accepts IANA timezone
//! names (`Europe/London`, etc.), not `Local`. We sidestep that by
//! anchoring on midnight UTC: date arithmetic at midnight UTC is
//! DST-immune (it can't be ambiguous or skipped) and rounds back to
//! the correct calendar date for any reasonable user timezone. The
//! cost is that "midnight" in the user's local clock might be a
//! different UTC date — that only matters if we later care about
//! the time-of-day component of the recurrence, which Atrium does
//! not.

use chrono::{NaiveDate, TimeZone};
use rrule::{RRuleError, RRuleSet, Tz};
use std::str::FromStr;

/// Org-style repeater semantics. Determines how the worker picks the
/// next anchor when a repeating task is completed.
///
/// Maps directly to the three Org-mode SCHEDULED/DEADLINE cookies:
/// `+1w`, `++1w`, `.+1w`. The Org round-trip layer (Phase 17) reuses
/// the same enum.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    /// `+1w` — always shift by one rule increment from the previous
    /// anchor, even if the result is in the past. Rare in practice;
    /// included for round-trip fidelity with Org files.
    Basic,
    /// `++1w` — shift repeatedly until the next occurrence is in the
    /// future. The default; matches OmniFocus's behavior of "spawn
    /// the next instance after now" and is the right shape for most
    /// recurring chores.
    #[default]
    Cumulative,
    /// `.+1w` — anchor on the completion date and shift by one rule
    /// increment from there. The previous schedule is ignored. Right
    /// for "every N after I last did this" (haircut, oil change).
    Next,
}

impl RepeatMode {
    /// Parse the persisted column value. `None` and unrecognised
    /// strings both fall back to the default ([`Self::Cumulative`]).
    pub fn from_column(value: Option<&str>) -> Self {
        match value.map(str::trim) {
            Some("BASIC") => Self::Basic,
            Some("NEXT") => Self::Next,
            Some("CUMULATIVE") => Self::Cumulative,
            _ => Self::default(),
        }
    }

    /// Persisted column value. The default mode round-trips to its
    /// canonical name rather than `NULL` so callers don't have to
    /// handle the implicit-default case.
    pub fn as_column(&self) -> &'static str {
        match self {
            Self::Basic => "BASIC",
            Self::Cumulative => "CUMULATIVE",
            Self::Next => "NEXT",
        }
    }

    /// Org-mode cookie prefix (`"+"`, `"++"`, `".+"`). Used by Phase
    /// 17's Org export to render the SCHEDULED/DEADLINE cookie.
    pub fn org_cookie(&self) -> &'static str {
        match self {
            Self::Basic => "+",
            Self::Cumulative => "++",
            Self::Next => ".+",
        }
    }
}

/// Result of [`RepeatRule::rule_with_count_decremented`]. The worker
/// matches on this to decide whether the just-completed task should
/// spawn a follow-up at all, and what its `repeat_rule` text should
/// be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CountStep {
    /// The rule has no `COUNT=` and runs forever (or terminates via
    /// `UNTIL=`, which is absolute and doesn't need decrementing).
    /// The follow-up keeps the same rule text.
    Unbounded,
    /// The rule's `COUNT=` was decremented; the follow-up's
    /// `repeat_rule` should be the carried string.
    Decremented(String),
    /// The rule's `COUNT=` was already 1 — the just-completed
    /// instance was the last in the series. The worker skips the
    /// spawn.
    Exhausted,
}

/// A parsed-and-mode-tagged repeat rule, ready to compute the next
/// occurrence. Built by [`RepeatRule::parse`].
#[derive(Debug, Clone)]
pub struct RepeatRule {
    /// The original RRULE text as stored on the task. We keep this
    /// alongside the parsed form so re-serialisation is byte-exact.
    pub rule: String,
    pub mode: RepeatMode,
}

impl RepeatRule {
    /// Validate an RRULE text + mode combo. The text must be the
    /// RRULE part only (no `DTSTART:` line) — we add a synthetic
    /// `DTSTART` at parse time using a placeholder anchor, since
    /// RRULE-by-itself isn't a complete RFC 5545 expression.
    ///
    /// Returns the parsed rule on success; on failure, returns the
    /// underlying `rrule::RRuleError` so callers can surface a
    /// useful diagnostic in the editor.
    pub fn parse(rule: &str, mode: RepeatMode) -> Result<Self, RRuleError> {
        // Validate against a synthetic Jan-1-2000 anchor. The anchor
        // doesn't matter for syntax validation — it's only used to
        // satisfy rrule's RRuleSet parser, which requires DTSTART.
        let _ = build_set_from_rule_text(rule, NaiveDate::from_ymd_opt(2000, 1, 1).unwrap())?;
        Ok(Self {
            rule: rule.to_string(),
            mode,
        })
    }

    /// Convenience constructor used by tests and call-sites that
    /// already know the rule is well-formed (e.g. round-trip from
    /// the database, where the worker validated on insert).
    pub fn new_unchecked(rule: impl Into<String>, mode: RepeatMode) -> Self {
        Self {
            rule: rule.into(),
            mode,
        }
    }

    /// Read the rule's `COUNT=N` token if present.
    ///
    /// Returns `None` when the rule has no `COUNT` (the rule runs
    /// forever, or terminates via `UNTIL=`). Used by the worker to
    /// decide whether the just-completed instance is the final one
    /// in a bounded series.
    pub fn count(&self) -> Option<u32> {
        for token in self.rule.split(';') {
            let trimmed = token.trim();
            if let Some(rest) = trimmed.strip_prefix("COUNT=") {
                return rest.trim().parse().ok();
            }
            if let Some(rest) = trimmed.strip_prefix("count=") {
                return rest.trim().parse().ok();
            }
        }
        None
    }

    /// Return a new rule string with `COUNT=N` decremented by one.
    ///
    /// Returns `None` when:
    ///
    /// - the rule has no `COUNT` (no decrement needed; spawn forever),
    /// - the existing `COUNT` is already 1 (the just-completed
    ///   instance was the final one — no spawn).
    ///
    /// Returns `Some(new_rule)` otherwise; substitution is textual
    /// to preserve the rest of the rule byte-for-byte.
    pub fn rule_with_count_decremented(&self) -> CountStep {
        let Some(n) = self.count() else {
            return CountStep::Unbounded;
        };
        if n <= 1 {
            return CountStep::Exhausted;
        }
        let new_count = n - 1;
        let mut parts: Vec<String> = Vec::new();
        for token in self.rule.split(';') {
            let trimmed = token.trim();
            if trimmed.to_ascii_uppercase().starts_with("COUNT=") {
                parts.push(format!("COUNT={new_count}"));
            } else if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
        }
        CountStep::Decremented(parts.join(";"))
    }

    /// Compute the next occurrence after a task is completed.
    ///
    /// `previous_anchor` is the date the just-completed task was
    /// scheduled for (or its deadline / defer date — whichever the
    /// caller picked as the rule's anchor). `completed_on` is the
    /// date of completion (almost always today, but tests want to
    /// pin it).
    ///
    /// Returns `None` if the rule has no further occurrences (e.g.
    /// `COUNT=N` exhausted, or `UNTIL` passed). Returns a date
    /// otherwise.
    ///
    /// **Mode semantics:**
    ///
    /// - [`RepeatMode::Basic`]: take the *next* occurrence after
    ///   `previous_anchor` regardless of completion date. May land
    ///   in the past.
    /// - [`RepeatMode::Cumulative`]: take the next occurrence that
    ///   is strictly after `completed_on`. Skips past any occurrences
    ///   that are already overdue.
    /// - [`RepeatMode::Next`]: anchor on `completed_on` and take the
    ///   next occurrence after that.
    pub fn next_after(
        &self,
        previous_anchor: NaiveDate,
        completed_on: NaiveDate,
    ) -> Option<NaiveDate> {
        match self.mode {
            RepeatMode::Basic => {
                let set = build_set_from_rule_text(&self.rule, previous_anchor).ok()?;
                let after = utc_midnight(previous_anchor);
                next_strictly_after(set, after)
            }
            RepeatMode::Cumulative => {
                let set = build_set_from_rule_text(&self.rule, previous_anchor).ok()?;
                let after = utc_midnight(completed_on);
                next_strictly_after(set, after)
            }
            RepeatMode::Next => {
                let set = build_set_from_rule_text(&self.rule, completed_on).ok()?;
                let after = utc_midnight(completed_on);
                next_strictly_after(set, after)
            }
        }
    }
}

/// Build an `RRuleSet` from a bare RRULE text string anchored at
/// midnight UTC on `anchor`. Used both for validation (parse
/// errors propagate up) and for iteration.
fn build_set_from_rule_text(rule: &str, anchor: NaiveDate) -> Result<RRuleSet, RRuleError> {
    let dt_start = utc_midnight(anchor);
    let prelude = format!(
        "DTSTART:{}Z\nRRULE:{}",
        dt_start.format("%Y%m%dT%H%M%S"),
        rule.trim()
    );
    RRuleSet::from_str(&prelude)
}

/// Convert a `NaiveDate` to midnight UTC in rrule's timezone wrapper.
/// Date-only tasks have no time component; UTC dodges DST entirely.
fn utc_midnight(date: NaiveDate) -> chrono::DateTime<Tz> {
    let naive = date.and_hms_opt(0, 0, 0).expect("midnight is always valid");
    chrono::Utc
        .from_utc_datetime(&naive)
        .with_timezone(&Tz::UTC)
}

/// Iterate `set` and return the first occurrence strictly after `after`.
fn next_strictly_after(set: RRuleSet, after: chrono::DateTime<Tz>) -> Option<NaiveDate> {
    // `RRuleSet::after(after)` filters the iterator inclusive — i.e.
    // returns occurrences ≥ after. We want strict-after for
    // CUMULATIVE / NEXT and BASIC's "next from anchor" both want
    // strict-after the previous anchor too. Add a one-second buffer
    // to dodge the inclusive boundary, then take the first match.
    //
    // Cap iteration at 366 occurrences so a malformed COUNT=∞ rule
    // (technically invalid but cheap to defend against) can't spin.
    let buffered = after + chrono::Duration::seconds(1);
    let result = set.after(buffered).all(366);
    result.dates.into_iter().next().map(|dt| dt.date_naive())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn parse_rejects_garbage() {
        assert!(RepeatRule::parse("not a rrule", RepeatMode::Cumulative).is_err());
        assert!(RepeatRule::parse("FREQ=NONSENSE", RepeatMode::Cumulative).is_err());
    }

    #[test]
    fn parse_accepts_simple_freqs() {
        for rule in [
            "FREQ=DAILY",
            "FREQ=WEEKLY",
            "FREQ=MONTHLY",
            "FREQ=YEARLY",
            "FREQ=WEEKLY;INTERVAL=2",
            "FREQ=MONTHLY;BYMONTHDAY=15",
        ] {
            assert!(
                RepeatRule::parse(rule, RepeatMode::Cumulative).is_ok(),
                "rule should parse: {rule}"
            );
        }
    }

    #[test]
    fn mode_round_trips_through_column() {
        for mode in [RepeatMode::Basic, RepeatMode::Cumulative, RepeatMode::Next] {
            assert_eq!(RepeatMode::from_column(Some(mode.as_column())), mode);
        }
    }

    #[test]
    fn unknown_mode_falls_back_to_default() {
        assert_eq!(RepeatMode::from_column(None), RepeatMode::Cumulative);
        assert_eq!(
            RepeatMode::from_column(Some("garbage")),
            RepeatMode::Cumulative
        );
        assert_eq!(RepeatMode::from_column(Some("")), RepeatMode::Cumulative);
    }

    #[test]
    fn cumulative_skips_to_next_future_occurrence() {
        // Weekly rule, anchored Mon 2026-01-05. Completed three weeks
        // late on Wed 2026-01-28. Cumulative should pick the next
        // Monday strictly after Jan 28 → 2026-02-02.
        let r = RepeatRule::parse("FREQ=WEEKLY", RepeatMode::Cumulative).unwrap();
        let next = r.next_after(d(2026, 1, 5), d(2026, 1, 28));
        assert_eq!(next, Some(d(2026, 2, 2)));
    }

    #[test]
    fn cumulative_skips_within_a_few_days() {
        // Same rule, completed mid-week before next Monday rolls over.
        // Weekly Mon, anchored 2026-01-05, completed Wed 2026-01-07.
        // Next Monday after Jan 7 → 2026-01-12.
        let r = RepeatRule::parse("FREQ=WEEKLY", RepeatMode::Cumulative).unwrap();
        let next = r.next_after(d(2026, 1, 5), d(2026, 1, 7));
        assert_eq!(next, Some(d(2026, 1, 12)));
    }

    #[test]
    fn basic_takes_strict_next_from_anchor() {
        // Weekly Mon, anchored 2026-01-05, completed three weeks late.
        // Basic should still hand back 2026-01-12 (one week from
        // anchor, regardless of completion date).
        let r = RepeatRule::parse("FREQ=WEEKLY", RepeatMode::Basic).unwrap();
        let next = r.next_after(d(2026, 1, 5), d(2026, 1, 28));
        assert_eq!(next, Some(d(2026, 1, 12)));
    }

    #[test]
    fn next_anchors_on_completion_date() {
        // Weekly rule, original anchor 2026-01-05, completed 2026-01-20.
        // NEXT should anchor on 2026-01-20 and pick a week later.
        let r = RepeatRule::parse("FREQ=WEEKLY", RepeatMode::Next).unwrap();
        let next = r.next_after(d(2026, 1, 5), d(2026, 1, 20));
        assert_eq!(next, Some(d(2026, 1, 27)));
    }

    #[test]
    fn daily_advances_one_day() {
        let r = RepeatRule::parse("FREQ=DAILY", RepeatMode::Cumulative).unwrap();
        let next = r.next_after(d(2026, 5, 1), d(2026, 5, 1));
        assert_eq!(next, Some(d(2026, 5, 2)));
    }

    #[test]
    fn monthly_advances_one_month() {
        let r = RepeatRule::parse("FREQ=MONTHLY", RepeatMode::Cumulative).unwrap();
        let next = r.next_after(d(2026, 1, 15), d(2026, 1, 15));
        assert_eq!(next, Some(d(2026, 2, 15)));
    }

    #[test]
    fn monthly_clamps_end_of_month() {
        // Rule anchored Jan 31; February has no 31st. RFC 5545 with
        // FREQ=MONTHLY skips months that don't have the BYMONTHDAY,
        // so the next occurrence after Jan 31 is Mar 31 (Feb gets
        // skipped). Atrium accepts that — it matches Org-mode's
        // semantics and is the least-surprising behavior compared
        // to clamping to Feb 28 silently.
        let r = RepeatRule::parse("FREQ=MONTHLY", RepeatMode::Cumulative).unwrap();
        let next = r.next_after(d(2026, 1, 31), d(2026, 1, 31));
        assert_eq!(next, Some(d(2026, 3, 31)));
    }

    #[test]
    fn yearly_advances_one_year() {
        let r = RepeatRule::parse("FREQ=YEARLY", RepeatMode::Cumulative).unwrap();
        let next = r.next_after(d(2026, 5, 1), d(2026, 5, 1));
        assert_eq!(next, Some(d(2027, 5, 1)));
    }

    #[test]
    fn count_terminates_after_n_occurrences() {
        // COUNT=3 means 3 total instances including the anchor.
        // After completing instances 1 and 2, the third is still
        // ahead; after instance 3 there's no fourth.
        let r = RepeatRule::parse("FREQ=DAILY;COUNT=3", RepeatMode::Cumulative).unwrap();
        let after_first = r.next_after(d(2026, 5, 1), d(2026, 5, 1));
        assert_eq!(after_first, Some(d(2026, 5, 2)));
        let after_third = r.next_after(d(2026, 5, 1), d(2026, 5, 3));
        assert_eq!(after_third, None);
    }

    #[test]
    fn org_cookie_round_trip() {
        assert_eq!(RepeatMode::Basic.org_cookie(), "+");
        assert_eq!(RepeatMode::Cumulative.org_cookie(), "++");
        assert_eq!(RepeatMode::Next.org_cookie(), ".+");
    }

    #[test]
    fn count_step_unbounded() {
        let r = RepeatRule::parse("FREQ=DAILY", RepeatMode::Cumulative).unwrap();
        assert!(matches!(
            r.rule_with_count_decremented(),
            CountStep::Unbounded
        ));
    }

    #[test]
    fn count_step_decrements_n_above_one() {
        let r = RepeatRule::parse("FREQ=DAILY;COUNT=5", RepeatMode::Cumulative).unwrap();
        match r.rule_with_count_decremented() {
            CountStep::Decremented(s) => assert!(s.contains("COUNT=4"), "got {s}"),
            other => panic!("expected Decremented, got {other:?}"),
        }
    }

    #[test]
    fn count_step_exhausted_at_one() {
        let r = RepeatRule::parse("FREQ=DAILY;COUNT=1", RepeatMode::Cumulative).unwrap();
        assert_eq!(r.rule_with_count_decremented(), CountStep::Exhausted);
    }

    #[test]
    fn count_step_preserves_other_tokens() {
        let r = RepeatRule::parse(
            "FREQ=WEEKLY;INTERVAL=2;COUNT=10;BYDAY=MO,WE",
            RepeatMode::Cumulative,
        )
        .unwrap();
        match r.rule_with_count_decremented() {
            CountStep::Decremented(s) => {
                assert!(s.contains("FREQ=WEEKLY"));
                assert!(s.contains("INTERVAL=2"));
                assert!(s.contains("BYDAY=MO,WE"));
                assert!(s.contains("COUNT=9"));
            }
            other => panic!("expected Decremented, got {other:?}"),
        }
    }
}
