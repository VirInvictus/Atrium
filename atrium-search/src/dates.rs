// SPDX-License-Identifier: MIT
//! Calibre-style date keywords resolved to concrete date ranges.
//!
//! Lifted out of `eval.rs` at v0.5.3 so the SQL translator can
//! produce the same date arithmetic without duplicating the rules.
//! The contract is: every keyword resolves to a half-closed `[low,
//! high]` (both inclusive) `NaiveDate` pair, and `Eq` means "the
//! task's date falls inside that pair." Single-day keywords use
//! `low == high`.

use chrono::{Datelike, Duration, NaiveDate};

use crate::ast::{Comparator, DateKeyword, Value};

/// Resolve a `Value` to a `[low, high]` pair against `today`. Used
/// by both the in-memory evaluator and the SQL translator. Numeric /
/// text values that aren't dates collapse to `(today, today)` so the
/// caller deterministically returns no matches; the parser already
/// rejected most of these shapes upstream.
pub fn value_to_range(value: &Value, today: NaiveDate) -> (NaiveDate, NaiveDate) {
    match value {
        Value::Date(d) => (*d, *d),
        Value::DateKeyword(k) => keyword_to_range(*k, today),
        _ => (today, today),
    }
}

/// Resolve a date keyword. `today`, `yesterday`, `tomorrow`,
/// `Ndaysago`, `Ndaysout` are single-day; `thisweek`, `thismonth`,
/// `thisyear` and their last/next siblings span ranges.
pub fn keyword_to_range(k: DateKeyword, today: NaiveDate) -> (NaiveDate, NaiveDate) {
    match k {
        DateKeyword::Today => (today, today),
        DateKeyword::Yesterday => {
            let d = today - Duration::days(1);
            (d, d)
        }
        DateKeyword::Tomorrow => {
            let d = today + Duration::days(1);
            (d, d)
        }
        DateKeyword::ThisWeek => week_bounds(today, 0),
        DateKeyword::LastWeek => week_bounds(today, -1),
        DateKeyword::NextWeek => week_bounds(today, 1),
        DateKeyword::ThisMonth => month_bounds(today, 0),
        DateKeyword::LastMonth => month_bounds(today, -1),
        DateKeyword::NextMonth => month_bounds(today, 1),
        DateKeyword::ThisYear => {
            let lo = NaiveDate::from_ymd_opt(today.year(), 1, 1).unwrap_or(today);
            let hi = NaiveDate::from_ymd_opt(today.year(), 12, 31).unwrap_or(today);
            (lo, hi)
        }
        DateKeyword::DaysAgo(n) => {
            let d = today - Duration::days(n as i64);
            (d, d)
        }
        DateKeyword::DaysOut(n) => {
            let d = today + Duration::days(n as i64);
            (d, d)
        }
    }
}

/// ISO Mon-start week. `offset_weeks` shifts to last/next/etc.
pub fn week_bounds(today: NaiveDate, offset_weeks: i32) -> (NaiveDate, NaiveDate) {
    let weekday = today.weekday().num_days_from_monday() as i64;
    let monday = today - Duration::days(weekday) + Duration::weeks(offset_weeks as i64);
    let sunday = monday + Duration::days(6);
    (monday, sunday)
}

pub fn month_bounds(today: NaiveDate, offset_months: i32) -> (NaiveDate, NaiveDate) {
    let mut y = today.year();
    let mut m = today.month() as i32 + offset_months;
    while m < 1 {
        m += 12;
        y -= 1;
    }
    while m > 12 {
        m -= 12;
        y += 1;
    }
    let m = m as u32;
    let lo = NaiveDate::from_ymd_opt(y, m, 1).unwrap_or(today);
    let hi_month = if m == 12 { 1 } else { m + 1 };
    let hi_year = if m == 12 { y + 1 } else { y };
    let hi = NaiveDate::from_ymd_opt(hi_year, hi_month, 1)
        .map(|d| d - Duration::days(1))
        .unwrap_or(today);
    (lo, hi)
}

/// Apply a comparator against a date and a [low, high] window.
/// Mirrors the in-memory evaluator's semantics so both paths stay
/// in lockstep — `Eq` against a range means "in the range" rather
/// than "equal to a specific day."
pub fn compare_date(d: NaiveDate, lo: NaiveDate, hi: NaiveDate, comp: Comparator) -> bool {
    match comp {
        Comparator::Eq => d >= lo && d <= hi,
        Comparator::Ne => d < lo || d > hi,
        Comparator::Lt => d < lo,
        Comparator::Le => d <= hi,
        Comparator::Gt => d > hi,
        Comparator::Ge => d >= lo,
    }
}
