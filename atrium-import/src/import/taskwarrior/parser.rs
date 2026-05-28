// SPDX-License-Identifier: MIT
//! Taskwarrior `task export` JSON parser. v0.26.0.
//!
//! Taskwarrior writes two shapes:
//!
//! - **Array form** (default, `json.array=on`): a single JSON
//!   document, a top-level `[ {…}, {…}, … ]` array.
//! - **Line-stream form** (`json.array=off`): one JSON object
//!   per line, no top-level array, lines separated by `\n`.
//!
//! Both round-trip through the same downstream mapper. We detect
//! by sniffing the first non-whitespace byte after the BOM (if
//! any): `[` is array, anything else is line-stream.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use serde_json::Value;

/// Errors the parser surfaces. Malformed JSON is the only hard
/// failure; missing-field is the mapper's concern (a task with
/// no description still parses; the mapper falls back to a
/// placeholder title).
#[derive(Debug)]
pub enum ParseError {
    /// `serde_json` rejected the document.
    Json(serde_json::Error),
    /// A non-array, non-object value at the top level. Real
    /// Taskwarrior output never does this; we treat it as a
    /// configuration mistake.
    NotTaskShape,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(e) => write!(f, "JSON parse error: {e}"),
            Self::NotTaskShape => write!(f, "input was neither a JSON array nor a JSON object"),
        }
    }
}

impl std::error::Error for ParseError {}

impl From<serde_json::Error> for ParseError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

/// One Taskwarrior task in Atrium's typed shape. Modeled fields
/// thread to typed columns through the mapper; `udas` is the
/// stash for everything else (unknown JSON keys), routed per
/// the `--uda-as` flag.
///
/// We don't attempt to model every possible Taskwarrior field
/// here — only the ones with an Atrium-side home or a lossy
/// surfacing. Unknown keys flow through `udas`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct TaskwarriorTask {
    pub uuid: Option<String>,
    pub description: Option<String>,
    /// Lowercase: `pending` / `completed` / `deleted` / `waiting` /
    /// `recurring`. Anything else surfaces in the lossy report.
    pub status: Option<String>,
    pub project: Option<String>,
    pub tags: Vec<String>,
    /// `H` / `M` / `L`. Anything else is dropped.
    pub priority: Option<String>,
    pub due: Option<DateOrDateTime>,
    pub scheduled: Option<DateOrDateTime>,
    pub wait: Option<DateOrDateTime>,
    pub until: Option<DateOrDateTime>,
    pub end: Option<DateTime<Utc>>,
    pub start: Option<DateTime<Utc>>,
    pub recur: Option<String>,
    pub parent: Option<String>,
    pub mask: Option<String>,
    pub imask: Option<i64>,
    pub annotations: Vec<Annotation>,
    /// Comma-separated UUID string per the RFC. Round-trips
    /// verbatim into the lossy report (the v0.29.0 dependencies
    /// schema will re-parse it later).
    pub depends: Option<String>,
    /// User-defined attributes — anything that wasn't one of
    /// the modeled fields above. Stored as `(name, scalar
    /// representation)` for the mapper.
    pub udas: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub entry: Option<DateTime<Utc>>,
    pub description: String,
}

/// A Taskwarrior DATE field is `YYYYMMDDTHHMMSSZ`. Some custom
/// configurations strip the time portion; we tolerate both.
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

/// Parse the export document. Detects array vs line-stream form.
pub fn parse_export(text: &str) -> Result<Vec<TaskwarriorTask>, ParseError> {
    let body = strip_bom(text);
    let trimmed_start = body.trim_start();
    if trimmed_start.is_empty() {
        return Ok(Vec::new());
    }

    if trimmed_start.starts_with('[') {
        let value: Value = serde_json::from_str(body)?;
        let Value::Array(items) = value else {
            return Err(ParseError::NotTaskShape);
        };
        items.into_iter().map(value_to_task).collect()
    } else {
        // Line-stream form: one JSON object per non-empty line.
        body.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(|line| {
                let value: Value = serde_json::from_str(line)?;
                value_to_task(value)
            })
            .collect()
    }
}

fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

fn value_to_task(value: Value) -> Result<TaskwarriorTask, ParseError> {
    let Value::Object(map) = value else {
        return Err(ParseError::NotTaskShape);
    };

    let mut task = TaskwarriorTask::default();

    for (key, value) in map {
        match key.as_str() {
            "uuid" => task.uuid = scalar_string(&value),
            "description" => task.description = scalar_string(&value),
            "status" => task.status = scalar_string(&value).map(|s| s.to_lowercase()),
            "project" => task.project = scalar_string(&value),
            "priority" => task.priority = scalar_string(&value),
            "due" => task.due = scalar_string(&value).as_deref().and_then(parse_tw_date),
            "scheduled" => {
                task.scheduled = scalar_string(&value).as_deref().and_then(parse_tw_date)
            }
            "wait" => task.wait = scalar_string(&value).as_deref().and_then(parse_tw_date),
            "until" => task.until = scalar_string(&value).as_deref().and_then(parse_tw_date),
            "end" => task.end = scalar_string(&value).as_deref().and_then(parse_tw_datetime),
            "start" => task.start = scalar_string(&value).as_deref().and_then(parse_tw_datetime),
            "recur" => task.recur = scalar_string(&value),
            "parent" => task.parent = scalar_string(&value),
            "mask" => task.mask = scalar_string(&value),
            "imask" => {
                task.imask = match &value {
                    Value::Number(n) => n.as_i64(),
                    Value::String(s) => s.parse().ok(),
                    _ => None,
                };
            }
            "depends" => task.depends = scalar_string(&value),
            "urgency" => {
                // Computed metric — dropped silently, no lossy entry.
            }
            "entry" | "modified" => {
                // Atrium auto-stamps these; ignore without surfacing.
            }
            "tags" => {
                if let Value::Array(items) = value {
                    for item in items {
                        if let Some(s) = scalar_string(&item) {
                            task.tags.push(s);
                        }
                    }
                }
            }
            "annotations" => {
                if let Value::Array(items) = value {
                    for item in items {
                        let Value::Object(entry) = item else {
                            continue;
                        };
                        let when = entry
                            .get("entry")
                            .and_then(scalar_string)
                            .as_deref()
                            .and_then(parse_tw_datetime);
                        let description = entry
                            .get("description")
                            .and_then(scalar_string)
                            .unwrap_or_default();
                        if description.is_empty() {
                            continue;
                        }
                        task.annotations.push(Annotation {
                            entry: when,
                            description,
                        });
                    }
                }
            }
            other => {
                // Unknown key — UDA stash. Scalars only; the
                // Taskwarrior format documents UDAs as scalar
                // strings, numbers, or fixed values. Arrays / nested
                // objects are unexpected and drop here.
                if let Some(rendered) = scalar_string(&value) {
                    task.udas.insert(other.to_string(), rendered);
                }
            }
        }
    }

    Ok(task)
}

fn scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        Value::Null => None,
        Value::Array(_) | Value::Object(_) => None,
    }
}

/// Parse a Taskwarrior DATE value (`YYYYMMDDTHHMMSSZ` per the RFC,
/// or `YYYYMMDD` on some non-standard exports). Floating-local
/// datetimes (no `Z`) get promoted to UTC silently.
fn parse_tw_date(raw: &str) -> Option<DateOrDateTime> {
    parse_tw_datetime(raw)
        .map(DateOrDateTime::DateTime)
        .or_else(|| {
            if raw.len() == 8 && !raw.contains('T') {
                NaiveDate::parse_from_str(raw, "%Y%m%d")
                    .ok()
                    .map(DateOrDateTime::Date)
            } else {
                None
            }
        })
}

fn parse_tw_datetime(raw: &str) -> Option<DateTime<Utc>> {
    let stripped = raw.strip_suffix('Z').unwrap_or(raw);
    NaiveDateTime::parse_from_str(stripped, "%Y%m%dT%H%M%S")
        .ok()
        .map(|ndt| Utc.from_utc_datetime(&ndt))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_object_array_form() {
        let text = r#"[{"uuid":"11111111-2222-3333-4444-555555555555","description":"Buy milk","status":"pending"}]"#;
        let tasks = parse_export(text).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks[0].uuid.as_deref(),
            Some("11111111-2222-3333-4444-555555555555")
        );
        assert_eq!(tasks[0].description.as_deref(), Some("Buy milk"));
        assert_eq!(tasks[0].status.as_deref(), Some("pending"));
    }

    #[test]
    fn parses_line_stream_form() {
        let text = concat!(
            r#"{"uuid":"a","description":"one","status":"pending"}"#,
            "\n",
            r#"{"uuid":"b","description":"two","status":"completed","end":"20260501T100000Z"}"#,
        );
        let tasks = parse_export(text).unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].description.as_deref(), Some("one"));
        assert_eq!(tasks[1].status.as_deref(), Some("completed"));
        assert!(tasks[1].end.is_some());
    }

    #[test]
    fn tolerates_utf8_bom_on_array_form() {
        let text = format!("\u{feff}[{}]", r#"{"uuid":"x","description":"with BOM"}"#);
        let tasks = parse_export(&text).unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].description.as_deref(), Some("with BOM"));
    }

    #[test]
    fn parses_dates_in_yyyymmddthhmmssz_form() {
        let text = r#"[{"uuid":"x","description":"d","due":"20260430T235959Z","scheduled":"20260415T100000Z"}]"#;
        let tasks = parse_export(text).unwrap();
        let v = &tasks[0];
        let due = v.due.unwrap();
        assert_eq!(due.date(), NaiveDate::from_ymd_opt(2026, 4, 30).unwrap());
        assert_eq!(
            due.time(),
            Some(chrono::NaiveTime::from_hms_opt(23, 59, 59).unwrap())
        );
        assert!(v.scheduled.is_some());
    }

    #[test]
    fn captures_unknown_fields_as_udas() {
        let text = r#"[{"uuid":"x","description":"d","effort":"large","client":"Acme"}]"#;
        let tasks = parse_export(text).unwrap();
        assert_eq!(
            tasks[0].udas.get("effort").map(String::as_str),
            Some("large")
        );
        assert_eq!(
            tasks[0].udas.get("client").map(String::as_str),
            Some("Acme")
        );
    }

    #[test]
    fn captures_tags_array() {
        let text = r#"[{"uuid":"x","description":"d","tags":["home","work"]}]"#;
        let tasks = parse_export(text).unwrap();
        assert_eq!(tasks[0].tags, vec!["home", "work"]);
    }

    #[test]
    fn captures_annotations_with_entry_dates() {
        let text = r#"[{"uuid":"x","description":"d","annotations":[{"entry":"20260101T120000Z","description":"first note"},{"entry":"20260201T080000Z","description":"second note"}]}]"#;
        let tasks = parse_export(text).unwrap();
        assert_eq!(tasks[0].annotations.len(), 2);
        assert_eq!(tasks[0].annotations[0].description, "first note");
        assert!(tasks[0].annotations[0].entry.is_some());
    }

    #[test]
    fn drops_urgency_field_silently() {
        let text = r#"[{"uuid":"x","description":"d","urgency":2.4}]"#;
        let tasks = parse_export(text).unwrap();
        // Urgency is not a UDA — it's a known-dropped Taskwarrior
        // computed field. Should not surface in udas.
        assert!(!tasks[0].udas.contains_key("urgency"));
    }

    #[test]
    fn rejects_top_level_non_array_non_object() {
        let err = parse_export(r#""just a string""#).unwrap_err();
        assert!(matches!(err, ParseError::NotTaskShape));
    }

    #[test]
    fn empty_input_yields_empty_vec() {
        assert!(parse_export("").unwrap().is_empty());
        assert!(parse_export("   \n  ").unwrap().is_empty());
    }

    #[test]
    fn lowercases_status_for_round_trip_parity() {
        let text = r#"[{"uuid":"x","description":"d","status":"PENDING"}]"#;
        let tasks = parse_export(text).unwrap();
        assert_eq!(tasks[0].status.as_deref(), Some("pending"));
    }

    #[test]
    fn integer_imask_round_trips() {
        let text = r#"[{"uuid":"x","description":"d","imask":3}]"#;
        let tasks = parse_export(text).unwrap();
        assert_eq!(tasks[0].imask, Some(3));
    }
}
