// SPDX-License-Identifier: MIT
//! Natural-language recurrence parser for Todoist's `DATE` column.
//!
//! Handles every phrasing in `home.csv` per the roadmap §18
//! mapping table, plus a few sensible extensions that fall out of
//! the same shape (year intervals, ISO dates, "tomorrow at X").
//! Output is an RFC 5545 RRULE string + the anchor date the
//! caller stamps on `scheduled_for`. Time-of-day is parsed for
//! the lossy report — Atrium's domain stores date-only schedules
//! per spec §4.1, so the recovered time is informational.
//!
//! Hand-rolled matching, no regex dep. The Todoist phrasings are
//! a small constellation; pattern-matching by tokenised words is
//! readable and maintainable.

use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Weekday};

/// Parsed result from a Todoist DATE string. The RRULE side of
/// this is the canonical form Atrium stores in
/// `task.repeat_rule`; the anchor goes into `scheduled_for`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecurrenceParse {
    /// RFC 5545 RRULE body (no leading `RRULE:` / `DTSTART:`).
    /// `None` for past-dated single occurrences with no rule.
    pub rrule: Option<String>,
    /// Anchor date for `scheduled_for`. Recurring rules need an
    /// anchor; we resolve it relative to `today` (e.g.
    /// "Every Sunday" → next Sunday on or after today).
    pub scheduled_date: NaiveDate,
    /// Time-of-day if the source phrase carries one
    /// ("at 10am" / "at 15:00"). Atrium drops time per spec §4.1
    /// but the caller preserves it in the lossy report.
    pub time: Option<NaiveTime>,
}

/// Parse a Todoist DATE phrase. Returns `None` for unrecognised
/// input — the caller adds the raw string to the post-import
/// lossy-fields report so the user knows what didn't translate.
pub fn parse_recurrence(input: &str, today: NaiveDate) -> Option<RecurrenceParse> {
    let normalised = input.trim().to_ascii_lowercase();
    if normalised.is_empty() {
        return None;
    }
    try_every_weekday(&normalised, today)
        .or_else(|| try_every_n_unit(&normalised, today))
        .or_else(|| try_every_unit(&normalised, today))
        .or_else(|| try_every_ordinal_day(&normalised, today))
        .or_else(|| try_n_units_ago(&normalised, today))
        .or_else(|| try_relative_keyword(&normalised, today))
        .or_else(|| try_iso_date(&normalised))
}

// ── Pattern 1: every <weekday> at <time> ─────────────────────

fn try_every_weekday(input: &str, today: NaiveDate) -> Option<RecurrenceParse> {
    let rest = input.strip_prefix("every ")?;
    let mut tokens = rest.split_whitespace();
    let weekday_word = tokens.next()?;
    let weekday = parse_weekday(weekday_word)?;
    // Optional "at <time>" tail.
    let time = parse_at_time_tail(tokens.collect::<Vec<_>>().join(" ").as_str());
    let scheduled_date = next_occurrence_of_weekday(today, weekday);
    let byday = weekday_to_byday_token(weekday);
    Some(RecurrenceParse {
        rrule: Some(format!("FREQ=WEEKLY;BYDAY={byday}")),
        scheduled_date,
        time,
    })
}

// ── Pattern 2: every <N> <unit>(s)? [at <time>] ──────────────

fn try_every_n_unit(input: &str, today: NaiveDate) -> Option<RecurrenceParse> {
    let rest = input.strip_prefix("every ")?;
    let mut tokens = rest.split_whitespace();
    let n_token = tokens.next()?;
    let n: u32 = n_token.parse().ok()?;
    if n == 0 {
        return None;
    }
    let unit_token = tokens.next()?;
    let freq = unit_to_freq(unit_token)?;
    let time = parse_at_time_tail(tokens.collect::<Vec<_>>().join(" ").as_str());
    let rrule = if n == 1 {
        format!("FREQ={freq}")
    } else {
        format!("FREQ={freq};INTERVAL={n}")
    };
    Some(RecurrenceParse {
        rrule: Some(rrule),
        scheduled_date: today,
        time,
    })
}

// ── Pattern 3: every <unit> [at <time>] ──────────────────────
//   "every day at 9pm" / "every month" / "every week"

fn try_every_unit(input: &str, today: NaiveDate) -> Option<RecurrenceParse> {
    let rest = input.strip_prefix("every ")?;
    let mut tokens = rest.split_whitespace();
    let unit_token = tokens.next()?;
    let freq = unit_to_freq(unit_token)?;
    let time = parse_at_time_tail(tokens.collect::<Vec<_>>().join(" ").as_str());
    Some(RecurrenceParse {
        rrule: Some(format!("FREQ={freq}")),
        scheduled_date: today,
        time,
    })
}

// ── Pattern 4: every <ordinal>day / every <ordinal> day ──────
//   "every 1st day" / "Every 1stday" → FREQ=MONTHLY;BYMONTHDAY=N

fn try_every_ordinal_day(input: &str, today: NaiveDate) -> Option<RecurrenceParse> {
    let rest = input.strip_prefix("every ")?;
    // Two shapes: "1st day" (with space) and "1stday" (no space).
    let mut tokens = rest.split_whitespace();
    let first = tokens.next()?;
    let day_of_month = if let Some(rest) = first.strip_suffix("day") {
        // "1stday" — the day suffix is glued.
        parse_ordinal(rest)?
    } else if let Some(second) = tokens.next() {
        // "1st day" — second token must be "day" (or "days").
        if !matches!(second, "day" | "days") {
            return None;
        }
        parse_ordinal(first)?
    } else {
        return None;
    };
    if !(1..=31).contains(&day_of_month) {
        return None;
    }
    Some(RecurrenceParse {
        rrule: Some(format!("FREQ=MONTHLY;BYMONTHDAY={day_of_month}")),
        scheduled_date: next_occurrence_of_day_of_month(today, day_of_month),
        time: None,
    })
}

// ── Pattern 5: <N> day(s)/week(s)/etc. ago [at <time>] ──────
//   "3 days ago at 15:00" — past-dated single, no rule.

fn try_n_units_ago(input: &str, today: NaiveDate) -> Option<RecurrenceParse> {
    let mut tokens = input.split_whitespace();
    let n_token = tokens.next()?;
    let n: i64 = n_token.parse().ok()?;
    let unit_token = tokens.next()?;
    let scale = unit_to_days(unit_token)?;
    let ago_token = tokens.next()?;
    if ago_token != "ago" {
        return None;
    }
    let time = parse_at_time_tail(tokens.collect::<Vec<_>>().join(" ").as_str());
    Some(RecurrenceParse {
        rrule: None,
        scheduled_date: today - Duration::days(n * scale),
        time,
    })
}

// ── Pattern 6: today / tomorrow / yesterday [at <time>] ──────

fn try_relative_keyword(input: &str, today: NaiveDate) -> Option<RecurrenceParse> {
    let mut tokens = input.split_whitespace();
    let kw = tokens.next()?;
    let offset = match kw {
        "today" => 0,
        "tomorrow" => 1,
        "yesterday" => -1,
        _ => return None,
    };
    let time = parse_at_time_tail(tokens.collect::<Vec<_>>().join(" ").as_str());
    Some(RecurrenceParse {
        rrule: None,
        scheduled_date: today + Duration::days(offset),
        time,
    })
}

// ── Pattern 7: ISO date YYYY-MM-DD ───────────────────────────

fn try_iso_date(input: &str) -> Option<RecurrenceParse> {
    let date = NaiveDate::parse_from_str(input, "%Y-%m-%d").ok()?;
    Some(RecurrenceParse {
        rrule: None,
        scheduled_date: date,
        time: None,
    })
}

// ── Helpers ──────────────────────────────────────────────────

fn parse_weekday(word: &str) -> Option<Weekday> {
    match word {
        "monday" => Some(Weekday::Mon),
        "tuesday" => Some(Weekday::Tue),
        "wednesday" => Some(Weekday::Wed),
        "thursday" => Some(Weekday::Thu),
        "friday" => Some(Weekday::Fri),
        "saturday" => Some(Weekday::Sat),
        "sunday" => Some(Weekday::Sun),
        _ => None,
    }
}

fn weekday_to_byday_token(w: Weekday) -> &'static str {
    match w {
        Weekday::Mon => "MO",
        Weekday::Tue => "TU",
        Weekday::Wed => "WE",
        Weekday::Thu => "TH",
        Weekday::Fri => "FR",
        Weekday::Sat => "SA",
        Weekday::Sun => "SU",
    }
}

fn next_occurrence_of_weekday(today: NaiveDate, target: Weekday) -> NaiveDate {
    let today_off = today.weekday().num_days_from_monday() as i64;
    let target_off = target.num_days_from_monday() as i64;
    let mut delta = target_off - today_off;
    if delta < 0 {
        delta += 7;
    }
    today + Duration::days(delta)
}

fn next_occurrence_of_day_of_month(today: NaiveDate, day: u32) -> NaiveDate {
    if today.day() <= day
        && let Some(d) = NaiveDate::from_ymd_opt(today.year(), today.month(), day)
    {
        return d;
    }
    // Move to the next month; clamp if the day doesn't exist in
    // that month (Feb 30 → Mar 1 by way of last-of-month + day).
    let (next_y, next_m) = if today.month() == 12 {
        (today.year() + 1, 1)
    } else {
        (today.year(), today.month() + 1)
    };
    NaiveDate::from_ymd_opt(next_y, next_m, day).unwrap_or_else(|| {
        // The target day doesn't exist in that month (e.g. day 31
        // and next month is February). Walk forward until we
        // find one that does — at most 12 months in the worst
        // case.
        let mut y = next_y;
        let mut m = next_m;
        for _ in 0..12 {
            if let Some(d) = NaiveDate::from_ymd_opt(y, m, day) {
                return d;
            }
            if m == 12 {
                m = 1;
                y += 1;
            } else {
                m += 1;
            }
        }
        // Genuinely unreachable for sane day values — bail with
        // today as a defensive fallback.
        today
    })
}

/// Convert a unit token to an RRULE FREQ value.
/// Accepts the singular/plural flavours and the `every 3 day`
/// typo from the fixture.
fn unit_to_freq(token: &str) -> Option<&'static str> {
    match token {
        "day" | "days" => Some("DAILY"),
        "week" | "weeks" => Some("WEEKLY"),
        "month" | "months" => Some("MONTHLY"),
        "year" | "years" => Some("YEARLY"),
        _ => None,
    }
}

/// Convert a unit token to days for `<N> days ago`-style
/// arithmetic. Months / years are coarser; for "3 months ago"
/// we interpret as 90-day approximation. The Todoist fixture
/// only uses `days ago`; the rest is forward-compatible.
fn unit_to_days(token: &str) -> Option<i64> {
    match token {
        "day" | "days" => Some(1),
        "week" | "weeks" => Some(7),
        "month" | "months" => Some(30),
        "year" | "years" => Some(365),
        _ => None,
    }
}

/// Parse a `1st`, `2nd`, `3rd`, `4th`, ... ordinal prefix into
/// its numeric value. Only the first 31 days matter for
/// BYMONTHDAY.
fn parse_ordinal(token: &str) -> Option<u32> {
    // Strip the trailing two letters (st/nd/rd/th) if present;
    // otherwise treat the whole thing as a number (rare, but
    // tolerant).
    let stripped = if token.len() > 2 {
        let (head, tail) = token.split_at(token.len() - 2);
        if matches!(tail, "st" | "nd" | "rd" | "th") {
            head
        } else {
            token
        }
    } else {
        token
    };
    stripped.parse().ok()
}

/// Pull a `[at <time>]` tail out of the remaining tokens.
/// Recognises `<H>am` / `<H>pm` / `<H>:<M>` / `<H>:<M>am` /
/// `<H>:<M>pm`. Returns None for any input that doesn't start
/// with `at`.
fn parse_at_time_tail(rest: &str) -> Option<NaiveTime> {
    let trimmed = rest.trim();
    let after_at = trimmed.strip_prefix("at ")?.trim();
    parse_time(after_at)
}

fn parse_time(token: &str) -> Option<NaiveTime> {
    let token = token.trim();
    // <HH>:<MM>[am|pm]
    if let Some((h, mm_with_suffix)) = token.split_once(':') {
        let hour: u32 = h.parse().ok()?;
        let (min_str, suffix) = split_am_pm(mm_with_suffix);
        let minute: u32 = min_str.parse().ok()?;
        let final_hour = match suffix {
            Some("am") => {
                if hour == 12 {
                    0
                } else {
                    hour
                }
            }
            Some("pm") => {
                if hour == 12 {
                    12
                } else {
                    hour + 12
                }
            }
            _ => hour,
        };
        return NaiveTime::from_hms_opt(final_hour, minute, 0);
    }
    // <H>am / <H>pm
    let (h_str, suffix) = split_am_pm(token);
    let hour: u32 = h_str.parse().ok()?;
    let final_hour = match suffix {
        Some("am") => {
            if hour == 12 {
                0
            } else {
                hour
            }
        }
        Some("pm") => {
            if hour == 12 {
                12
            } else {
                hour + 12
            }
        }
        _ => return None, // bare hour with no am/pm is ambiguous
    };
    NaiveTime::from_hms_opt(final_hour, 0, 0)
}

fn split_am_pm(token: &str) -> (&str, Option<&str>) {
    if let Some(stripped) = token.strip_suffix("am") {
        (stripped, Some("am"))
    } else if let Some(stripped) = token.strip_suffix("pm") {
        (stripped, Some("pm"))
    } else {
        (token, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    /// Friday 2026-05-15 — same anchor the Agenda tests use, so
    /// "next Sunday" lands on May 17 + "next Monday" lands on
    /// May 18 etc.
    fn today() -> NaiveDate {
        d(2026, 5, 15)
    }

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    // ── Patterns from the home.csv fixture ──────────────────

    #[test]
    fn every_sunday_at_10am() {
        let r = parse_recurrence("Every Sunday at 10am", today()).unwrap();
        assert_eq!(r.rrule.as_deref(), Some("FREQ=WEEKLY;BYDAY=SU"));
        assert_eq!(r.scheduled_date, d(2026, 5, 17)); // next Sunday
        assert_eq!(r.time, Some(t(10, 0)));
    }

    #[test]
    fn every_monday_at_8pm() {
        let r = parse_recurrence("Every Monday at 8pm", today()).unwrap();
        assert_eq!(r.rrule.as_deref(), Some("FREQ=WEEKLY;BYDAY=MO"));
        assert_eq!(r.scheduled_date, d(2026, 5, 18)); // next Monday
        assert_eq!(r.time, Some(t(20, 0)));
    }

    #[test]
    fn every_wednesday_at_3pm_lowercase() {
        // The fixture has "every wednesday at 3pm" all lowercase
        // — the parser must handle case-insensitively.
        let r = parse_recurrence("every wednesday at 3pm", today()).unwrap();
        assert_eq!(r.rrule.as_deref(), Some("FREQ=WEEKLY;BYDAY=WE"));
        assert_eq!(r.time, Some(t(15, 0)));
    }

    #[test]
    fn every_3_day_at_9am_with_typo() {
        // The fixture has "every 3 day at 9am" — note "day" is
        // singular. Parser tolerates both "day" and "days".
        let r = parse_recurrence("every 3 day at 9am", today()).unwrap();
        assert_eq!(r.rrule.as_deref(), Some("FREQ=DAILY;INTERVAL=3"));
        assert_eq!(r.time, Some(t(9, 0)));
    }

    #[test]
    fn every_3_month_singular() {
        let r = parse_recurrence("every 3 month", today()).unwrap();
        assert_eq!(r.rrule.as_deref(), Some("FREQ=MONTHLY;INTERVAL=3"));
        assert_eq!(r.time, None);
    }

    #[test]
    fn every_3_months_plural() {
        let r = parse_recurrence("every 3 months", today()).unwrap();
        assert_eq!(r.rrule.as_deref(), Some("FREQ=MONTHLY;INTERVAL=3"));
    }

    #[test]
    fn every_3_weeks() {
        let r = parse_recurrence("every 3 weeks", today()).unwrap();
        assert_eq!(r.rrule.as_deref(), Some("FREQ=WEEKLY;INTERVAL=3"));
    }

    #[test]
    fn every_month() {
        let r = parse_recurrence("every month", today()).unwrap();
        assert_eq!(r.rrule.as_deref(), Some("FREQ=MONTHLY"));
    }

    #[test]
    fn every_day_at_9pm() {
        let r = parse_recurrence("Every day at 9pm", today()).unwrap();
        assert_eq!(r.rrule.as_deref(), Some("FREQ=DAILY"));
        assert_eq!(r.time, Some(t(21, 0)));
    }

    #[test]
    fn every_day_at_8am() {
        let r = parse_recurrence("Every day at 8am", today()).unwrap();
        assert_eq!(r.rrule.as_deref(), Some("FREQ=DAILY"));
        assert_eq!(r.time, Some(t(8, 0)));
    }

    #[test]
    fn every_1st_day_with_space() {
        let r = parse_recurrence("every 1st day", today()).unwrap();
        assert_eq!(r.rrule.as_deref(), Some("FREQ=MONTHLY;BYMONTHDAY=1"));
    }

    #[test]
    fn every_1stday_no_space() {
        // The fixture has "Every 1stday" (no space).
        let r = parse_recurrence("Every 1stday", today()).unwrap();
        assert_eq!(r.rrule.as_deref(), Some("FREQ=MONTHLY;BYMONTHDAY=1"));
    }

    // ── Past-dated, single-occurrence ───────────────────────

    #[test]
    fn three_days_ago_at_15_00() {
        // Per roadmap §18: "3 days ago at 15:00" → past single.
        let r = parse_recurrence("3 days ago at 15:00", today()).unwrap();
        assert_eq!(r.rrule, None);
        assert_eq!(r.scheduled_date, d(2026, 5, 12));
        assert_eq!(r.time, Some(t(15, 0)));
    }

    #[test]
    fn relative_today_keyword() {
        let r = parse_recurrence("today at 10am", today()).unwrap();
        assert_eq!(r.rrule, None);
        assert_eq!(r.scheduled_date, today());
        assert_eq!(r.time, Some(t(10, 0)));
    }

    #[test]
    fn relative_tomorrow_keyword() {
        let r = parse_recurrence("tomorrow", today()).unwrap();
        assert_eq!(r.rrule, None);
        assert_eq!(r.scheduled_date, d(2026, 5, 16));
        assert_eq!(r.time, None);
    }

    #[test]
    fn iso_date_parses_as_single_anchor() {
        let r = parse_recurrence("2026-06-15", today()).unwrap();
        assert_eq!(r.rrule, None);
        assert_eq!(r.scheduled_date, d(2026, 6, 15));
    }

    // ── Negative cases ──────────────────────────────────────

    #[test]
    fn empty_returns_none() {
        assert_eq!(parse_recurrence("", today()), None);
        assert_eq!(parse_recurrence("   ", today()), None);
    }

    #[test]
    fn unrecognised_phrasing_returns_none() {
        // Caller surfaces this in the lossy-fields report.
        assert_eq!(parse_recurrence("when the moon is blue", today()), None);
    }

    #[test]
    fn every_zero_unit_rejected() {
        assert_eq!(parse_recurrence("every 0 days", today()), None);
    }

    // ── Time parsing edge cases ─────────────────────────────

    #[test]
    fn time_12am_means_midnight() {
        let r = parse_recurrence("Every day at 12am", today()).unwrap();
        assert_eq!(r.time, Some(t(0, 0)));
    }

    #[test]
    fn time_12pm_means_noon() {
        let r = parse_recurrence("Every day at 12pm", today()).unwrap();
        assert_eq!(r.time, Some(t(12, 0)));
    }

    #[test]
    fn time_with_minutes() {
        let r = parse_recurrence("Every day at 9:30am", today()).unwrap();
        assert_eq!(r.time, Some(t(9, 30)));
    }

    #[test]
    fn time_24h_format() {
        let r = parse_recurrence("Every day at 15:00", today()).unwrap();
        assert_eq!(r.time, Some(t(15, 0)));
    }

    // ── Anchor resolution ───────────────────────────────────

    #[test]
    fn next_sunday_when_today_is_sunday() {
        let sunday = d(2026, 5, 17);
        let r = parse_recurrence("Every Sunday at 10am", sunday).unwrap();
        // When today IS the target weekday, "next Sunday" is
        // today (delta = 0). Matches Todoist's interpretation.
        assert_eq!(r.scheduled_date, sunday);
    }

    #[test]
    fn next_first_when_today_is_third() {
        // Today May 15 → next 1st is June 1.
        let r = parse_recurrence("Every 1stday", today()).unwrap();
        assert_eq!(r.scheduled_date, d(2026, 6, 1));
    }

    #[test]
    fn next_first_when_today_is_first() {
        // Today is the 1st → today wins.
        let first = d(2026, 5, 1);
        let r = parse_recurrence("Every 1stday", first).unwrap();
        assert_eq!(r.scheduled_date, first);
    }
}
