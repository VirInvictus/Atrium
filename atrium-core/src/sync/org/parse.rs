// SPDX-License-Identifier: MIT
//! Hand-rolled Org-mode subset parser.
//!
//! Coverage matches spec §7.3:
//!
//! - Headlines: `*+ [KEYWORD ]title [:tag1:tag2:]`
//! - Keywords: TODO / DONE / CANCELLED. Custom keywords (e.g.
//!   `WAITING`) are preserved as `OrgKeyword::Custom(name)`.
//! - Cookies on the line below a headline: SCHEDULED, DEADLINE,
//!   CLOSED. Each is `<YYYY-MM-DD …>` (active) or `[YYYY-MM-DD …]`
//!   (inactive — used by CLOSED). Optional repeater suffix
//!   (`+1w`, `++1w`, `.+1w`) parsed into [`OrgRepeater`].
//! - `:PROPERTIES:` drawer with `:KEY: value` lines until `:END:`.
//! - Headline body: every non-headline line that isn't a cookie /
//!   property / drawer entry, captured verbatim.
//! - Subtasks: deeper headlines nest under their nearest shallower
//!   ancestor.
//!
//! "Preserve unknown constructs verbatim" is satisfied at two
//! layers:
//!
//! 1. **Within a task's body**, any unrecognised line (custom
//!    drawers, source blocks, tables, comments, etc.) is kept in
//!    the body text exactly as-read. Re-emitting writes the body
//!    back verbatim.
//! 2. **Properties** Atrium doesn't consume into typed fields
//!    (everything except :ID:, :CREATED:, :MODIFIED:, :EFFORT:,
//!    :DEFER_UNTIL:, :RRULE:, :SEQUENTIAL:, :REVIEW_INTERVAL:,
//!    :LAST_REVIEWED:, :ARCHIVED:, :ORIG_KEYWORD:) live in the
//!    [`OrgTask::properties`] HashMap and round-trip cleanly.
//!
//! Limitations the parser explicitly *doesn't* try to handle in
//! v0.7.7 (deferred to follow-up patches):
//!
//! - Multi-line property values.
//! - Active-timestamp time-of-day (we keep the date, drop the
//!   `HH:MM`). Atrium's `scheduled_for` is date-only.
//! - Multiple cookies on the same line — handled, but unusual
//!   layouts (cookies before keywords, etc.) aren't pattern-
//!   matched.
//! - File-level `#+TITLE:` and other affixes — tracked separately
//!   in v0.7.8 when the importer needs the project title.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

use chrono::{DateTime, NaiveDate, Utc};

/// TODO-cycle keyword on a headline. Spec §7.3.2 maps
/// open / done / cancelled to TODO / DONE / CANCELLED. Custom
/// keywords (e.g. `WAITING`, `IN-PROGRESS`) the user has in their
/// existing vault are preserved verbatim under `Custom`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrgKeyword {
    Todo,
    Done,
    Cancelled,
    Custom(String),
}

impl OrgKeyword {
    /// Render back to the canonical Org keyword string.
    pub fn as_str(&self) -> &str {
        match self {
            OrgKeyword::Todo => "TODO",
            OrgKeyword::Done => "DONE",
            OrgKeyword::Cancelled => "CANCELLED",
            OrgKeyword::Custom(s) => s.as_str(),
        }
    }
}

/// Repeater on a SCHEDULED or DEADLINE cookie. Org's three
/// modes (`+1w`, `++1w`, `.+1w`) round-trip per spec §7.3.3
/// rule 3. Stored as raw fields here; the converter to / from
/// [`atrium_core::repeat::RepeatRule`] lives in the importer
/// (v0.7.8+).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgRepeater {
    /// `+`, `++`, or `.+`.
    pub mode: String,
    /// Numeric interval (e.g. 1, 2, 14).
    pub interval: u32,
    /// Time unit: `d`, `w`, `m`, `y`.
    pub unit: char,
}

/// One headline + everything that belongs to it. The full
/// document parses into a flat top-level vec; subtasks (deeper
/// headlines) appear under [`children`].
#[derive(Debug, Clone, PartialEq)]
pub struct OrgTask {
    /// Heading depth — number of leading `*` on the line. Root
    /// headlines are depth 1; subtasks are `parent.depth + 1`.
    pub depth: usize,
    /// TODO-cycle keyword, if any. Headlines without a keyword
    /// (project sub-headings per spec §7.3.1) stay `None`.
    pub keyword: Option<OrgKeyword>,
    /// Headline text after stripping the leading stars,
    /// keyword, and trailing tags.
    pub title: String,
    /// Trailing `:tag1:tag2:` headline tags.
    pub tags: Vec<String>,
    /// `SCHEDULED:` cookie date.
    pub scheduled: Option<NaiveDate>,
    /// Repeater suffix on the SCHEDULED cookie.
    pub scheduled_repeater: Option<OrgRepeater>,
    /// `DEADLINE:` cookie date.
    pub deadline: Option<NaiveDate>,
    /// Repeater suffix on the DEADLINE cookie.
    pub deadline_repeater: Option<OrgRepeater>,
    /// `CLOSED:` cookie timestamp. Preserves the time-of-day if
    /// present; defaults to noon UTC if Org gave us only a date.
    pub closed: Option<DateTime<Utc>>,
    /// `:PROPERTIES:` drawer entries. Keys preserve case.
    pub properties: HashMap<String, String>,
    /// Headline body — everything between the cookies / properties
    /// and the next headline. Captured verbatim so things like
    /// source blocks, tables, custom drawers, etc. survive a
    /// round-trip. Does not include the trailing newline.
    pub body: String,
    /// Lines we couldn't pattern-match elsewhere. Currently used
    /// for the (rare) case of a malformed cookie or drawer line
    /// that we want to preserve without putting in body. Most
    /// "unknown constructs" land in body verbatim and don't
    /// reach this field.
    pub unknown_lines: Vec<String>,
    /// Nested subtasks (depth = self.depth + 1).
    pub children: Vec<OrgTask>,
}

impl OrgTask {
    fn new(depth: usize, title: String) -> Self {
        Self {
            depth,
            keyword: None,
            title,
            tags: Vec::new(),
            scheduled: None,
            scheduled_repeater: None,
            deadline: None,
            deadline_repeater: None,
            closed: None,
            properties: HashMap::new(),
            body: String::new(),
            unknown_lines: Vec::new(),
            children: Vec::new(),
        }
    }
}

/// Parse a `.org` file from disk. Returns the top-level headlines
/// (project sub-headings or root tasks) with subtasks nested.
pub fn parse_org_file(path: &Path) -> io::Result<Vec<OrgTask>> {
    let text = fs::read_to_string(path)?;
    Ok(parse_org_text(&text))
}

/// Parse Org text directly. Useful in tests and any in-memory
/// flow (e.g. atrium-cli reading from stdin).
pub fn parse_org_text(text: &str) -> Vec<OrgTask> {
    let mut flat: Vec<OrgTask> = Vec::new();
    let mut current: Option<OrgTask> = None;
    let mut in_properties = false;

    for raw_line in text.lines() {
        // Detect a headline first — it terminates the current
        // task's body and starts a new one.
        if let Some((depth, keyword, title, tags)) = parse_headline(raw_line) {
            if let Some(task) = current.take() {
                flat.push(task);
            }
            let mut task = OrgTask::new(depth, title);
            task.keyword = keyword;
            task.tags = tags;
            current = Some(task);
            in_properties = false;
            continue;
        }

        let Some(task) = current.as_mut() else {
            // Anything before the first headline is file-level
            // preamble (#+TITLE:, #+CATEGORY:, etc.). v0.7.7
            // discards it; v0.7.8 will capture it for the
            // file-level project metadata.
            continue;
        };

        // :PROPERTIES: drawer state machine.
        if in_properties {
            if raw_line.trim_end().eq_ignore_ascii_case(":END:") {
                in_properties = false;
                continue;
            }
            if let Some((key, value)) = parse_property_line(raw_line) {
                task.properties.insert(key, value);
            } else {
                // Garbage inside a properties drawer — preserve
                // verbatim so we can round-trip even malformed
                // upstream files.
                task.unknown_lines.push(raw_line.to_string());
            }
            continue;
        }

        if raw_line.trim_end().eq_ignore_ascii_case(":PROPERTIES:") {
            in_properties = true;
            continue;
        }

        // Cookie line — SCHEDULED / DEADLINE / CLOSED. The same
        // line can carry multiple cookies in Org, so we scan
        // them all rather than dispatching on the first match.
        if extract_cookies(raw_line, task) {
            continue;
        }

        // Otherwise the line is body content; preserve verbatim.
        if !task.body.is_empty() {
            task.body.push('\n');
        }
        task.body.push_str(raw_line);
    }

    if let Some(task) = current.take() {
        flat.push(task);
    }

    // Normalise body: trim trailing blank lines so we don't
    // accumulate empty separators between tasks. This is
    // round-trip-safe because the emitter (v0.7.8) is
    // responsible for re-inserting whatever spacing it wants.
    for task in &mut flat {
        while task.body.ends_with('\n') {
            task.body.pop();
        }
    }

    nest_by_depth(flat)
}

/// Re-organise a flat list of tasks into a tree by depth. A task
/// of depth `d` becomes a child of the most recent ancestor with
/// depth `< d`. Tasks of depth 1 stay at the top level.
fn nest_by_depth(flat: Vec<OrgTask>) -> Vec<OrgTask> {
    let mut top: Vec<OrgTask> = Vec::new();
    // Stack of indices into the path from top → current. Each
    // entry is a raw pointer-style index path (we re-walk the
    // tree to find the entry rather than holding mutable
    // references, which the borrow checker won't allow).
    let mut path: Vec<usize> = Vec::new();

    for task in flat {
        // Pop the path until the top matches a depth less than
        // the incoming task.
        while let Some(&_idx) = path.last() {
            let depth_of_top_of_path = depth_at(&top, &path);
            if depth_of_top_of_path < task.depth {
                break;
            }
            path.pop();
        }

        if path.is_empty() {
            top.push(task);
            path.push(top.len() - 1);
        } else {
            let parent = walk_mut(&mut top, &path);
            parent.children.push(task);
            let new_idx = parent.children.len() - 1;
            path.push(new_idx);
        }
    }

    top
}

fn depth_at(top: &[OrgTask], path: &[usize]) -> usize {
    let mut nodes = top;
    let mut last_depth = 0;
    for (i, &idx) in path.iter().enumerate() {
        if i + 1 == path.len() {
            return nodes[idx].depth;
        }
        last_depth = nodes[idx].depth;
        nodes = &nodes[idx].children;
    }
    last_depth
}

fn walk_mut<'a>(top: &'a mut Vec<OrgTask>, path: &[usize]) -> &'a mut OrgTask {
    let mut nodes = top;
    for (i, &idx) in path.iter().enumerate() {
        if i + 1 == path.len() {
            return &mut nodes[idx];
        }
        nodes = &mut nodes[idx].children;
    }
    unreachable!("walk_mut called with empty path");
}

/// Try to recognise a headline. Pattern:
/// `^(\*+) (?:KEYWORD )?title (?:\s+:tag1:tag2:)?$`
///
/// Returns `(depth, keyword, title, tags)` on match.
fn parse_headline(line: &str) -> Option<(usize, Option<OrgKeyword>, String, Vec<String>)> {
    if !line.starts_with('*') {
        return None;
    }
    let stars_end = line.bytes().take_while(|b| *b == b'*').count();
    // A headline requires a space after the stars; otherwise it's
    // a list bullet or some other construct.
    let rest = line.get(stars_end..)?;
    if !rest.starts_with(' ') {
        return None;
    }
    let body = rest[1..].trim_end();
    if body.is_empty() {
        return Some((stars_end, None, String::new(), Vec::new()));
    }

    // Split off trailing tags `:foo:bar:`. The pattern requires
    // the tag chunk to be at the very end of the line, preceded
    // by at least one whitespace char.
    let (title_with_keyword, tags) = strip_trailing_tags(body);

    // First word might be a TODO-cycle keyword.
    let (keyword, title) = match title_with_keyword.split_once(' ') {
        Some((first, rest)) if is_todo_keyword(first) => (Some(parse_keyword(first)), rest.into()),
        _ if is_todo_keyword(&title_with_keyword) => {
            // Bare keyword headline ("* TODO" with no title)
            (Some(parse_keyword(&title_with_keyword)), String::new())
        }
        _ => (None, title_with_keyword),
    };

    Some((stars_end, keyword, title.trim().to_string(), tags))
}

fn is_todo_keyword(word: &str) -> bool {
    // Org's default keyword sets are user-configurable, but the
    // spec pins three states: TODO / DONE / CANCELLED. Custom
    // keywords are detected heuristically: ALL_CAPS, no spaces,
    // no punctuation. Conservative — false positives turn into
    // odd "Custom(x)" detections, which the importer surfaces;
    // false negatives mean a real keyword reads as part of the
    // title (caller can see the leading-uppercase word and
    // file a fix).
    if word.is_empty() {
        return false;
    }
    matches!(word, "TODO" | "DONE" | "CANCELLED")
        || (word
            .chars()
            .all(|c| c.is_ascii_uppercase() || c == '-' || c == '_')
            && word.chars().any(|c| c.is_ascii_uppercase()))
}

fn parse_keyword(word: &str) -> OrgKeyword {
    match word {
        "TODO" => OrgKeyword::Todo,
        "DONE" => OrgKeyword::Done,
        "CANCELLED" => OrgKeyword::Cancelled,
        other => OrgKeyword::Custom(other.to_string()),
    }
}

/// Strip trailing `:tag1:tag2:` from a headline body.
fn strip_trailing_tags(body: &str) -> (String, Vec<String>) {
    // Look for the last whitespace-delimited chunk. If it matches
    // the tag pattern, peel it off.
    let trimmed = body.trim_end();
    if !trimmed.ends_with(':') {
        return (trimmed.to_string(), Vec::new());
    }

    // Find the rightmost whitespace.
    let split_at = trimmed.rfind(char::is_whitespace);
    let Some(split_at) = split_at else {
        // No whitespace — the entire body could be a tag chunk
        // (`:foo:bar:` with no preceding title). Treat as
        // tag-only headline only if the chunk validates as tags.
        if let Some(tags) = parse_tag_chunk(trimmed) {
            return (String::new(), tags);
        }
        return (trimmed.to_string(), Vec::new());
    };

    let (title_part, tag_part) = trimmed.split_at(split_at);
    let tag_part = tag_part.trim_start();
    if let Some(tags) = parse_tag_chunk(tag_part) {
        (title_part.trim_end().to_string(), tags)
    } else {
        (trimmed.to_string(), Vec::new())
    }
}

/// Parse a chunk like `:tag1:tag2:tag3:` into a Vec of tag
/// strings. Returns `None` if the chunk doesn't look like a
/// canonical Org tag block (so the caller leaves the chunk in
/// the title).
fn parse_tag_chunk(chunk: &str) -> Option<Vec<String>> {
    if !chunk.starts_with(':') || !chunk.ends_with(':') || chunk.len() < 2 {
        return None;
    }
    let inner = &chunk[1..chunk.len() - 1];
    if inner.is_empty() {
        return None;
    }
    let parts: Vec<String> = inner.split(':').map(|s| s.to_string()).collect();
    if parts.iter().any(|p| p.is_empty()) {
        return None; // adjacent colons → not a valid tag block
    }
    if parts.iter().any(|p| p.contains(char::is_whitespace)) {
        return None; // whitespace inside a tag → not valid
    }
    Some(parts)
}

/// Match `:KEY: value` lines inside a `:PROPERTIES:` drawer.
fn parse_property_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with(':') {
        return None;
    }
    // Find the second `:` that closes the key.
    let after_open = &trimmed[1..];
    let close = after_open.find(':')?;
    let key = &after_open[..close];
    if key.is_empty() {
        return None;
    }
    let value = after_open[close + 1..].trim();
    Some((key.to_string(), value.to_string()))
}

/// Scan a line for SCHEDULED / DEADLINE / CLOSED cookies, mutating
/// `task` for each match found. Returns true when the line was
/// consumed (one or more cookies extracted) — the caller should
/// not also append it to the body.
fn extract_cookies(line: &str, task: &mut OrgTask) -> bool {
    let mut found_any = false;

    if let Some(rest) = line.find("SCHEDULED:")
        && let Some((date, repeater)) = parse_active_timestamp(&line[rest + "SCHEDULED:".len()..])
    {
        task.scheduled = Some(date);
        task.scheduled_repeater = repeater;
        found_any = true;
    }
    if let Some(rest) = line.find("DEADLINE:")
        && let Some((date, repeater)) = parse_active_timestamp(&line[rest + "DEADLINE:".len()..])
    {
        task.deadline = Some(date);
        task.deadline_repeater = repeater;
        found_any = true;
    }
    if let Some(rest) = line.find("CLOSED:")
        && let Some(closed) = parse_inactive_timestamp(&line[rest + "CLOSED:".len()..])
    {
        task.closed = Some(closed);
        found_any = true;
    }

    found_any
}

/// Parse an active timestamp `<YYYY-MM-DD ...>` returning the
/// date and any trailing repeater (`+1w`, `++1w`, `.+1w`).
fn parse_active_timestamp(text: &str) -> Option<(NaiveDate, Option<OrgRepeater>)> {
    let start = text.find('<')?;
    let end = text[start..].find('>')? + start;
    let inner = &text[start + 1..end];
    parse_timestamp_inner(inner)
}

/// Parse an inactive timestamp `[YYYY-MM-DD ...]` returning a
/// UTC datetime. CLOSED uses inactive timestamps in Org. If only
/// a date is present we use noon UTC (arbitrary but stable).
fn parse_inactive_timestamp(text: &str) -> Option<DateTime<Utc>> {
    let start = text.find('[')?;
    let end = text[start..].find(']')? + start;
    let inner = &text[start + 1..end];
    let (date, _repeater) = parse_timestamp_inner(inner)?;

    // Look for a `HH:MM` after the date.
    let parts: Vec<&str> = inner.split_whitespace().collect();
    let time = parts.iter().find_map(|p| {
        let mut split = p.split(':');
        let h: u32 = split.next()?.parse().ok()?;
        let m: u32 = split.next()?.parse().ok()?;
        if split.next().is_some() {
            return None;
        }
        chrono::NaiveTime::from_hms_opt(h, m, 0)
    });
    let dt = match time {
        Some(t) => date.and_time(t),
        None => date.and_hms_opt(12, 0, 0)?,
    };
    Some(dt.and_utc())
}

fn parse_timestamp_inner(inner: &str) -> Option<(NaiveDate, Option<OrgRepeater>)> {
    let mut parts = inner.split_whitespace();
    let date_part = parts.next()?;
    let date = NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()?;

    let repeater = parts.find_map(parse_repeater);
    Some((date, repeater))
}

fn parse_repeater(token: &str) -> Option<OrgRepeater> {
    // Token shape: `+1w`, `++1w`, `.+1w`. The mode is one of `+`,
    // `++`, `.+`; then digits; then a unit char.
    let (mode, rest) = if let Some(rest) = token.strip_prefix("++") {
        ("++", rest)
    } else if let Some(rest) = token.strip_prefix(".+") {
        (".+", rest)
    } else if let Some(rest) = token.strip_prefix('+') {
        ("+", rest)
    } else {
        return None;
    };
    if rest.is_empty() {
        return None;
    }
    let unit_pos = rest.find(|c: char| c.is_ascii_alphabetic())?;
    let interval: u32 = rest[..unit_pos].parse().ok()?;
    let unit = rest[unit_pos..].chars().next()?;
    if !matches!(unit, 'd' | 'w' | 'm' | 'y') {
        return None;
    }
    Some(OrgRepeater {
        mode: mode.to_string(),
        interval,
        unit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn parses_simple_todo() {
        let input = "* TODO Email João\n";
        let tasks = parse_org_text(input);
        assert_eq!(tasks.len(), 1);
        let t = &tasks[0];
        assert_eq!(t.depth, 1);
        assert_eq!(t.keyword, Some(OrgKeyword::Todo));
        assert_eq!(t.title, "Email João");
        assert_eq!(t.scheduled, None);
        assert_eq!(t.deadline, None);
        assert!(t.tags.is_empty());
        assert!(t.body.is_empty());
        assert!(t.children.is_empty());
    }

    #[test]
    fn parses_done_with_closed_cookie() {
        let input = "\
* DONE Audit
CLOSED: [2026-05-01 Fri 14:30]
";
        let tasks = parse_org_text(input);
        assert_eq!(tasks.len(), 1);
        let t = &tasks[0];
        assert_eq!(t.keyword, Some(OrgKeyword::Done));
        let closed = t.closed.unwrap();
        assert_eq!(closed.date_naive(), d(2026, 5, 1));
        assert_eq!(closed.time().to_string(), "14:30:00");
    }

    #[test]
    fn parses_cancelled() {
        let input = "* CANCELLED Old idea\n";
        let tasks = parse_org_text(input);
        assert_eq!(tasks[0].keyword, Some(OrgKeyword::Cancelled));
    }

    #[test]
    fn parses_scheduled_and_deadline() {
        let input = "\
* TODO Plan Q3
SCHEDULED: <2026-05-15 Fri> DEADLINE: <2026-06-01 Mon>
";
        let tasks = parse_org_text(input);
        let t = &tasks[0];
        assert_eq!(t.scheduled, Some(d(2026, 5, 15)));
        assert_eq!(t.deadline, Some(d(2026, 6, 1)));
    }

    #[test]
    fn parses_repeater_on_scheduled() {
        let input = "\
* TODO Weekly review
SCHEDULED: <2026-05-15 Fri ++1w>
";
        let tasks = parse_org_text(input);
        let t = &tasks[0];
        assert_eq!(t.scheduled, Some(d(2026, 5, 15)));
        let rep = t.scheduled_repeater.as_ref().unwrap();
        assert_eq!(rep.mode, "++");
        assert_eq!(rep.interval, 1);
        assert_eq!(rep.unit, 'w');
    }

    #[test]
    fn parses_headline_tags() {
        let input = "* TODO Run errands :errand:home:\n";
        let tasks = parse_org_text(input);
        let t = &tasks[0];
        assert_eq!(t.title, "Run errands");
        assert_eq!(t.tags, vec!["errand", "home"]);
    }

    #[test]
    fn ignores_invalid_tag_chunks() {
        // ":foo bar:" has whitespace inside tags → not a valid
        // tag chunk. Should stay in title.
        let input = "* TODO Some task :foo bar:\n";
        let tasks = parse_org_text(input);
        assert_eq!(tasks[0].title, "Some task :foo bar:");
        assert!(tasks[0].tags.is_empty());
    }

    #[test]
    fn parses_properties_drawer() {
        let input = "\
* TODO Q3 Roadmap
:PROPERTIES:
:ID: 9c2f9c0e-1a1b-44e2-9f9c-0e1a1b44e29f
:CREATED: [2026-04-01 Wed]
:EFFORT: 0:30
:CUSTOM:   verbatim value
:END:
";
        let tasks = parse_org_text(input);
        let t = &tasks[0];
        assert_eq!(
            t.properties.get("ID").map(String::as_str),
            Some("9c2f9c0e-1a1b-44e2-9f9c-0e1a1b44e29f")
        );
        assert_eq!(
            t.properties.get("CREATED").map(String::as_str),
            Some("[2026-04-01 Wed]")
        );
        assert_eq!(t.properties.get("EFFORT").map(String::as_str), Some("0:30"));
        assert_eq!(
            t.properties.get("CUSTOM").map(String::as_str),
            Some("verbatim value")
        );
    }

    #[test]
    fn captures_body_verbatim() {
        let input = "\
* TODO Brainstorm
Some prose body.

  - bullet 1
  - bullet 2

#+BEGIN_SRC rust
fn foo() {}
#+END_SRC
";
        let tasks = parse_org_text(input);
        let t = &tasks[0];
        assert!(t.body.contains("Some prose body."));
        assert!(t.body.contains("- bullet 1"));
        assert!(t.body.contains("#+BEGIN_SRC rust"));
        assert!(t.body.contains("fn foo() {}"));
        assert!(t.body.contains("#+END_SRC"));
    }

    #[test]
    fn nests_subtasks_by_depth() {
        let input = "\
* TODO Parent
** TODO Child A
** TODO Child B
*** TODO Grandchild
* TODO Sibling
";
        let tasks = parse_org_text(input);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].title, "Parent");
        assert_eq!(tasks[0].children.len(), 2);
        assert_eq!(tasks[0].children[0].title, "Child A");
        assert_eq!(tasks[0].children[1].title, "Child B");
        assert_eq!(tasks[0].children[1].children.len(), 1);
        assert_eq!(tasks[0].children[1].children[0].title, "Grandchild");
        assert_eq!(tasks[1].title, "Sibling");
    }

    #[test]
    fn project_subheading_no_keyword() {
        // Headlines without a TODO keyword are project sub-
        // headings per spec §7.3.1.
        let input = "* Backlog\n** TODO Real task\n";
        let tasks = parse_org_text(input);
        assert_eq!(tasks[0].keyword, None);
        assert_eq!(tasks[0].title, "Backlog");
        assert_eq!(tasks[0].children.len(), 1);
        assert_eq!(tasks[0].children[0].keyword, Some(OrgKeyword::Todo));
    }

    #[test]
    fn custom_keyword_preserved() {
        let input = "* WAITING External signoff\n";
        let tasks = parse_org_text(input);
        assert_eq!(
            tasks[0].keyword,
            Some(OrgKeyword::Custom("WAITING".to_string()))
        );
        assert_eq!(tasks[0].title, "External signoff");
    }

    #[test]
    fn ignores_preamble_before_first_headline() {
        let input = "\
#+TITLE: Q3 Plans
#+CATEGORY: work

* TODO Real headline
";
        let tasks = parse_org_text(input);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Real headline");
    }

    #[test]
    fn preserves_unknown_lines_inside_properties() {
        // Garbage line inside a :PROPERTIES: drawer should be
        // captured into unknown_lines so a round-trip can preserve
        // it. Real-world Org files occasionally have these.
        let input = "\
* TODO Thing
:PROPERTIES:
:ID: abc
this is not a property line
:END:
";
        let tasks = parse_org_text(input);
        let t = &tasks[0];
        assert_eq!(t.properties.get("ID").map(String::as_str), Some("abc"));
        assert_eq!(t.unknown_lines.len(), 1);
        assert_eq!(t.unknown_lines[0], "this is not a property line");
    }

    #[test]
    fn body_does_not_include_cookie_lines() {
        let input = "\
* TODO Plan
SCHEDULED: <2026-05-15 Fri>
Body line below cookies.
";
        let tasks = parse_org_text(input);
        let t = &tasks[0];
        assert_eq!(t.scheduled, Some(d(2026, 5, 15)));
        assert_eq!(t.body, "Body line below cookies.");
    }

    #[test]
    fn empty_input_yields_no_tasks() {
        assert!(parse_org_text("").is_empty());
        assert!(parse_org_text("\n\n").is_empty());
        assert!(parse_org_text("#+TITLE: empty file\n").is_empty());
    }

    #[test]
    fn keyword_token_classifier_basics() {
        assert!(is_todo_keyword("TODO"));
        assert!(is_todo_keyword("DONE"));
        assert!(is_todo_keyword("CANCELLED"));
        assert!(is_todo_keyword("WAITING")); // custom
        assert!(is_todo_keyword("IN-PROGRESS")); // custom with hyphen
        assert!(!is_todo_keyword("Hello")); // mixed case
        assert!(!is_todo_keyword(""));
        assert!(!is_todo_keyword("123"));
    }

    #[test]
    fn repeater_parses_three_modes() {
        let r = parse_repeater("+1w").unwrap();
        assert_eq!(r.mode, "+");
        assert_eq!(r.interval, 1);
        assert_eq!(r.unit, 'w');

        let r = parse_repeater("++2d").unwrap();
        assert_eq!(r.mode, "++");
        assert_eq!(r.interval, 2);

        let r = parse_repeater(".+14d").unwrap();
        assert_eq!(r.mode, ".+");
        assert_eq!(r.interval, 14);

        assert!(parse_repeater("just text").is_none());
        assert!(parse_repeater("+").is_none());
        assert!(parse_repeater("+1z").is_none()); // bad unit
    }
}
