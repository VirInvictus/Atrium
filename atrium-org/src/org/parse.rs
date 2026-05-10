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
//! Known limits:
//!
//! - **Property drawer values are single-line.** Multi-line
//!   `:KEY: ...` continuations aren't recognised. None of
//!   Atrium's modeled properties need them.
//! - **Active timestamps lose time-of-day.** `<2026-05-15 Fri 14:00>`
//!   parses as the date `2026-05-15`. Atrium's `scheduled_for` is
//!   date-only by design.
//! - **Headline layout is rigid.** Stars, keyword, title, tags,
//!   in that order. Cookies-before-keyword and other unusual
//!   shapes aren't pattern-matched.

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

/// v0.15.0 — Phase 18.5 Tier-1 statistics cookie on a parent
/// headline. Two shapes per Org spec: `[done/total]` and `[N%]`.
/// The variant is preserved verbatim across a round-trip so the
/// emitter writes back the form the user wrote — Atrium's
/// projection happens to always *compute* the cookie when it
/// emits a parent, but the *shape* (fraction vs percent) is the
/// user's call. When Atrium synthesises a cookie for a parent
/// that didn't carry one on read, it defaults to `Counter` (the
/// fraction form) — that's what the overwhelming majority of
/// "how I org" tutorials use.
///
/// Empty shapes (`[/]`, `[%]`) parse as zero values; the next
/// emit overwrites them with the freshly-computed counts. The
/// shape preservation is the variant choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatisticsCookie {
    Counter { done: u32, total: u32 },
    Percent { value: u8 },
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
    /// Warning suffix on the SCHEDULED cookie (`-Nd` / `--Nd`).
    /// v0.14.0 — Org allows a warning period on SCHEDULED, though
    /// it's rare. Atrium has no DB column for it (the spec only
    /// models the deadline-side warning), so this field exists
    /// purely for verbatim round-trip — the emitter writes it back
    /// in the same shape we read it.
    pub scheduled_warning: Option<u32>,
    /// `DEADLINE:` cookie date.
    pub deadline: Option<NaiveDate>,
    /// Repeater suffix on the DEADLINE cookie.
    pub deadline_repeater: Option<OrgRepeater>,
    /// Warning suffix on the DEADLINE cookie (`-Nd` / `--Nd`).
    /// v0.14.0 (Phase 18.5 Tier-1) — projected to / from
    /// `Task.deadline_warn_days`. Org distinguishes `-` (per-task
    /// warning) from `--` (override of the global default), but
    /// Atrium has no global-default-override concept so both forms
    /// parse to the same value and the emitter normalises onto `-`.
    pub deadline_warning: Option<u32>,
    /// v0.15.0 — Phase 18.5 Tier-1 statistics cookie on a parent
    /// headline (`[2/5]` or `[40%]`). Captured from the source on
    /// read, stripped from the title, re-emitted on write. The
    /// variant preserves the user's chosen shape across the
    /// round-trip; Atrium recomputes the values from DB state on
    /// every emit so a stale cookie self-heals on the next flush.
    pub statistics_cookie: Option<StatisticsCookie>,
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
    /// v0.15.0 — minimal builder for tests in sibling crates.
    /// Produces an empty task at `depth` with everything else
    /// defaulted; tests fill in the fields they care about
    /// rather than carrying every literal field forward whenever
    /// OrgTask gains a new column.
    #[doc(hidden)]
    pub fn default_test_node(depth: usize) -> Self {
        Self::new(depth, String::new())
    }

    fn new(depth: usize, title: String) -> Self {
        Self {
            depth,
            keyword: None,
            title,
            tags: Vec::new(),
            scheduled: None,
            scheduled_repeater: None,
            scheduled_warning: None,
            deadline: None,
            deadline_repeater: None,
            deadline_warning: None,
            statistics_cookie: None,
            closed: None,
            properties: HashMap::new(),
            body: String::new(),
            unknown_lines: Vec::new(),
            children: Vec::new(),
        }
    }
}

/// file-level Org metadata captured alongside the
/// headline tree. `directives` carries `#+TITLE:`, `#+CATEGORY:`,
/// etc. as case-insensitive keys (uppercased). `file_properties`
/// carries the entries of any top-level `:PROPERTIES: ... :END:`
/// block that appeared before the first headline. Both are empty
/// for the common case (a file that opens with a headline).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct OrgFile {
    pub directives: std::collections::HashMap<String, String>,
    pub file_properties: HashMap<String, String>,
    pub headlines: Vec<OrgTask>,
}

/// Parse a `.org` file from disk. Returns the top-level headlines
/// (project sub-headings or root tasks) with subtasks nested.
///
/// Drops file-level preamble — use [`parse_org_file_with_meta`]
/// when the caller needs `#+TITLE:` or file-level properties.
pub fn parse_org_file(path: &Path) -> io::Result<Vec<OrgTask>> {
    Ok(parse_org_file_with_meta(path)?.headlines)
}

/// Phase 16 entry point that captures file-level metadata
/// alongside the headline tree.
pub fn parse_org_file_with_meta(path: &Path) -> io::Result<OrgFile> {
    let text = fs::read_to_string(path)?;
    Ok(parse_org_text_with_meta(&text))
}

/// Parse Org text and return the headline tree only. Drops
/// file-level preamble for backwards compatibility — use
/// [`parse_org_text_with_meta`] when the caller needs `#+TITLE:`
/// or file-level properties.
pub fn parse_org_text(text: &str) -> Vec<OrgTask> {
    parse_org_text_with_meta(text).headlines
}

/// Parse Org text directly, returning headlines + file-level
/// directives + file-level properties.
pub fn parse_org_text_with_meta(text: &str) -> OrgFile {
    let mut directives: HashMap<String, String> = HashMap::new();
    let mut file_properties: HashMap<String, String> = HashMap::new();
    let mut flat: Vec<OrgTask> = Vec::new();
    let mut current: Option<OrgTask> = None;
    let mut in_properties = false;
    /// Where a `:PROPERTIES:` block belongs while the parser is
    /// inside one.
    enum PropsTarget {
        File,
        Headline,
    }
    let mut props_target = PropsTarget::Headline;

    for raw_line in text.lines() {
        // Detect a headline first — it terminates the current
        // task's body and starts a new one.
        if let Some((depth, keyword, title, cookie, tags)) = parse_headline(raw_line) {
            if let Some(task) = current.take() {
                flat.push(task);
            }
            let mut task = OrgTask::new(depth, title);
            task.keyword = keyword;
            task.tags = tags;
            task.statistics_cookie = cookie;
            current = Some(task);
            in_properties = false;
            continue;
        }

        // :PROPERTIES: drawer state machine — runs whether or not
        // we've seen a headline yet (a top-level drawer carries
        // file-level project metadata in v0.7.13).
        if in_properties {
            if raw_line.trim_end().eq_ignore_ascii_case(":END:") {
                in_properties = false;
                continue;
            }
            if let Some((key, value)) = parse_property_line(raw_line) {
                match props_target {
                    PropsTarget::File => {
                        file_properties.insert(key, value);
                    }
                    PropsTarget::Headline => {
                        if let Some(task) = current.as_mut() {
                            task.properties.insert(key, value);
                        }
                    }
                }
            } else if let Some(task) = current.as_mut() {
                // Garbage inside a headline-attached properties
                // drawer — preserve verbatim so we can round-trip
                // even malformed upstream files.
                task.unknown_lines.push(raw_line.to_string());
            }
            // Stray garbage in a file-level drawer is dropped (rare;
            // we don't have an unknown_lines collector at the file
            // level yet).
            continue;
        }

        if raw_line.trim_end().eq_ignore_ascii_case(":PROPERTIES:") {
            in_properties = true;
            props_target = if current.is_some() {
                PropsTarget::Headline
            } else {
                PropsTarget::File
            };
            continue;
        }

        let Some(task) = current.as_mut() else {
            // No headline yet — this is file-level preamble.
            // Capture #+DIRECTIVE: VALUE lines. Other lines
            // (blank, comments, etc.) are dropped silently;
            // round-tripping the entire preamble verbatim is a
            // future polish item.
            if let Some((key, value)) = parse_directive(raw_line) {
                directives.insert(key, value);
            }
            continue;
        };

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

    OrgFile {
        directives,
        file_properties,
        headlines: nest_by_depth(flat),
    }
}

/// Parse a `#+KEY: value` directive line. Returns
/// `(key_uppercased, value_trimmed)` on match. Case-insensitive
/// — `#+title:` and `#+TITLE:` both produce the key `"TITLE"`.
fn parse_directive(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim_start();
    let after_pound = trimmed.strip_prefix("#+")?;
    let colon = after_pound.find(':')?;
    let key = after_pound[..colon].to_uppercase();
    if key.is_empty() {
        return None;
    }
    let value = after_pound[colon + 1..].trim().to_string();
    Some((key, value))
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
/// `^(\*+) (?:KEYWORD )?title (?:\s+\[N/M|N%\])?(?:\s+:tag1:tag2:)?$`
///
/// Returns `(depth, keyword, title, cookie, tags)` on match.
/// v0.15.0 added the cookie return — the trailing `[done/total]`
/// or `[N%]` statistics cookie on parent headlines, stripped
/// from the title text and surfaced separately.
#[allow(clippy::type_complexity)]
fn parse_headline(
    line: &str,
) -> Option<(
    usize,
    Option<OrgKeyword>,
    String,
    Option<StatisticsCookie>,
    Vec<String>,
)> {
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
        return Some((stars_end, None, String::new(), None, Vec::new()));
    }

    // Split off trailing tags `:foo:bar:`. The pattern requires
    // the tag chunk to be at the very end of the line, preceded
    // by at least one whitespace char.
    let (title_with_keyword_and_cookie, tags) = strip_trailing_tags(body);

    // Then split off the trailing statistics cookie if present.
    // Cookie sits between title and tags in canonical Org
    // headlines: `* TODO Project [3/5] :work:`.
    let (title_with_keyword, cookie) = strip_trailing_cookie(&title_with_keyword_and_cookie);

    // First word might be a TODO-cycle keyword.
    let (keyword, title) = match title_with_keyword.split_once(' ') {
        Some((first, rest)) if is_todo_keyword(first) => (Some(parse_keyword(first)), rest.into()),
        _ if is_todo_keyword(&title_with_keyword) => {
            // Bare keyword headline ("* TODO" with no title)
            (Some(parse_keyword(&title_with_keyword)), String::new())
        }
        _ => (None, title_with_keyword),
    };

    Some((stars_end, keyword, title.trim().to_string(), cookie, tags))
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

/// v0.15.0 — strip a trailing statistics cookie (`[N/M]` or
/// `[N%]`) from a headline body. The cookie pattern matches only
/// when it sits at the very end of the (already tag-stripped)
/// body, separated by at least one whitespace char from the
/// title. Empty shapes (`[/]`, `[%]`) parse with zero values —
/// the next emit recomputes from DB state.
fn strip_trailing_cookie(body: &str) -> (String, Option<StatisticsCookie>) {
    let trimmed = body.trim_end();
    if !trimmed.ends_with(']') {
        return (trimmed.to_string(), None);
    }
    // Find the matching `[`. Cookies don't nest, so the last
    // unmatched `[` is the start.
    let bracket_open = trimmed.rfind('[');
    let Some(open_idx) = bracket_open else {
        return (trimmed.to_string(), None);
    };
    // Cookie must be preceded by whitespace (so we don't snip
    // user-content brackets at title start, e.g. "* TODO [draft]
    // ...").
    if open_idx == 0 {
        return (trimmed.to_string(), None);
    }
    let preceding = &trimmed[..open_idx];
    if !preceding.ends_with(char::is_whitespace) {
        return (trimmed.to_string(), None);
    }
    let inner = &trimmed[open_idx + 1..trimmed.len() - 1];
    let cookie = parse_cookie_inner(inner);
    if cookie.is_none() {
        return (trimmed.to_string(), None);
    }
    (preceding.trim_end().to_string(), cookie)
}

/// Parse the contents of a cookie: `2/5`, `40%`, `/`, `%`, or
/// empty fraction/percent. Returns `None` for shapes that don't
/// match — those stay in the title verbatim.
fn parse_cookie_inner(inner: &str) -> Option<StatisticsCookie> {
    let inner = inner.trim();
    if let Some(rest) = inner.strip_suffix('%') {
        // Percent form. `[%]` (empty) or `[N%]`.
        if rest.is_empty() {
            return Some(StatisticsCookie::Percent { value: 0 });
        }
        let value: u8 = rest.parse().ok()?;
        if value > 100 {
            return None;
        }
        return Some(StatisticsCookie::Percent { value });
    }
    if let Some((done_part, total_part)) = inner.split_once('/') {
        // Fraction form. `[/]` (both empty) or `[N/M]`.
        let done = if done_part.is_empty() {
            0
        } else {
            done_part.parse().ok()?
        };
        let total = if total_part.is_empty() {
            0
        } else {
            total_part.parse().ok()?
        };
        return Some(StatisticsCookie::Counter { done, total });
    }
    None
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
        && let Some((date, repeater, warning)) =
            parse_active_timestamp(&line[rest + "SCHEDULED:".len()..])
    {
        task.scheduled = Some(date);
        task.scheduled_repeater = repeater;
        task.scheduled_warning = warning;
        found_any = true;
    }
    if let Some(rest) = line.find("DEADLINE:")
        && let Some((date, repeater, warning)) =
            parse_active_timestamp(&line[rest + "DEADLINE:".len()..])
    {
        task.deadline = Some(date);
        task.deadline_repeater = repeater;
        task.deadline_warning = warning;
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
/// date and any trailing repeater (`+1w`, `++1w`, `.+1w`) and
/// warning suffix (`-Nd`, `--Nd`). Per Org docs, repeater and
/// warning may appear in either order — both are recognised
/// independently of position.
fn parse_active_timestamp(text: &str) -> Option<(NaiveDate, Option<OrgRepeater>, Option<u32>)> {
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
    let (date, _repeater, _warning) = parse_timestamp_inner(inner)?;

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

fn parse_timestamp_inner(inner: &str) -> Option<(NaiveDate, Option<OrgRepeater>, Option<u32>)> {
    let mut parts = inner.split_whitespace().peekable();
    let date_part = parts.next()?;
    let date = NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()?;

    // Org allows the repeater and warning suffixes to appear in
    // either order. Walk the remaining tokens once and pick out
    // the first that matches each shape — they're disjoint
    // (`+`/`++`/`.+` vs. `-`/`--`) so a token can only ever match
    // one or the other.
    let mut repeater = None;
    let mut warning = None;
    for token in parts {
        if repeater.is_none()
            && let Some(r) = parse_repeater(token)
        {
            repeater = Some(r);
        } else if warning.is_none()
            && let Some(w) = parse_warning(token)
        {
            warning = Some(w);
        }
    }
    Some((date, repeater, warning))
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

/// v0.14.0 — parse a warning suffix `-Nd` / `--Nd` (or w/m/y).
/// Org's `--` form is meant to override the global default
/// `org-deadline-warning-days`, but Atrium has no global-default-
/// override concept — both prefixes parse to the same numeric
/// days value, and the emitter normalises onto `-`. The unit is
/// folded to days for the DB column (Atrium models the column as
/// integer days; weeks/months/years would force a calendar-aware
/// projection that doesn't match the existing `today + N days`
/// query shape). Uncommon non-day units `w`/`m`/`y` resolve to
/// 7/30/365 day approximations on parse.
fn parse_warning(token: &str) -> Option<u32> {
    // Strip the `--` prefix first so a `-` doesn't swallow `--N`'s
    // leading dash. Both prefixes parse identically — Atrium has
    // no global-default-override concept that would distinguish
    // them.
    let rest = match token.strip_prefix("--") {
        Some(r) => r,
        None => token.strip_prefix('-')?,
    };
    if rest.is_empty() {
        return None;
    }
    let unit_pos = rest.find(|c: char| c.is_ascii_alphabetic())?;
    let interval: u32 = rest[..unit_pos].parse().ok()?;
    let unit = rest[unit_pos..].chars().next()?;
    let days = match unit {
        'd' => interval,
        'w' => interval.checked_mul(7)?,
        'm' => interval.checked_mul(30)?,
        'y' => interval.checked_mul(365)?,
        _ => return None,
    };
    Some(days)
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

    // v0.15.0 — Phase 18.5 Tier-1 statistics cookies. Captured
    // and stripped from the title; emit roundtrip preserves the
    // shape variant.
    #[test]
    fn parses_counter_cookie_strips_title() {
        let input = "* Project [3/5]\n";
        let tasks = parse_org_text(input);
        let t = &tasks[0];
        assert_eq!(t.title, "Project");
        assert_eq!(
            t.statistics_cookie,
            Some(StatisticsCookie::Counter { done: 3, total: 5 })
        );
    }

    #[test]
    fn parses_percent_cookie_strips_title() {
        let input = "* TODO Big initiative [40%]\n";
        let tasks = parse_org_text(input);
        let t = &tasks[0];
        assert_eq!(t.title, "Big initiative");
        assert_eq!(
            t.statistics_cookie,
            Some(StatisticsCookie::Percent { value: 40 })
        );
    }

    #[test]
    fn parses_cookie_before_tags() {
        let input = "* TODO Project [2/4] :work:focus:\n";
        let tasks = parse_org_text(input);
        let t = &tasks[0];
        assert_eq!(t.title, "Project");
        assert_eq!(t.tags, vec!["work", "focus"]);
        assert_eq!(
            t.statistics_cookie,
            Some(StatisticsCookie::Counter { done: 2, total: 4 })
        );
    }

    #[test]
    fn parses_empty_cookie_shapes() {
        let counter_input = "* TODO Project [/]\n";
        assert_eq!(
            parse_org_text(counter_input)[0].statistics_cookie,
            Some(StatisticsCookie::Counter { done: 0, total: 0 })
        );
        let percent_input = "* TODO Project [%]\n";
        assert_eq!(
            parse_org_text(percent_input)[0].statistics_cookie,
            Some(StatisticsCookie::Percent { value: 0 })
        );
    }

    #[test]
    fn does_not_strip_user_brackets_in_title() {
        // A bracketed token at the START of the title isn't a
        // cookie (no preceding whitespace). Stays in the title.
        let input = "* TODO [draft] Plan\n";
        let tasks = parse_org_text(input);
        assert_eq!(tasks[0].title, "[draft] Plan");
        assert_eq!(tasks[0].statistics_cookie, None);
    }

    #[test]
    fn rejects_malformed_cookie_keeps_in_title() {
        // `[abc]` doesn't match either cookie shape.
        let input = "* TODO Project [abc]\n";
        let tasks = parse_org_text(input);
        // Lands in title verbatim — not a recognised cookie.
        assert_eq!(tasks[0].title, "Project [abc]");
        assert_eq!(tasks[0].statistics_cookie, None);
    }

    // v0.14.0 — DEADLINE warning suffix (`-Nd` / `--Nd`). Phase
    // 18.5 Tier-1; round-trips the warning days into
    // `OrgTask.deadline_warning` for both prefix shapes.
    #[test]
    fn parses_deadline_with_warning_suffix() {
        let input = "\
* TODO File taxes
DEADLINE: <2026-04-15 Wed -7d>
";
        let tasks = parse_org_text(input);
        let t = &tasks[0];
        assert_eq!(t.deadline, Some(d(2026, 4, 15)));
        assert_eq!(t.deadline_warning, Some(7));
        assert!(t.deadline_repeater.is_none());
    }

    #[test]
    fn parses_deadline_with_double_dash_warning() {
        // Org's `--` form overrides the global default; Atrium has
        // no global-default-override concept so it normalises onto
        // the single-dash form. Both prefixes parse to the same
        // numeric days value.
        let input = "\
* TODO Renew passport
DEADLINE: <2026-08-01 Sat --14d>
";
        let tasks = parse_org_text(input);
        assert_eq!(tasks[0].deadline_warning, Some(14));
    }

    #[test]
    fn parses_deadline_with_repeater_and_warning_in_either_order() {
        // Per Org docs, repeater and warning may appear in either
        // order. We pull both out regardless of position.
        let repeater_first = "\
* TODO Renew domain
DEADLINE: <2026-12-01 Tue +1y -30d>
";
        let warning_first = "\
* TODO Renew domain
DEADLINE: <2026-12-01 Tue -30d +1y>
";
        for input in [repeater_first, warning_first] {
            let tasks = parse_org_text(input);
            let t = &tasks[0];
            assert_eq!(t.deadline_warning, Some(30));
            let rep = t.deadline_repeater.as_ref().unwrap();
            assert_eq!(rep.mode, "+");
            assert_eq!(rep.interval, 1);
            assert_eq!(rep.unit, 'y');
        }
    }

    #[test]
    fn parses_warning_suffix_units_w_m_y() {
        // Day units land canonically; week/month/year units fold
        // into days so the column stays integer-day.
        let cases = [
            ("DEADLINE: <2026-06-01 Mon -2w>", 14),
            ("DEADLINE: <2026-06-01 Mon -1m>", 30),
            ("DEADLINE: <2026-06-01 Mon -1y>", 365),
        ];
        for (line, expected) in cases {
            let input = format!("* TODO X\n{line}\n");
            let tasks = parse_org_text(&input);
            assert_eq!(
                tasks[0].deadline_warning,
                Some(expected),
                "unit fold for {line}"
            );
        }
    }

    #[test]
    fn parses_scheduled_warning_suffix_for_round_trip() {
        // Org allows `-Nd` after SCHEDULED too (rare). Atrium has
        // no DB column for it, but the parser captures it so the
        // emitter can write it back verbatim — preserves user
        // intent across a round-trip even though Atrium doesn't
        // semantically interpret it.
        let input = "\
* TODO Pay rent
SCHEDULED: <2026-05-01 Fri -3d>
";
        let tasks = parse_org_text(input);
        assert_eq!(tasks[0].scheduled, Some(d(2026, 5, 1)));
        assert_eq!(tasks[0].scheduled_warning, Some(3));
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
        // The legacy parse_org_text flow continues to discard
        // preamble silently — preserves backwards compat for
        // existing callers.
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
    fn captures_file_directives_with_meta() {
        // parse_org_text_with_meta surfaces the same
        // preamble that parse_org_text drops.
        let input = "\
#+TITLE: Q3 Plans
#+CATEGORY: work
#+filetags: :work:roadmap:

* TODO Real headline
";
        let file = parse_org_text_with_meta(input);
        assert_eq!(file.headlines.len(), 1);
        assert_eq!(
            file.directives.get("TITLE").map(String::as_str),
            Some("Q3 Plans")
        );
        assert_eq!(
            file.directives.get("CATEGORY").map(String::as_str),
            Some("work")
        );
        // Directive keys are upper-cased on parse so callers
        // can do case-insensitive lookups.
        assert_eq!(
            file.directives.get("FILETAGS").map(String::as_str),
            Some(":work:roadmap:")
        );
    }

    #[test]
    fn captures_file_level_properties_block() {
        // a top-level :PROPERTIES: ... :END: block
        // before the first headline lands in
        // OrgFile::file_properties; headline-attached drawers
        // still go to OrgTask::properties.
        let input = "\
#+TITLE: Q3 Plans
:PROPERTIES:
:SEQUENTIAL: t
:REVIEW_INTERVAL: 14
:END:

* TODO Headline
:PROPERTIES:
:ID: per-task-uuid
:END:
";
        let file = parse_org_text_with_meta(input);
        assert_eq!(file.headlines.len(), 1);
        assert_eq!(
            file.file_properties.get("SEQUENTIAL").map(String::as_str),
            Some("t")
        );
        assert_eq!(
            file.file_properties
                .get("REVIEW_INTERVAL")
                .map(String::as_str),
            Some("14")
        );
        // Headline-attached drawer untouched — sanity that the
        // file-vs-headline split doesn't leak.
        assert_eq!(
            file.headlines[0].properties.get("ID").map(String::as_str),
            Some("per-task-uuid")
        );
        assert!(!file.headlines[0].properties.contains_key("SEQUENTIAL"));
    }

    #[test]
    fn empty_with_meta_has_no_directives_or_properties() {
        let file = parse_org_text_with_meta("* TODO Plain\n");
        assert!(file.directives.is_empty());
        assert!(file.file_properties.is_empty());
        assert_eq!(file.headlines.len(), 1);
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

    /// Roadmap §17 acceptance: a 1000-task project file parses
    /// in under half a second so the watcher's debounce + parse
    /// cycle stays comfortably under the perceptual budget on
    /// realistic vaults. Generous bound — debug builds on slow
    /// CI runners shouldn't trip; real-machine release builds
    /// are typically in the low tens of milliseconds.
    #[test]
    fn large_file_parses_under_budget() {
        use std::fmt::Write;
        use std::time::Instant;

        let mut text = String::with_capacity(150 * 1000);
        text.push_str("#+TITLE: Large Project\n");
        text.push_str(":PROPERTIES:\n:ID: 11111111-2222-3333-4444-555555555555\n:END:\n\n");
        for i in 0..1000 {
            writeln!(text, "* TODO Task number {i}").unwrap();
            writeln!(
                text,
                "SCHEDULED: <2026-05-09 Sat> DEADLINE: <2026-05-15 Fri>"
            )
            .unwrap();
            text.push_str(":PROPERTIES:\n");
            writeln!(text, ":ID: aaaa{i:04}-2222-3333-4444-555555555555").unwrap();
            text.push_str(":CREATED: [2026-05-01 Fri 09:00]\n");
            text.push_str(":END:\n");
            writeln!(text, "Body content for task {i}, plain prose.").unwrap();
            text.push('\n');
        }

        let start = Instant::now();
        let parsed = parse_org_text_with_meta(&text);
        let elapsed = start.elapsed();

        assert_eq!(
            parsed.headlines.len(),
            1000,
            "should round-trip 1000 headlines"
        );
        assert!(
            elapsed.as_millis() < 500,
            "1K-task parse took {} ms; budget is 500 ms",
            elapsed.as_millis()
        );
    }
}
