// SPDX-License-Identifier: MIT
//! Hand-rolled RFC 5545 VTODO parser. Stdlib-only.
//!
//! Scope is bounded: Atrium needs a VTODO subset, not a full
//! iCalendar implementation. The parser:
//!
//! - Unfolds continuation lines (`CRLF + SPACE/TAB` → join).
//! - Decodes TEXT escapes (`\n` / `\N` → LF, `\,` → `,`,
//!   `\;` → `;`, `\\` → `\`) per RFC 5545 §3.3.11.
//! - Parses properties as `KEY[;PARAM=VALUE...]:VALUE`.
//! - Walks `BEGIN:X ... END:X` component blocks.
//! - Captures every VTODO into a typed [`VtodoComponent`] with
//!   modeled fields populated and a small set of "saw this"
//!   counters the mapper turns into lossy entries.
//! - Tolerates non-VTODO components (VEVENT, VJOURNAL,
//!   VTIMEZONE, X-*) at the top level — records them in
//!   [`ParsedIcs::unsupported_top_level`] for the mapper's
//!   lossy report and otherwise skips them.
//!
//! What we deliberately don't do:
//!
//! - No VTIMEZONE handling. Non-UTC timestamps surface in the
//!   per-component `had_timezone` flag; the mapper flags them
//!   as `LossyKind::DroppedTimezone` and the date portion still
//!   threads through.
//! - No DURATION computation. A VTODO with DURATION but no DUE
//!   is rare; we record it on the component and the mapper
//!   reports as lossy.
//! - No VALARM round-trip. Atrium's reminders are independent
//!   (see `task.reminder_at`); cross-mapping is a future ticket.
//!   We count alarms per-VTODO so the mapper can surface the
//!   drop.

use std::fmt;

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};

/// Errors the parser surfaces. Malformed input is the only
/// hard failure; missing-property is the mapper's concern.
#[derive(Debug)]
pub enum ParseError {
    /// A `BEGIN:X` had no matching `END:X` before EOF.
    UnclosedComponent(String),
    /// An `END:X` appeared without a matching `BEGIN:X`.
    UnmatchedEnd(String),
    /// A property line lacked a `:` separator after the key
    /// + optional parameters.
    MalformedProperty(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnclosedComponent(name) => write!(f, "unclosed component: BEGIN:{name}"),
            Self::UnmatchedEnd(name) => write!(f, "unmatched END:{name}"),
            Self::MalformedProperty(line) => write!(f, "malformed property line: {line}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// One parsed VTODO, in Atrium's typed shape. Modeled fields
/// thread to typed columns through the mapper; `extras` and
/// the counters (`alarm_count`, `attendee_count`, etc.) turn
/// into lossy report entries.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct VtodoComponent {
    pub uid: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub dtstart: Option<DateOrDateTime>,
    pub due: Option<DateOrDateTime>,
    pub completed: Option<DateTime<Utc>>,
    pub status: Option<String>,
    pub priority: Option<u8>,
    /// Multi-valued; `CATEGORIES:home,finance` → `vec!["home", "finance"]`.
    pub categories: Vec<String>,
    /// Full RFC 5545 RRULE value-portion, verbatim
    /// (`FREQ=WEEKLY;BYDAY=MO,WE`). Atrium stores this as-is.
    pub rrule: Option<String>,
    /// `LOCATION` — Atrium has no typed home; the mapper
    /// stashes into `extra_properties["VTODO_LOCATION"]`.
    pub location: Option<String>,

    /// Unmodeled `X-*` properties (and any other key not in
    /// the modeled set). Preserved as `(key, value)` pairs;
    /// the mapper folds them into `task.extra_properties`.
    pub x_properties: Vec<(String, String)>,

    /// Per-VTODO tally — for lossy reporting only.
    pub alarm_count: usize,
    pub attendee_count: usize,
    pub has_geo: bool,
    pub percent_complete: Option<u8>,
    pub has_duration: bool,
    /// True when at least one of DTSTART / DUE / COMPLETED
    /// carried a TZID parameter (non-UTC). Atrium drops the
    /// timezone and the mapper flags the loss.
    pub had_timezone: bool,
    /// Unmodeled property names (post-stash). Mapper surfaces
    /// one lossy entry per kind on first occurrence; this
    /// list lets it group, not enumerate every line.
    pub unknown_property_names: Vec<String>,
}

/// A VTODO's DTSTART/DUE/COMPLETED value: either a date or
/// a UTC datetime. We never carry tzinfo into the DB — the
/// `had_timezone` flag on the parent component is enough for
/// the mapper to surface the loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateOrDateTime {
    Date(NaiveDate),
    DateTime(DateTime<Utc>),
}

impl DateOrDateTime {
    pub fn date(&self) -> NaiveDate {
        match self {
            Self::Date(d) => *d,
            Self::DateTime(dt) => dt.date_naive(),
        }
    }

    pub fn time(&self) -> Option<chrono::NaiveTime> {
        match self {
            Self::Date(_) => None,
            Self::DateTime(dt) => Some(dt.time()),
        }
    }
}

/// Top-level parse result: every VTODO Atrium recognised plus
/// a list of unsupported top-level component names the file
/// contained (VEVENT, VJOURNAL, VFREEBUSY, etc.) so the mapper
/// can surface them in the lossy report.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ParsedIcs {
    pub vtodos: Vec<VtodoComponent>,
    /// Names of non-VTODO components seen at the top level.
    /// Duplicates preserved so the mapper can show the count.
    pub unsupported_top_level: Vec<String>,
}

/// Parse a `.ics` text into Atrium's typed VTODO shape.
pub fn parse_ics(text: &str) -> Result<ParsedIcs, ParseError> {
    let lines = unfold_lines(text);
    let mut result = ParsedIcs::default();
    let mut stack: Vec<String> = Vec::new();
    let mut current_vtodo: Option<VtodoComponent> = None;

    for line in lines {
        if line.is_empty() {
            continue;
        }
        let property = parse_property(&line)?;

        match property.key.as_str() {
            "BEGIN" => {
                let name = property.value.to_uppercase();
                stack.push(name.clone());
                // Top-level non-VTODO components: record once
                // and skip ahead. We still walk lines so a
                // nested component (e.g. VTIMEZONE → STANDARD)
                // is consumed; the BEGIN/END stack handles that.
                if name == "VTODO" && stack.len() == 2 {
                    // VTODO inside VCALENDAR.
                    current_vtodo = Some(VtodoComponent::default());
                } else if stack.len() == 2 && name != "VTODO" {
                    // Top-level non-VTODO component — record
                    // for the lossy report.
                    result.unsupported_top_level.push(name);
                } else if name == "VALARM" {
                    // Nested VALARM inside the current VTODO.
                    if let Some(vt) = current_vtodo.as_mut() {
                        vt.alarm_count += 1;
                    }
                }
                continue;
            }
            "END" => {
                let name = property.value.to_uppercase();
                match stack.pop() {
                    Some(top) if top == name => {}
                    Some(top) => {
                        return Err(ParseError::UnmatchedEnd(format!(
                            "expected END:{top}, got END:{name}"
                        )));
                    }
                    None => return Err(ParseError::UnmatchedEnd(name)),
                }
                if name == "VTODO"
                    && let Some(vt) = current_vtodo.take()
                {
                    result.vtodos.push(vt);
                }
                continue;
            }
            _ => {}
        }

        // Property assignment is only meaningful inside a
        // VTODO block at depth 2. Anything else is consumed
        // and ignored (e.g. VTIMEZONE's properties).
        if stack.len() != 2 || stack.last().map(String::as_str) != Some("VTODO") {
            continue;
        }
        let Some(vtodo) = current_vtodo.as_mut() else {
            continue;
        };

        apply_property_to_vtodo(vtodo, &property);
    }

    if !stack.is_empty() {
        return Err(ParseError::UnclosedComponent(stack.join(",")));
    }
    Ok(result)
}

/// Property in tokenised form: key + parameters + raw value.
/// Text-escape decoding happens at apply-time, not here, so
/// parameter values stay verbatim (`TZID=America/New_York` etc.).
#[derive(Debug, Clone)]
struct Property {
    key: String,
    /// `(name, value)` pairs from `;PARAM=value` segments.
    /// Names uppercased per RFC 5545 §3.2.
    params: Vec<(String, String)>,
    /// Raw value portion (everything after the first
    /// non-quoted `:`). Escape decoding deferred.
    value: String,
}

impl Property {
    fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn parse_property(line: &str) -> Result<Property, ParseError> {
    // Split key+params from value at the first non-quoted `:`.
    let mut in_quote = false;
    let mut colon_idx = None;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => in_quote = !in_quote,
            ':' if !in_quote => {
                colon_idx = Some(i);
                break;
            }
            _ => {}
        }
    }
    let Some(idx) = colon_idx else {
        return Err(ParseError::MalformedProperty(line.to_string()));
    };
    let (head, rest) = line.split_at(idx);
    let value = rest[1..].to_string();

    // Split head into key + parameters at unquoted `;`.
    let mut parts: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut in_q = false;
    for ch in head.chars() {
        match ch {
            '"' => {
                in_q = !in_q;
                buf.push(ch);
            }
            ';' if !in_q => {
                parts.push(std::mem::take(&mut buf));
            }
            _ => buf.push(ch),
        }
    }
    parts.push(buf);

    let key = parts.remove(0).to_uppercase();
    let mut params: Vec<(String, String)> = Vec::with_capacity(parts.len());
    for part in parts {
        // PARAM=VALUE — quoted values strip the surrounding `"`.
        if let Some(eq) = part.find('=') {
            let name = part[..eq].to_uppercase();
            let raw = &part[eq + 1..];
            let value = if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
                raw[1..raw.len() - 1].to_string()
            } else {
                raw.to_string()
            };
            params.push((name, value));
        } else {
            // Stray parameter with no `=` — keep as a flag-style
            // (`name`, "").
            params.push((part.to_uppercase(), String::new()));
        }
    }

    Ok(Property { key, params, value })
}

/// Decode a TEXT-typed property value per RFC 5545 §3.3.11.
/// Reserved for the SUMMARY / DESCRIPTION / LOCATION properties
/// and CATEGORIES values; structured value types (DATE, RRULE,
/// PRIORITY, STATUS, UID) skip decoding because the escape
/// sequence is invalid in their grammar.
fn decode_text(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.peek() {
            Some('n' | 'N') => {
                chars.next();
                out.push('\n');
            }
            Some(',') => {
                chars.next();
                out.push(',');
            }
            Some(';') => {
                chars.next();
                out.push(';');
            }
            Some('\\') => {
                chars.next();
                out.push('\\');
            }
            _ => {
                // Lone `\` — keep verbatim. Spec says illegal
                // but real-world `.ics` files survive on this.
                out.push('\\');
            }
        }
    }
    out
}

/// Apply one parsed property to the in-flight VTODO. Only the
/// modeled set of keys lands in typed fields; X-* and other
/// unmodeled names route to `x_properties` for the mapper to
/// stash. `unknown_property_names` records dropped-by-design
/// keys (PERCENT-COMPLETE etc.) so the mapper can surface them
/// in the lossy report.
fn apply_property_to_vtodo(vtodo: &mut VtodoComponent, property: &Property) {
    match property.key.as_str() {
        "UID" => vtodo.uid = Some(property.value.clone()),
        "SUMMARY" => vtodo.summary = Some(decode_text(&property.value)),
        "DESCRIPTION" => vtodo.description = Some(decode_text(&property.value)),
        "DTSTART" => {
            if property.param("TZID").is_some() {
                vtodo.had_timezone = true;
            }
            if let Some(parsed) = parse_date_or_datetime(&property.value, property.param("VALUE")) {
                vtodo.dtstart = Some(parsed);
            }
        }
        "DUE" => {
            if property.param("TZID").is_some() {
                vtodo.had_timezone = true;
            }
            if let Some(parsed) = parse_date_or_datetime(&property.value, property.param("VALUE")) {
                vtodo.due = Some(parsed);
            }
        }
        "COMPLETED" => {
            if property.param("TZID").is_some() {
                vtodo.had_timezone = true;
            }
            if let Some(DateOrDateTime::DateTime(dt)) =
                parse_date_or_datetime(&property.value, property.param("VALUE"))
            {
                vtodo.completed = Some(dt);
            } else if let Some(DateOrDateTime::Date(d)) =
                parse_date_or_datetime(&property.value, property.param("VALUE"))
            {
                // COMPLETED with a date-only value is non-spec but
                // tolerated — promote to midnight UTC.
                if let Some(dt) = d.and_hms_opt(0, 0, 0) {
                    vtodo.completed = Some(dt.and_utc());
                }
            }
        }
        "STATUS" => vtodo.status = Some(property.value.to_uppercase()),
        "PRIORITY" => {
            if let Ok(n) = property.value.trim().parse::<u8>() {
                vtodo.priority = Some(n);
            }
        }
        "CATEGORIES" => {
            // Comma-separated, escape-aware: `\,` is a literal
            // comma inside one category name.
            for raw in split_unescaped_commas(&property.value) {
                let decoded = decode_text(&raw);
                if !decoded.is_empty() {
                    vtodo.categories.push(decoded);
                }
            }
        }
        "RRULE" => vtodo.rrule = Some(property.value.clone()),
        "LOCATION" => vtodo.location = Some(decode_text(&property.value)),
        // Dropped-by-design — recorded for the lossy report.
        "GEO" => vtodo.has_geo = true,
        "ATTENDEE" | "ORGANIZER" => vtodo.attendee_count += 1,
        "PERCENT-COMPLETE" => {
            if let Ok(n) = property.value.trim().parse::<u8>() {
                vtodo.percent_complete = Some(n);
            }
        }
        "DURATION" => vtodo.has_duration = true,
        // Atrium auto-stamps these; ignored without surfacing.
        "CREATED" | "LAST-MODIFIED" | "DTSTAMP" | "SEQUENCE" | "CLASS" | "TRANSP" => {}
        other if other.starts_with("X-") => {
            vtodo
                .x_properties
                .push((other.to_string(), property.value.clone()));
        }
        other => {
            // Unknown property — record name for lossy report,
            // don't keep value (we can't preserve what we don't
            // model on emit).
            vtodo.unknown_property_names.push(other.to_string());
        }
    }
}

/// Parse a `DATE` or `DATE-TIME` value string. Recognises:
///
/// - `YYYYMMDD` (date)
/// - `YYYYMMDDTHHMMSSZ` (UTC datetime)
/// - `YYYYMMDDTHHMMSS` (floating local — promoted to UTC)
///
/// The `VALUE` parameter hint (`VALUE=DATE`) forces the date
/// interpretation when set.
fn parse_date_or_datetime(raw: &str, value_param: Option<&str>) -> Option<DateOrDateTime> {
    let raw = raw.trim();
    let is_explicit_date = value_param.is_some_and(|v| v.eq_ignore_ascii_case("DATE"));

    if is_explicit_date || (raw.len() == 8 && !raw.contains('T')) {
        return NaiveDate::parse_from_str(raw, "%Y%m%d")
            .ok()
            .map(DateOrDateTime::Date);
    }

    // UTC datetime — explicit `Z` suffix.
    if let Some(body) = raw.strip_suffix('Z') {
        return NaiveDateTime::parse_from_str(body, "%Y%m%dT%H%M%S")
            .ok()
            .map(|ndt| DateOrDateTime::DateTime(ndt.and_utc()));
    }

    // Floating datetime — promote to UTC, mapper flags the
    // timezone loss separately via `had_timezone`.
    if let Ok(ndt) = NaiveDateTime::parse_from_str(raw, "%Y%m%dT%H%M%S") {
        return Some(DateOrDateTime::DateTime(ndt.and_utc()));
    }
    None
}

/// Split a value at unescaped commas. `\,` is a literal comma
/// in the result token; `,` is a separator.
fn split_unescaped_commas(value: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && matches!(chars.peek(), Some(',')) {
            buf.push('\\');
            buf.push(chars.next().unwrap());
            continue;
        }
        if ch == ',' {
            out.push(std::mem::take(&mut buf));
            continue;
        }
        buf.push(ch);
    }
    out.push(buf);
    out
}

/// Unfold line continuations per RFC 5545 §3.1. A CRLF (or LF)
/// followed by SPACE or TAB is folding; the leading whitespace
/// is consumed and the rest of the line joins the previous.
/// Returns lines with neither CR nor trailing whitespace.
fn unfold_lines(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut started = false;
    for raw in text.split('\n') {
        // Strip optional trailing CR (handle both CRLF + LF).
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.starts_with(' ') || line.starts_with('\t') {
            // Continuation — append the rest (drop one leading
            // SPACE/TAB) onto the current line.
            current.push_str(&line[1..]);
            continue;
        }
        if started {
            out.push(std::mem::take(&mut current));
        }
        current.push_str(line);
        started = true;
    }
    if started {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> ParsedIcs {
        parse_ics(text).expect("parse_ics should succeed")
    }

    #[test]
    fn unfolds_continuation_lines() {
        let text = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nSUMMARY:Long line\r\n splits here\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        let parsed = parse(text);
        assert_eq!(parsed.vtodos.len(), 1);
        assert_eq!(
            parsed.vtodos[0].summary.as_deref(),
            Some("Long linesplits here"),
        );
    }

    #[test]
    fn decodes_escapes_in_text_properties() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VTODO\nSUMMARY:line1\\nline2\\, with comma\nEND:VTODO\nEND:VCALENDAR\n";
        let parsed = parse(text);
        assert_eq!(
            parsed.vtodos[0].summary.as_deref(),
            Some("line1\nline2, with comma"),
        );
    }

    #[test]
    fn parses_basic_modeled_properties() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VTODO\nUID:abc-123\nSUMMARY:Buy milk\nDESCRIPTION:two percent\nDUE:20260430T235959Z\nDTSTART:20260415\nSTATUS:NEEDS-ACTION\nPRIORITY:5\nCATEGORIES:home,errands\nRRULE:FREQ=WEEKLY\nLOCATION:Corner store\nEND:VTODO\nEND:VCALENDAR\n";
        let parsed = parse(text);
        assert_eq!(parsed.vtodos.len(), 1);
        let v = &parsed.vtodos[0];
        assert_eq!(v.uid.as_deref(), Some("abc-123"));
        assert_eq!(v.summary.as_deref(), Some("Buy milk"));
        assert_eq!(v.description.as_deref(), Some("two percent"));
        assert_eq!(
            v.due,
            Some(DateOrDateTime::DateTime(
                NaiveDate::from_ymd_opt(2026, 4, 30)
                    .unwrap()
                    .and_hms_opt(23, 59, 59)
                    .unwrap()
                    .and_utc(),
            )),
        );
        assert_eq!(
            v.dtstart,
            Some(DateOrDateTime::Date(
                NaiveDate::from_ymd_opt(2026, 4, 15).unwrap(),
            )),
        );
        assert_eq!(v.status.as_deref(), Some("NEEDS-ACTION"));
        assert_eq!(v.priority, Some(5));
        assert_eq!(v.categories, vec!["home", "errands"]);
        assert_eq!(v.rrule.as_deref(), Some("FREQ=WEEKLY"));
        assert_eq!(v.location.as_deref(), Some("Corner store"));
    }

    #[test]
    fn captures_x_properties_separately() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VTODO\nUID:1\nX-CUSTOM-FIELD:value here\nX-ANOTHER:second\nEND:VTODO\nEND:VCALENDAR\n";
        let parsed = parse(text);
        assert_eq!(
            parsed.vtodos[0].x_properties,
            vec![
                ("X-CUSTOM-FIELD".to_string(), "value here".to_string()),
                ("X-ANOTHER".to_string(), "second".to_string()),
            ],
        );
    }

    #[test]
    fn flags_timezone_on_dtstart() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VTODO\nUID:tz\nDTSTART;TZID=America/New_York:20260415T103000\nEND:VTODO\nEND:VCALENDAR\n";
        let parsed = parse(text);
        assert!(parsed.vtodos[0].had_timezone);
    }

    #[test]
    fn counts_alarms_and_attendees() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VTODO\nUID:1\nATTENDEE:mailto:a@x\nATTENDEE:mailto:b@x\nBEGIN:VALARM\nACTION:DISPLAY\nEND:VALARM\nBEGIN:VALARM\nACTION:AUDIO\nEND:VALARM\nEND:VTODO\nEND:VCALENDAR\n";
        let parsed = parse(text);
        let v = &parsed.vtodos[0];
        assert_eq!(v.attendee_count, 2);
        assert_eq!(v.alarm_count, 2);
    }

    #[test]
    fn skips_non_vtodo_top_level_components() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VEVENT\nSUMMARY:not a todo\nEND:VEVENT\nBEGIN:VTODO\nUID:a\nSUMMARY:real\nEND:VTODO\nBEGIN:VJOURNAL\nEND:VJOURNAL\nEND:VCALENDAR\n";
        let parsed = parse(text);
        assert_eq!(parsed.vtodos.len(), 1);
        assert_eq!(parsed.vtodos[0].uid.as_deref(), Some("a"));
        assert_eq!(parsed.unsupported_top_level, vec!["VEVENT", "VJOURNAL"]);
    }

    #[test]
    fn tolerates_vtimezone_block_without_recording_lossy() {
        // VTIMEZONE is recorded as unsupported_top_level so the
        // mapper can flag the presence; the mapper then routes
        // the per-VTODO `had_timezone` flag to the lossy report.
        let text = "BEGIN:VCALENDAR\nBEGIN:VTIMEZONE\nTZID:America/New_York\nBEGIN:STANDARD\nDTSTART:19700101T000000\nEND:STANDARD\nEND:VTIMEZONE\nBEGIN:VTODO\nUID:tz\nEND:VTODO\nEND:VCALENDAR\n";
        let parsed = parse(text);
        assert_eq!(parsed.vtodos.len(), 1);
        assert_eq!(parsed.unsupported_top_level, vec!["VTIMEZONE"]);
    }

    #[test]
    fn parses_two_vtodos() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VTODO\nUID:1\nSUMMARY:one\nEND:VTODO\nBEGIN:VTODO\nUID:2\nSUMMARY:two\nEND:VTODO\nEND:VCALENDAR\n";
        let parsed = parse(text);
        assert_eq!(parsed.vtodos.len(), 2);
        assert_eq!(parsed.vtodos[0].uid.as_deref(), Some("1"));
        assert_eq!(parsed.vtodos[1].uid.as_deref(), Some("2"));
    }

    #[test]
    fn unmatched_end_is_an_error() {
        let text = "BEGIN:VCALENDAR\nEND:VTODO\n";
        let err = parse_ics(text).unwrap_err();
        assert!(matches!(err, ParseError::UnmatchedEnd(_)));
    }

    #[test]
    fn unclosed_component_is_an_error() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VTODO\nUID:1\n";
        let err = parse_ics(text).unwrap_err();
        assert!(matches!(err, ParseError::UnclosedComponent(_)));
    }

    #[test]
    fn decode_text_handles_all_escapes() {
        assert_eq!(decode_text(r"a\nb"), "a\nb");
        assert_eq!(decode_text(r"a\Nb"), "a\nb");
        assert_eq!(decode_text(r"a\,b"), "a,b");
        assert_eq!(decode_text(r"a\;b"), "a;b");
        assert_eq!(decode_text(r"a\\b"), "a\\b");
    }

    #[test]
    fn split_unescaped_commas_preserves_escaped() {
        let out = split_unescaped_commas(r"home\, garden,work");
        assert_eq!(out, vec![r"home\, garden".to_string(), "work".to_string()]);
    }

    #[test]
    fn parse_property_separates_params_from_value() {
        let p = parse_property("DTSTART;TZID=America/New_York:20260415T103000").unwrap();
        assert_eq!(p.key, "DTSTART");
        assert_eq!(p.params.len(), 1);
        assert_eq!(p.params[0].0, "TZID");
        assert_eq!(p.params[0].1, "America/New_York");
        assert_eq!(p.value, "20260415T103000");
    }

    #[test]
    fn parse_property_handles_quoted_param_value() {
        let p = parse_property(r#"ATTENDEE;CN="Doe, John":mailto:jdoe@x"#).unwrap();
        assert_eq!(p.key, "ATTENDEE");
        assert_eq!(p.params[0].0, "CN");
        assert_eq!(p.params[0].1, "Doe, John");
        assert_eq!(p.value, "mailto:jdoe@x");
    }

    #[test]
    fn parse_property_value_with_colon_after_first() {
        let p = parse_property("ATTENDEE:mailto:jdoe@example.com").unwrap();
        assert_eq!(p.key, "ATTENDEE");
        assert_eq!(p.value, "mailto:jdoe@example.com");
    }

    #[test]
    fn unmatched_begin_end_kinds_errors() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VTODO\nEND:VEVENT\nEND:VCALENDAR\n";
        let err = parse_ics(text).unwrap_err();
        assert!(matches!(err, ParseError::UnmatchedEnd(_)));
    }

    #[test]
    fn completed_with_date_only_value_promotes_to_midnight_utc() {
        let text = "BEGIN:VCALENDAR\nBEGIN:VTODO\nUID:1\nCOMPLETED;VALUE=DATE:20260501\nEND:VTODO\nEND:VCALENDAR\n";
        let parsed = parse(text);
        let dt = parsed.vtodos[0].completed.unwrap();
        assert_eq!(
            dt.date_naive(),
            NaiveDate::from_ymd_opt(2026, 5, 1).unwrap()
        );
    }
}
