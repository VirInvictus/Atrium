// SPDX-License-Identifier: MIT
//! VCALENDAR / VTODO emitter. Stdlib-only.
//!
//! Atrium's VTODO export is a one-way file dump (spec §7.2 —
//! not a CalDAV client). One `.ics` file with a single
//! VCALENDAR component containing one VTODO per task. All
//! timestamps render as UTC; receiving CalDAV apps universally
//! accept UTC, so omitting VTIMEZONE is safe.
//!
//! # Round-trip contract
//!
//! - SUMMARY / DESCRIPTION / LOCATION emit as escaped TEXT
//!   (newlines → `\n`, commas → `\,`, semicolons → `\;`,
//!   backslashes → `\\`) per RFC 5545 §3.3.11.
//! - Lines > 75 octets fold per §3.1 (CRLF + space continuation).
//! - DTSTART / DUE render as date-only (`VALUE=DATE:YYYYMMDD`)
//!   when the source was date-only, or `YYYYMMDDTHHMMSSZ` when
//!   the source had a time-of-day.
//! - COMPLETED always renders as UTC datetime.
//! - PRIORITY emits as-is (Atrium stores 1–9 / NULL; NULL skips
//!   the line entirely).
//! - CATEGORIES emits as a single comma-joined value with
//!   per-name escape encoding.
//! - X-* properties (and any other stashed `extra_properties`)
//!   emit in BTreeMap sort order after the modeled lines.

use std::fmt::Write;

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};

/// Top-level emit configuration. PRODID identifies the source
/// app per RFC 5545 §3.7.3. Atrium's product token tracks the
/// crate version so a re-import can correlate file-source.
pub struct EmitConfig {
    pub prodid: String,
}

impl Default for EmitConfig {
    fn default() -> Self {
        Self {
            prodid: format!("-//Atrium//atrium-cli {}//EN", env!("CARGO_PKG_VERSION")),
        }
    }
}

/// One VTODO's emission shape. The mapper builds these from
/// `task` + `task_tag` + `extra_properties` rows; the emitter
/// renders them. Optional fields skip their property line when
/// `None`.
#[derive(Debug, Clone, Default)]
pub struct VtodoOutput {
    /// REQUIRED — the round-trip anchor. Either the original
    /// `extra_properties["VTODO_UID"]` or the task's
    /// UUID-v4 surface form.
    pub uid: String,
    /// REQUIRED — DTSTAMP per RFC 5545 §3.8.7.2.
    pub dtstamp: DateTime<Utc>,
    pub summary: Option<String>,
    pub description: Option<String>,
    /// Date-only or date + time-of-day. Renders with the
    /// matching VALUE parameter on the date-only path.
    pub dtstart: Option<DateOrDateTime>,
    pub due: Option<DateOrDateTime>,
    pub completed: Option<DateTime<Utc>>,
    /// `NEEDS-ACTION` / `IN-PROCESS` / `COMPLETED` / `CANCELLED`.
    pub status: Option<String>,
    /// 1–9 (Atrium maps from `priority-N` tags). 0 / None
    /// skips the line.
    pub priority: Option<u8>,
    pub categories: Vec<String>,
    pub rrule: Option<String>,
    pub location: Option<String>,
    /// X-* and any other unmodeled keys preserved from the
    /// source side via `task.extra_properties`. Emitted after
    /// the modeled lines, in source order.
    pub extra_properties: Vec<(String, String)>,
}

/// Date-only or UTC datetime — the input side of DTSTART /
/// DUE. Matches the parser's [`crate::vtodo::parser::DateOrDateTime`]
/// shape but is duplicated here so the emit module is
/// self-contained (no parser dependency for export-only
/// callers).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateOrDateTime {
    Date(NaiveDate),
    DateTime(DateTime<Utc>),
}

impl DateOrDateTime {
    pub fn from_date(date: NaiveDate) -> Self {
        Self::Date(date)
    }

    pub fn from_date_time(date: NaiveDate, time: NaiveTime) -> Self {
        let dt = date.and_time(time).and_utc();
        Self::DateTime(dt)
    }
}

/// Render `components` as a complete VCALENDAR text. The
/// caller is responsible for atomic-writing it to disk via
/// `atrium_core::sync::atomic::write_atomic`.
pub fn emit_vcalendar(components: &[VtodoOutput], config: &EmitConfig) -> String {
    let mut out = String::new();
    push_line(&mut out, "BEGIN:VCALENDAR");
    push_line(&mut out, "VERSION:2.0");
    push_line(&mut out, &format!("PRODID:{}", config.prodid));

    for vtodo in components {
        push_line(&mut out, "BEGIN:VTODO");
        push_line(&mut out, &format!("UID:{}", vtodo.uid));
        push_line(
            &mut out,
            &format!("DTSTAMP:{}", format_utc_datetime(vtodo.dtstamp)),
        );
        if let Some(s) = &vtodo.summary {
            push_line(&mut out, &format!("SUMMARY:{}", encode_text(s)));
        }
        if let Some(s) = &vtodo.description {
            push_line(&mut out, &format!("DESCRIPTION:{}", encode_text(s)));
        }
        if let Some(d) = vtodo.dtstart {
            push_line(&mut out, &format_date_or_datetime("DTSTART", d));
        }
        if let Some(d) = vtodo.due {
            push_line(&mut out, &format_date_or_datetime("DUE", d));
        }
        if let Some(c) = vtodo.completed {
            push_line(&mut out, &format!("COMPLETED:{}", format_utc_datetime(c)));
        }
        if let Some(s) = &vtodo.status {
            push_line(&mut out, &format!("STATUS:{s}"));
        }
        if let Some(p) = vtodo.priority {
            push_line(&mut out, &format!("PRIORITY:{p}"));
        }
        if !vtodo.categories.is_empty() {
            let encoded: Vec<String> = vtodo.categories.iter().map(|c| encode_text(c)).collect();
            push_line(&mut out, &format!("CATEGORIES:{}", encoded.join(",")));
        }
        if let Some(r) = &vtodo.rrule {
            push_line(&mut out, &format!("RRULE:{r}"));
        }
        if let Some(l) = &vtodo.location {
            push_line(&mut out, &format!("LOCATION:{}", encode_text(l)));
        }
        for (key, value) in &vtodo.extra_properties {
            push_line(&mut out, &format!("{key}:{value}"));
        }
        push_line(&mut out, "END:VTODO");
    }

    push_line(&mut out, "END:VCALENDAR");
    out
}

/// Encode TEXT-typed property value per RFC 5545 §3.3.11:
/// backslash, semicolon, comma, newline escape; CR drops.
fn encode_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            other => out.push(other),
        }
    }
    out
}

/// Format a date as `YYYYMMDD` or a datetime as
/// `YYYYMMDDTHHMMSSZ`. The header carries the VALUE=DATE
/// parameter on the date-only path so the receiver knows the
/// shape without sniffing.
fn format_date_or_datetime(key: &str, value: DateOrDateTime) -> String {
    match value {
        DateOrDateTime::Date(d) => {
            format!("{key};VALUE=DATE:{}", d.format("%Y%m%d"))
        }
        DateOrDateTime::DateTime(dt) => {
            format!("{key}:{}", format_utc_datetime(dt))
        }
    }
}

fn format_utc_datetime(dt: DateTime<Utc>) -> String {
    dt.format("%Y%m%dT%H%M%SZ").to_string()
}

/// Append `line` to `out` with CRLF terminators, folding at 75
/// octets per RFC 5545 §3.1. Continuation lines start with one
/// SPACE.
fn push_line(out: &mut String, line: &str) {
    const FOLD_AT: usize = 75;
    let bytes = line.as_bytes();
    if bytes.len() <= FOLD_AT {
        let _ = write!(out, "{line}\r\n");
        return;
    }

    // Walk char boundaries so a fold doesn't split a UTF-8
    // codepoint. The first segment fits FOLD_AT bytes; later
    // segments fit FOLD_AT-1 bytes (the leading space costs
    // one octet on each continuation line).
    let mut start = 0;
    let mut emitted_first = false;
    let total = bytes.len();

    while start < total {
        let limit = if emitted_first { FOLD_AT - 1 } else { FOLD_AT };
        let mut end = (start + limit).min(total);
        // Walk back to a char boundary.
        while end < total && !line.is_char_boundary(end) {
            end -= 1;
        }
        let segment = &line[start..end];
        if emitted_first {
            let _ = write!(out, " {segment}\r\n");
        } else {
            let _ = write!(out, "{segment}\r\n");
            emitted_first = true;
        }
        start = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vtodo::parser::{ParsedIcs, parse_ics};
    use chrono::TimeZone;

    fn round_trip(components: Vec<VtodoOutput>) -> ParsedIcs {
        let config = EmitConfig::default();
        let text = emit_vcalendar(&components, &config);
        parse_ics(&text).expect("emitted text should parse")
    }

    fn fixed_dtstamp() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 28, 12, 0, 0).unwrap()
    }

    #[test]
    fn round_trips_basic_vtodo() {
        let vt = VtodoOutput {
            uid: "11111111-2222-3333-4444-555555555555".to_string(),
            dtstamp: fixed_dtstamp(),
            summary: Some("Buy milk".to_string()),
            description: Some("two percent".to_string()),
            dtstart: Some(DateOrDateTime::from_date(
                NaiveDate::from_ymd_opt(2026, 4, 15).unwrap(),
            )),
            due: Some(DateOrDateTime::DateTime(
                Utc.with_ymd_and_hms(2026, 4, 30, 23, 59, 59).unwrap(),
            )),
            completed: None,
            status: Some("NEEDS-ACTION".to_string()),
            priority: Some(5),
            categories: vec!["home".to_string(), "errands".to_string()],
            rrule: Some("FREQ=WEEKLY".to_string()),
            location: Some("Corner store".to_string()),
            extra_properties: Vec::new(),
        };
        let parsed = round_trip(vec![vt.clone()]);
        assert_eq!(parsed.vtodos.len(), 1);
        let r = &parsed.vtodos[0];
        assert_eq!(r.uid.as_deref(), Some(vt.uid.as_str()));
        assert_eq!(r.summary, vt.summary);
        assert_eq!(r.description, vt.description);
        assert_eq!(r.status, vt.status);
        assert_eq!(r.priority, vt.priority);
        assert_eq!(r.categories, vt.categories);
        assert_eq!(r.rrule, vt.rrule);
        assert_eq!(r.location, vt.location);
    }

    #[test]
    fn escapes_special_chars_in_text() {
        let vt = VtodoOutput {
            uid: "u1".to_string(),
            dtstamp: fixed_dtstamp(),
            summary: Some("line1\nline2, with comma; semicolon \\ backslash".to_string()),
            ..Default::default()
        };
        let text = emit_vcalendar(std::slice::from_ref(&vt), &EmitConfig::default());
        assert!(text.contains(r"SUMMARY:line1\nline2\, with comma\; semicolon \\ backslash"));
        let parsed = parse_ics(&text).unwrap();
        assert_eq!(parsed.vtodos[0].summary, vt.summary);
    }

    #[test]
    fn folds_long_lines_at_75_octets() {
        let long = "x".repeat(200);
        let vt = VtodoOutput {
            uid: "u".to_string(),
            dtstamp: fixed_dtstamp(),
            summary: Some(long.clone()),
            ..Default::default()
        };
        let text = emit_vcalendar(&[vt], &EmitConfig::default());
        // Every emitted physical line ≤ 75 octets + CRLF.
        for line in text.split("\r\n") {
            assert!(
                line.len() <= 75,
                "found unfolded line of {} bytes: {line:?}",
                line.len()
            );
        }
        // Re-parse must restore the long summary.
        let parsed = parse_ics(&text).unwrap();
        assert_eq!(parsed.vtodos[0].summary.as_deref(), Some(long.as_str()));
    }

    #[test]
    fn emits_dtstart_as_date_when_source_was_date_only() {
        let vt = VtodoOutput {
            uid: "u".to_string(),
            dtstamp: fixed_dtstamp(),
            dtstart: Some(DateOrDateTime::from_date(
                NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            )),
            ..Default::default()
        };
        let text = emit_vcalendar(&[vt], &EmitConfig::default());
        assert!(
            text.contains("DTSTART;VALUE=DATE:20260601"),
            "expected date-only DTSTART; got:\n{text}"
        );
    }

    #[test]
    fn empty_vtodo_list_still_emits_vcalendar_wrapper() {
        let text = emit_vcalendar(&[], &EmitConfig::default());
        assert!(text.starts_with("BEGIN:VCALENDAR\r\n"));
        assert!(text.contains("VERSION:2.0\r\n"));
        assert!(text.trim_end().ends_with("END:VCALENDAR"));
    }

    #[test]
    fn extras_emit_after_modeled_properties() {
        let vt = VtodoOutput {
            uid: "u".to_string(),
            dtstamp: fixed_dtstamp(),
            summary: Some("s".to_string()),
            extra_properties: vec![
                ("X-CUSTOM".to_string(), "v1".to_string()),
                ("X-ANOTHER".to_string(), "v2".to_string()),
            ],
            ..Default::default()
        };
        let text = emit_vcalendar(&[vt], &EmitConfig::default());
        let summary_idx = text.find("SUMMARY:").unwrap();
        let custom_idx = text.find("X-CUSTOM:").unwrap();
        let another_idx = text.find("X-ANOTHER:").unwrap();
        assert!(summary_idx < custom_idx);
        assert!(custom_idx < another_idx);
    }
}
