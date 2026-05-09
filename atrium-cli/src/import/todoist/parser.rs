// SPDX-License-Identifier: MIT
//! Hand-rolled CSV parser for Todoist exports.
//!
//! Tolerant of:
//!
//! - UTF-8 BOM at the start of the file (Todoist's CSV export
//!   adds one).
//! - Quoted fields with embedded commas (`"Check for milk, add to
//!   list"` is one field).
//! - Escaped double-quotes inside quoted fields (`""inside""`).
//! - Blank lines used as visual separators between sections.
//! - Trailing whitespace on lines.
//!
//! Doesn't depend on a CSV crate — same hand-roll discipline as
//! the Org parser. The Todoist column count is fixed at 15 so
//! we don't need general-purpose flexibility.
//!
//! The `TYPE` column gates the row class. We model that as a
//! typed enum so callers don't have to re-validate downstream.

use std::fmt;

/// 15 columns in the Todoist export, in order. The parser binds
/// values to these so callers don't have to remember positions.
const COLUMN_NAMES: &[&str] = &[
    "TYPE",
    "CONTENT",
    "DESCRIPTION",
    "IS_COLLAPSED",
    "PRIORITY",
    "INDENT",
    "AUTHOR",
    "RESPONSIBLE",
    "DATE",
    "DATE_LANG",
    "TIMEZONE",
    "DURATION",
    "DURATION_UNIT",
    "DEADLINE",
    "DEADLINE_LANG",
];

/// One row of the Todoist export, classified by the `TYPE` column.
/// Blank rows surface as [`TodoistRow::Blank`] so the caller can
/// preserve grouping intent without scanning whitespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TodoistRow {
    /// `meta` rows carry project-level UI hints (e.g.
    /// `view_style=board`). The raw value is preserved so the
    /// mapper interprets `key=value` shapes itself.
    Meta { value: String },
    /// `section` rows become Atrium headings within the project.
    Section { title: String, is_collapsed: bool },
    /// `task` rows are the headlines. `indent` is 1-based: 1 =
    /// top-level under the surrounding section / project, 2 =
    /// subtask of the previous indent-1 row, etc. The fixture
    /// goes 2 deep ("Check for essentials" / "Check for milk").
    /// Boxed so the enum stays small even though `TodoistTask`
    /// carries 14 fields — clippy::large_enum_variant.
    Task(Box<TodoistTask>),
    /// Blank separator row. Empty values across every column.
    Blank,
}

/// Parsed task row. Strings are kept verbatim — the mapper layer
/// strips `@labels` from `content`, parses `date`'s natural-
/// language phrasing into an RRULE, and translates `priority` /
/// `indent` to Atrium's domain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TodoistTask {
    pub content: String,
    pub description: String,
    pub is_collapsed: bool,
    pub priority: Option<u8>,
    pub indent: u8,
    pub author: Option<String>,
    pub responsible: Option<String>,
    pub date: Option<String>,
    pub date_lang: Option<String>,
    pub timezone: Option<String>,
    pub duration: Option<String>,
    pub duration_unit: Option<String>,
    pub deadline: Option<String>,
    pub deadline_lang: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Header row didn't match the expected Todoist columns.
    /// Carries the actual columns observed so the user can spot
    /// where the export drifted.
    UnexpectedHeader { observed: Vec<String> },
    /// `INDENT` field was set but didn't parse as a positive
    /// integer.
    BadIndent { line: usize, value: String },
    /// `PRIORITY` field was set but didn't parse 1-4.
    BadPriority { line: usize, value: String },
    /// A quoted field never closed before the file ended.
    UnterminatedQuote { line: usize },
    /// Row had more / fewer fields than the 15-column header.
    BadFieldCount {
        line: usize,
        expected: usize,
        got: usize,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedHeader { observed } => write!(
                f,
                "header row didn't match Todoist's column set; got: {observed:?}"
            ),
            Self::BadIndent { line, value } => {
                write!(f, "line {line}: INDENT {value:?} isn't a positive integer")
            }
            Self::BadPriority { line, value } => {
                write!(f, "line {line}: PRIORITY {value:?} isn't 1-4")
            }
            Self::UnterminatedQuote { line } => {
                write!(f, "line {line}: quoted field never closed")
            }
            Self::BadFieldCount {
                line,
                expected,
                got,
            } => {
                write!(f, "line {line}: expected {expected} fields, got {got}")
            }
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse a Todoist CSV export. Returns rows in source order;
/// blank separator lines preserved as [`TodoistRow::Blank`] so
/// the caller can treat them as section breaks if it wants.
pub fn parse_csv(text: &str) -> Result<Vec<TodoistRow>, ParseError> {
    let stripped = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let raw_rows = split_csv_rows(stripped)?;
    let mut iter = raw_rows.into_iter();

    let header = iter.next().unwrap_or_default();
    if !header_matches(&header) {
        return Err(ParseError::UnexpectedHeader { observed: header });
    }

    let mut out = Vec::new();
    for (idx, fields) in iter.enumerate() {
        // Header is line 1; the first data row is line 2 in
        // user-facing diagnostics.
        let line = idx + 2;
        out.push(classify_row(line, fields)?);
    }
    Ok(out)
}

fn header_matches(observed: &[String]) -> bool {
    if observed.len() != COLUMN_NAMES.len() {
        return false;
    }
    observed
        .iter()
        .zip(COLUMN_NAMES.iter())
        .all(|(o, e)| o.eq_ignore_ascii_case(e))
}

fn classify_row(line: usize, fields: Vec<String>) -> Result<TodoistRow, ParseError> {
    if fields.len() != COLUMN_NAMES.len() {
        return Err(ParseError::BadFieldCount {
            line,
            expected: COLUMN_NAMES.len(),
            got: fields.len(),
        });
    }

    let row_type = fields[0].trim();
    if fields.iter().all(|f| f.is_empty()) {
        return Ok(TodoistRow::Blank);
    }

    match row_type {
        "meta" => Ok(TodoistRow::Meta {
            value: fields[1].clone(),
        }),
        "section" => Ok(TodoistRow::Section {
            title: fields[1].clone(),
            is_collapsed: parse_bool(&fields[3]),
        }),
        "task" => Ok(TodoistRow::Task(Box::new(TodoistTask {
            content: fields[1].clone(),
            description: fields[2].clone(),
            is_collapsed: parse_bool(&fields[3]),
            priority: parse_priority(line, &fields[4])?,
            indent: parse_indent(line, &fields[5])?,
            author: optional(&fields[6]),
            responsible: optional(&fields[7]),
            date: optional(&fields[8]),
            date_lang: optional(&fields[9]),
            timezone: optional(&fields[10]),
            duration: optional(&fields[11]),
            duration_unit: optional(&fields[12]),
            deadline: optional(&fields[13]),
            deadline_lang: optional(&fields[14]),
        }))),
        // Empty TYPE on an otherwise-blank-looking line rarely
        // happens (the all-empty check above catches it), but
        // the export occasionally drops separator rows with a
        // few stray fields. Treat any unknown TYPE on a row
        // that's mostly empty as a Blank.
        "" => Ok(TodoistRow::Blank),
        // Unknown TYPE — preserve as Meta so the caller sees it
        // in the lossy report rather than dropping silently.
        _ => Ok(TodoistRow::Meta {
            value: format!("{}={}", fields[0], fields[1]),
        }),
    }
}

fn optional(field: &str) -> Option<String> {
    let trimmed = field.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn parse_bool(field: &str) -> bool {
    matches!(field.trim(), "True" | "true" | "TRUE" | "1")
}

fn parse_priority(line: usize, field: &str) -> Result<Option<u8>, ParseError> {
    let trimmed = field.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    match trimmed.parse::<u8>() {
        Ok(p) if (1..=4).contains(&p) => Ok(Some(p)),
        _ => Err(ParseError::BadPriority {
            line,
            value: trimmed.to_string(),
        }),
    }
}

fn parse_indent(line: usize, field: &str) -> Result<u8, ParseError> {
    let trimmed = field.trim();
    if trimmed.is_empty() {
        return Ok(1); // Default to top-level when omitted.
    }
    match trimmed.parse::<u8>() {
        Ok(n) if n >= 1 => Ok(n),
        _ => Err(ParseError::BadIndent {
            line,
            value: trimmed.to_string(),
        }),
    }
}

/// Split CSV text into rows of fields. Handles quoted fields
/// with embedded commas + escaped double-quotes. Trailing CRLF
/// or LF, and blank trailing rows, are tolerated.
fn split_csv_rows(text: &str) -> Result<Vec<Vec<String>>, ParseError> {
    let mut rows = Vec::new();
    let mut current_row: Vec<String> = Vec::new();
    let mut current_field = String::new();
    let mut in_quotes = false;
    let mut line = 1usize;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes => {
                if matches!(chars.peek(), Some('"')) {
                    // Escaped quote inside a quoted field.
                    current_field.push('"');
                    chars.next();
                } else {
                    in_quotes = false;
                }
            }
            '"' => {
                in_quotes = true;
            }
            ',' if !in_quotes => {
                current_row.push(std::mem::take(&mut current_field));
            }
            '\r' if !in_quotes => {
                // Swallow; LF will close the row.
                if matches!(chars.peek(), Some('\n')) {
                    continue;
                }
                // Bare CR — treat like LF for tolerance.
                current_row.push(std::mem::take(&mut current_field));
                rows.push(std::mem::take(&mut current_row));
                line += 1;
            }
            '\n' if !in_quotes => {
                current_row.push(std::mem::take(&mut current_field));
                rows.push(std::mem::take(&mut current_row));
                line += 1;
            }
            _ => current_field.push(ch),
        }
    }

    if in_quotes {
        return Err(ParseError::UnterminatedQuote { line });
    }

    // Trailing field / row without final newline.
    if !current_field.is_empty() || !current_row.is_empty() {
        current_row.push(current_field);
        rows.push(current_row);
    }

    // Drop completely-empty trailing rows (file ended with \n).
    while matches!(rows.last(), Some(r) if r.iter().all(|f| f.is_empty()) && r.len() <= 1) {
        rows.pop();
    }

    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!("../../../tests/fixtures/todoist/home.csv");

    #[test]
    fn fixture_parses_without_error() {
        let rows = parse_csv(FIXTURE).expect("home.csv should parse");
        assert!(!rows.is_empty(), "fixture produces rows");
    }

    #[test]
    fn fixture_carries_meta_view_style_board() {
        let rows = parse_csv(FIXTURE).unwrap();
        let first_meta = rows
            .iter()
            .find(|r| matches!(r, TodoistRow::Meta { .. }))
            .expect("meta row exists");
        match first_meta {
            TodoistRow::Meta { value } => assert_eq!(value, "view_style=board"),
            _ => unreachable!(),
        }
    }

    #[test]
    fn fixture_first_section_is_sunday() {
        let rows = parse_csv(FIXTURE).unwrap();
        let first_section = rows
            .iter()
            .find(|r| matches!(r, TodoistRow::Section { .. }))
            .expect("section row exists");
        match first_section {
            TodoistRow::Section { title, .. } => {
                assert_eq!(title, "Sunday: Prep for the week");
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn fixture_quoted_field_preserves_embedded_comma() {
        // "Check for milk, add to list" — the comma inside quotes
        // must not split the field.
        let rows = parse_csv(FIXTURE).unwrap();
        let milk = rows
            .iter()
            .filter_map(|r| match r {
                TodoistRow::Task(t) => Some(t),
                _ => None,
            })
            .find(|t| t.content.starts_with("Check for milk"))
            .expect("milk task exists");
        assert_eq!(milk.content, "Check for milk, add to list");
    }

    #[test]
    fn fixture_indent_levels_present() {
        let rows = parse_csv(FIXTURE).unwrap();
        let mut depths = std::collections::HashSet::new();
        for r in &rows {
            if let TodoistRow::Task(t) = r {
                depths.insert(t.indent);
            }
        }
        // Fixture goes at least 2 deep.
        assert!(depths.contains(&1), "saw depths: {depths:?}");
        assert!(depths.contains(&2), "saw depths: {depths:?}");
    }

    #[test]
    fn fixture_priority_parsed_when_set() {
        // Most rows in the fixture have PRIORITY=4. Some have it
        // empty. Either case is valid; the important assertion is
        // that 4 parses cleanly.
        let rows = parse_csv(FIXTURE).unwrap();
        let mut saw_priority_4 = false;
        for r in &rows {
            if let TodoistRow::Task(t) = r
                && t.priority == Some(4)
            {
                saw_priority_4 = true;
                break;
            }
        }
        assert!(saw_priority_4, "expected priority-4 task");
    }

    #[test]
    fn fixture_inline_labels_preserved_in_content() {
        // The mapper strips @label tokens; the parser must NOT —
        // it preserves CONTENT verbatim. The mapper sees
        // "Check for essentials @chore @home" and decides what
        // to do.
        let rows = parse_csv(FIXTURE).unwrap();
        let labelled = rows
            .iter()
            .filter_map(|r| match r {
                TodoistRow::Task(t) => Some(t),
                _ => None,
            })
            .find(|t| t.content.contains("@chore"))
            .expect("expected an @chore task");
        assert!(labelled.content.contains("@chore"));
        assert!(labelled.content.contains("@home"));
    }

    #[test]
    fn rejects_header_mismatch() {
        let bad = "TYPE,CONTENT\nmeta,view_style=board\n";
        let err = parse_csv(bad).unwrap_err();
        assert!(matches!(err, ParseError::UnexpectedHeader { .. }));
    }

    #[test]
    fn rejects_unterminated_quote() {
        let bad = format!(
            "{}\ntask,\"never closed\n",
            "TYPE,CONTENT,DESCRIPTION,IS_COLLAPSED,PRIORITY,INDENT,AUTHOR,RESPONSIBLE,DATE,DATE_LANG,TIMEZONE,DURATION,DURATION_UNIT,DEADLINE,DEADLINE_LANG"
        );
        let err = parse_csv(&bad).unwrap_err();
        assert!(matches!(err, ParseError::UnterminatedQuote { .. }));
    }

    #[test]
    fn handles_utf8_bom() {
        let body = "\u{FEFF}TYPE,CONTENT,DESCRIPTION,IS_COLLAPSED,PRIORITY,INDENT,AUTHOR,RESPONSIBLE,DATE,DATE_LANG,TIMEZONE,DURATION,DURATION_UNIT,DEADLINE,DEADLINE_LANG\nmeta,view_style=board,,,,,,,,,,,,,\n";
        let rows = parse_csv(body).unwrap();
        assert!(matches!(rows[0], TodoistRow::Meta { .. }));
    }

    #[test]
    fn handles_escaped_quotes_inside_quoted_field() {
        let body = format!(
            "{}\ntask,\"He said \"\"hi\"\"\",,,,,,,,,,,,,\n",
            "TYPE,CONTENT,DESCRIPTION,IS_COLLAPSED,PRIORITY,INDENT,AUTHOR,RESPONSIBLE,DATE,DATE_LANG,TIMEZONE,DURATION,DURATION_UNIT,DEADLINE,DEADLINE_LANG"
        );
        let rows = parse_csv(&body).unwrap();
        match &rows[0] {
            TodoistRow::Task(t) => assert_eq!(t.content, "He said \"hi\""),
            other => panic!("expected Task, got {other:?}"),
        }
    }

    #[test]
    fn blank_separator_rows_recognised() {
        let body = format!(
            "{}\nmeta,k=v,,,,,,,,,,,,,\n,,,,,,,,,,,,,,\nsection,A,,False,,,,,,,,,,,\n",
            "TYPE,CONTENT,DESCRIPTION,IS_COLLAPSED,PRIORITY,INDENT,AUTHOR,RESPONSIBLE,DATE,DATE_LANG,TIMEZONE,DURATION,DURATION_UNIT,DEADLINE,DEADLINE_LANG"
        );
        let rows = parse_csv(&body).unwrap();
        assert!(matches!(rows[0], TodoistRow::Meta { .. }));
        assert!(matches!(rows[1], TodoistRow::Blank));
        assert!(matches!(rows[2], TodoistRow::Section { .. }));
    }
}
