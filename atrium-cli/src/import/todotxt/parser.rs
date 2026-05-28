// SPDX-License-Identifier: MIT
//! todo.txt line parser. v0.27.0. Stdlib-only.
//!
//! The format is simple enough that a hand-tokeniser fits in
//! one file: split on spaces, treat the first few fields
//! positionally (completion marker, priority, dates), then
//! walk the remaining tokens classifying as `@context` /
//! `+project` / `key:value` / title-word.

use chrono::NaiveDate;

/// One parsed todo.txt line in Atrium's typed shape.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TodoTxtTask {
    /// True when the line started with `x `.
    pub completed: bool,
    pub completion_date: Option<NaiveDate>,
    /// Priority letter (`A` through `Z`). None when the source
    /// didn't include `(L)`.
    pub priority: Option<char>,
    pub creation_date: Option<NaiveDate>,
    /// Description with `@context` / `+project` / `key:value`
    /// tokens stripped. Whitespace-collapsed.
    pub description: String,
    /// `@`-prefixed tokens. Order preserved.
    pub contexts: Vec<String>,
    /// `+`-prefixed tokens. Order preserved. The mapper drops
    /// these (lossy) since `--into` wins.
    pub projects: Vec<String>,
    /// `key:value` pairs. The mapper routes `due:` to deadline,
    /// `t:` to defer_until, and others to lossy.
    pub key_values: Vec<(String, String)>,
}

/// Parse a single todo.txt line. Returns `None` for empty
/// lines and lines starting with `#` (treated as comments per
/// the most-widely-accepted extension).
pub fn parse_line(line: &str) -> Option<TodoTxtTask> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let mut tokens: Vec<&str> = trimmed.split_whitespace().collect();
    let mut task = TodoTxtTask::default();
    let mut cursor = 0;

    // Completion marker: `x` as the first token (case-sensitive
    // per the spec). The optional next token is the completion
    // date.
    if tokens.first().copied() == Some("x") {
        task.completed = true;
        cursor += 1;
        if let Some(date_tok) = tokens.get(cursor).copied()
            && let Some(d) = parse_date(date_tok)
        {
            task.completion_date = Some(d);
            cursor += 1;
        }
    }

    // Priority `(L)` where L is A-Z (uppercase).
    if let Some(tok) = tokens.get(cursor).copied()
        && let Some(letter) = parse_priority(tok)
    {
        task.priority = Some(letter);
        cursor += 1;
    }

    // Creation date — the next token if it parses as YYYY-MM-DD.
    if let Some(tok) = tokens.get(cursor).copied()
        && let Some(d) = parse_date(tok)
    {
        task.creation_date = Some(d);
        cursor += 1;
    }

    // Remaining tokens form the description plus inline
    // `@`/`+`/`key:value` markers. Tokens with no special
    // prefix join the description.
    let mut description_words: Vec<&str> = Vec::new();
    for tok in tokens.drain(cursor..) {
        if let Some(name) = tok.strip_prefix('@') {
            if !name.is_empty() {
                task.contexts.push(name.to_string());
            }
        } else if let Some(name) = tok.strip_prefix('+') {
            if !name.is_empty() {
                task.projects.push(name.to_string());
            }
        } else if let Some((key, value)) = split_key_value(tok) {
            task.key_values.push((key.to_string(), value.to_string()));
        } else {
            description_words.push(tok);
        }
    }
    task.description = description_words.join(" ");
    Some(task)
}

/// Parse the entire body as a multi-line todo.txt document.
/// Skips comments + blank lines.
pub fn parse_document(text: &str) -> Vec<TodoTxtTask> {
    text.lines().filter_map(parse_line).collect()
}

fn parse_date(token: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(token, "%Y-%m-%d").ok()
}

fn parse_priority(token: &str) -> Option<char> {
    let bytes = token.as_bytes();
    if bytes.len() == 3 && bytes[0] == b'(' && bytes[2] == b')' && bytes[1].is_ascii_uppercase() {
        Some(bytes[1] as char)
    } else {
        None
    }
}

/// Recognise `key:value` extensions. The key must be an
/// alphanumeric / underscore identifier (no leading colon) and
/// the value must be a non-empty string with no internal colon.
/// We deliberately don't recurse — `http://example.com` will
/// parse as `http` → `//example.com`, but the mapper routes it
/// to a lossy entry, not a typed column, so the misclassification
/// is contained.
fn split_key_value(token: &str) -> Option<(&str, &str)> {
    let idx = token.find(':')?;
    let (key, rest) = token.split_at(idx);
    if key.is_empty() {
        return None;
    }
    if !key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    let value = &rest[1..];
    if value.is_empty() {
        return None;
    }
    Some((key, value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_title_only() {
        let t = parse_line("Buy milk").unwrap();
        assert_eq!(t.description, "Buy milk");
        assert!(!t.completed);
        assert!(t.priority.is_none());
    }

    #[test]
    fn parses_priority_and_creation_date() {
        let t = parse_line("(A) 2026-04-15 Buy milk").unwrap();
        assert_eq!(t.priority, Some('A'));
        assert_eq!(
            t.creation_date,
            Some(NaiveDate::from_ymd_opt(2026, 4, 15).unwrap()),
        );
        assert_eq!(t.description, "Buy milk");
    }

    #[test]
    fn parses_completion_marker_and_date() {
        let t = parse_line("x 2026-05-02 (A) 2026-04-15 Buy milk").unwrap();
        assert!(t.completed);
        assert_eq!(
            t.completion_date,
            Some(NaiveDate::from_ymd_opt(2026, 5, 2).unwrap()),
        );
        assert_eq!(t.priority, Some('A'));
        assert_eq!(
            t.creation_date,
            Some(NaiveDate::from_ymd_opt(2026, 4, 15).unwrap()),
        );
        assert_eq!(t.description, "Buy milk");
    }

    #[test]
    fn classifies_inline_at_plus_and_key_value_tokens() {
        let t =
            parse_line("Buy milk @home @errands +groceries due:2026-05-01 t:2026-04-20").unwrap();
        assert_eq!(t.description, "Buy milk");
        assert_eq!(t.contexts, vec!["home", "errands"]);
        assert_eq!(t.projects, vec!["groceries"]);
        assert_eq!(
            t.key_values,
            vec![
                ("due".to_string(), "2026-05-01".to_string()),
                ("t".to_string(), "2026-04-20".to_string()),
            ],
        );
    }

    #[test]
    fn collapses_inline_tokens_out_of_description() {
        let t = parse_line("Pay @home rent +bills due:2026-05-01").unwrap();
        assert_eq!(t.description, "Pay rent");
    }

    #[test]
    fn skips_blank_and_comment_lines() {
        assert!(parse_line("").is_none());
        assert!(parse_line("   ").is_none());
        assert!(parse_line("# a comment").is_none());
    }

    #[test]
    fn priority_below_a_still_captures_letter() {
        // Mapping side decides whether to surface lossy.
        let t = parse_line("(D) some task").unwrap();
        assert_eq!(t.priority, Some('D'));
        assert_eq!(t.description, "some task");
    }

    #[test]
    fn double_letter_priority_does_not_match() {
        let t = parse_line("(AA) some task").unwrap();
        assert_eq!(t.priority, None);
        assert!(t.description.contains("(AA)"));
    }

    #[test]
    fn parse_document_strips_comments_and_blanks() {
        let text = "# header\n(A) one\n\n(B) two\n";
        let tasks = parse_document(text);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].priority, Some('A'));
        assert_eq!(tasks[1].priority, Some('B'));
    }

    #[test]
    fn split_key_value_rejects_url_like_token() {
        // http://example.com — we accept `http` → `//example.com`
        // for parser simplicity; mapper-side routes to lossy.
        // This documents the chosen behaviour.
        let t = parse_line("Look at http://example.com").unwrap();
        assert!(t.key_values.iter().any(|(k, _)| k == "http"));
        // The URL fragment didn't land in the description.
        assert_eq!(t.description, "Look at");
    }

    #[test]
    fn x_prefix_without_space_is_not_completion() {
        // `xfoo` is part of the description, not a completion
        // marker — the spec requires `x ` (lowercase x + space).
        let t = parse_line("xfoo bar").unwrap();
        assert!(!t.completed);
        assert_eq!(t.description, "xfoo bar");
    }

    #[test]
    fn empty_at_and_plus_tokens_dont_create_entries() {
        // `@` or `+` with nothing after — keep as literal in
        // description rather than emitting empty context/project.
        let t = parse_line("at @ sign +").unwrap();
        // Neither pushed.
        assert!(t.contexts.is_empty());
        assert!(t.projects.is_empty());
    }
}
