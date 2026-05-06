// SPDX-License-Identifier: MIT
//! Inline-syntax parser for the bottom-of-list entry (Phase 6b) and
//! the Quick Entry modal (Phase 6c).
//!
//! Supported syntax (per spec.md §6):
//!
//! - `#errand` — attach the tag named `errand` (case-insensitive,
//!   created on first use by the calling code).
//! - `@today` / `@tomorrow` / `@someday` — set `scheduled_for`.
//! - `@yyyy-mm-dd` — set `scheduled_for` to a specific date.
//! - `@deadline yyyy-mm-dd` — set `deadline`.
//!
//! Anything else is title text. Unrecognised `@foo` strings stay in
//! the title verbatim — no silent data loss.

use atrium_core::ScheduledFor;
use chrono::{Local, NaiveDate};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ParsedEntry {
    pub title: String,
    pub tag_names: Vec<String>,
    pub scheduled_for: Option<ScheduledFor>,
    pub deadline: Option<NaiveDate>,
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
        } else if let Some(date_str) = word.strip_prefix('@') {
            if let Ok(d) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                scheduled_for = Some(ScheduledFor::Date(d));
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
    }
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
}
