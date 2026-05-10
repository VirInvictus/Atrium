// SPDX-License-Identifier: MIT
//! v0.15.0 — Phase 18.5 Tier-1 inline checkbox parser for note
//! bodies. Recognises Org-style `- [ ]` / `- [X]` / `- [-]`
//! lines so the Inspector can render interactive toggles and the
//! statistics-cookie counter can fold body checkboxes alongside
//! child TODOs.
//!
//! Discipline:
//!
//! - **The body string stays the source of truth.** This module
//!   reads the string and emits a structured view; toggles
//!   produce a *new* string with one line rewritten in place.
//!   Round-trip-stable: `parse → toggle(line) → reparse` round-
//!   trips to the same shape (modulo the toggled state).
//! - **Forgiving scanner.** Lines that don't match the checkbox
//!   pattern are simply not surfaced. The body otherwise stays
//!   verbatim — no normalisation, no whitespace trimming, no
//!   bullet-style enforcement.
//! - **Pattern.** `^(\s*)([-+*])\s+\[([ Xx-])\]\s+(.*)$`. The
//!   roadmap-named shape is `- [ ]`; we accept `+` and `*` as
//!   valid bullet characters because Org does. Numbered lists
//!   (`1. [ ]`) aren't surfaced — Org rendering doesn't show
//!   them as checkboxes by default, and we follow.
//!
//! "Done" criterion for the cookie counter: `[X]` (or `[x]`)
//! counts as done; `[ ]` and `[-]` count as open.

/// State of a single checkbox line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckboxState {
    /// `- [ ]` — unchecked.
    Unchecked,
    /// `- [X]` (or `- [x]`) — checked.
    Checked,
    /// `- [-]` — partial / indeterminate (Org's "some children
    /// checked" parent state).
    Indeterminate,
}

impl CheckboxState {
    /// `true` when the cookie counter should treat this state as
    /// done. Unchecked + Indeterminate count as open; Checked is
    /// done.
    pub fn is_done(self) -> bool {
        matches!(self, CheckboxState::Checked)
    }

    fn from_char(c: char) -> Option<Self> {
        match c {
            ' ' => Some(CheckboxState::Unchecked),
            'X' | 'x' => Some(CheckboxState::Checked),
            '-' => Some(CheckboxState::Indeterminate),
            _ => None,
        }
    }
}

/// One checkbox line, surfaced from a body string. The
/// `line_index` is a zero-based index into the body's
/// newline-split lines so the Inspector can route a toggle back
/// to the right line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyCheckbox {
    /// Zero-based line index in the body (split on '\n').
    pub line_index: usize,
    /// Leading-whitespace prefix length in chars. The Inspector
    /// uses this for indent-aware rendering.
    pub indent: usize,
    pub state: CheckboxState,
    /// Text after the `[X] ` marker, trimmed of trailing
    /// whitespace.
    pub label: String,
}

/// Parse a body string and return one `BodyCheckbox` per line
/// that matches the pattern. Order matches body-line order.
pub fn parse_body_checkboxes(body: &str) -> Vec<BodyCheckbox> {
    body.lines()
        .enumerate()
        .filter_map(|(i, line)| {
            parse_one(line).map(|(indent, state, label)| BodyCheckbox {
                line_index: i,
                indent,
                state,
                label,
            })
        })
        .collect()
}

/// Toggle the checkbox on `line_index` in `body`. Returns the
/// new body string. If the line isn't a checkbox, returns the
/// body unchanged. Toggle semantics:
///
/// - `[ ]` → `[X]`
/// - `[X]` → `[ ]`
/// - `[-]` → `[ ]` (mirrors Org's `org-toggle-checkbox` default,
///   which clears a partial state to unchecked rather than
///   promoting it; user can click again to check)
pub fn toggle_body_checkbox(body: &str, line_index: usize) -> String {
    let mut out = String::with_capacity(body.len());
    let mut emitted_any = false;
    for (i, line) in body.lines().enumerate() {
        if emitted_any {
            out.push('\n');
        }
        emitted_any = true;
        if i == line_index
            && let Some((indent, state, label)) = parse_one(line)
        {
            let new_state = match state {
                CheckboxState::Unchecked => CheckboxState::Checked,
                CheckboxState::Checked => CheckboxState::Unchecked,
                CheckboxState::Indeterminate => CheckboxState::Unchecked,
            };
            // Recover the bullet char from the source so we
            // don't normalise `+` / `*` to `-` accidentally.
            let bullet = bullet_char(line).unwrap_or('-');
            let marker = match new_state {
                CheckboxState::Unchecked => "[ ]",
                CheckboxState::Checked => "[X]",
                CheckboxState::Indeterminate => "[-]",
            };
            let pad = " ".repeat(indent);
            out.push_str(&format!("{pad}{bullet} {marker} {label}"));
        } else {
            out.push_str(line);
        }
    }
    // Preserve a trailing newline if the original had one — `lines()`
    // strips it.
    if body.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Count `(done, total)` body checkboxes. Used by the
/// statistics-cookie projection to fold body-level checkboxes
/// alongside child TODO counts (mirrors Org's
/// `org-hierarchical-todo-statistics` when `org-checkbox-hierarchical-statistics`
/// is on, which is the default).
pub fn count_body_checkboxes(body: &str) -> (u32, u32) {
    let mut done = 0u32;
    let mut total = 0u32;
    for cb in parse_body_checkboxes(body) {
        total = total.saturating_add(1);
        if cb.state.is_done() {
            done = done.saturating_add(1);
        }
    }
    (done, total)
}

/// Returns `Some((indent_len, state, label))` if `line` is a
/// checkbox line. The label is trimmed of trailing whitespace
/// only — leading whitespace inside the label (after the marker)
/// is preserved.
fn parse_one(line: &str) -> Option<(usize, CheckboxState, String)> {
    let trimmed_start = line.trim_start_matches([' ', '\t']);
    let indent = line.len() - trimmed_start.len();
    let mut chars = trimmed_start.chars();
    let bullet = chars.next()?;
    if !matches!(bullet, '-' | '+' | '*') {
        return None;
    }
    if chars.next()? != ' ' {
        return None;
    }
    if chars.next()? != '[' {
        return None;
    }
    let state_char = chars.next()?;
    let state = CheckboxState::from_char(state_char)?;
    if chars.next()? != ']' {
        return None;
    }
    if chars.next()? != ' ' {
        return None;
    }
    let label_start = trimmed_start.len() - chars.as_str().len();
    let label = trimmed_start[label_start..].trim_end().to_string();
    Some((indent, state, label))
}

/// Recover the bullet character (`-`, `+`, `*`) from a line
/// known to match `parse_one`. Returns `None` if the line
/// doesn't match — caller falls back to `-`.
fn bullet_char(line: &str) -> Option<char> {
    let trimmed_start = line.trim_start_matches([' ', '\t']);
    let bullet = trimmed_start.chars().next()?;
    if matches!(bullet, '-' | '+' | '*') {
        Some(bullet)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_unchecked() {
        let cb = parse_body_checkboxes("- [ ] do the thing");
        assert_eq!(cb.len(), 1);
        assert_eq!(cb[0].state, CheckboxState::Unchecked);
        assert_eq!(cb[0].label, "do the thing");
        assert_eq!(cb[0].indent, 0);
        assert_eq!(cb[0].line_index, 0);
    }

    #[test]
    fn parses_checked_uppercase_and_lowercase() {
        let cb = parse_body_checkboxes("- [X] big X\n- [x] small x");
        assert_eq!(cb.len(), 2);
        assert_eq!(cb[0].state, CheckboxState::Checked);
        assert_eq!(cb[1].state, CheckboxState::Checked);
    }

    #[test]
    fn parses_indeterminate() {
        let cb = parse_body_checkboxes("- [-] partially done");
        assert_eq!(cb[0].state, CheckboxState::Indeterminate);
    }

    #[test]
    fn parses_alternative_bullets() {
        let cb = parse_body_checkboxes("+ [ ] plus\n* [ ] star");
        assert_eq!(cb.len(), 2);
        assert_eq!(cb[0].label, "plus");
        assert_eq!(cb[1].label, "star");
    }

    #[test]
    fn parses_indented_checkbox() {
        let cb = parse_body_checkboxes("    - [ ] indented");
        assert_eq!(cb[0].indent, 4);
        assert_eq!(cb[0].label, "indented");
    }

    #[test]
    fn skips_non_checkbox_lines() {
        let body =
            "Some prose.\n- [ ] real cb\nMore prose.\n- not a cb (no brackets)\n- [Y] bad state";
        let cb = parse_body_checkboxes(body);
        assert_eq!(cb.len(), 1);
        assert_eq!(cb[0].label, "real cb");
        assert_eq!(cb[0].line_index, 1);
    }

    #[test]
    fn toggle_unchecked_to_checked() {
        let body = "intro\n- [ ] one\n- [ ] two\noutro";
        let new = toggle_body_checkbox(body, 1);
        assert_eq!(new, "intro\n- [X] one\n- [ ] two\noutro");
    }

    #[test]
    fn toggle_checked_to_unchecked() {
        let body = "- [X] done";
        let new = toggle_body_checkbox(body, 0);
        assert_eq!(new, "- [ ] done");
    }

    #[test]
    fn toggle_indeterminate_clears_to_unchecked() {
        let body = "- [-] partial";
        let new = toggle_body_checkbox(body, 0);
        assert_eq!(new, "- [ ] partial");
    }

    #[test]
    fn toggle_preserves_indent_and_bullet_char() {
        let body = "  + [ ] indented plus";
        let new = toggle_body_checkbox(body, 0);
        assert_eq!(new, "  + [X] indented plus");
    }

    #[test]
    fn toggle_preserves_trailing_newline() {
        let body = "- [ ] one\n";
        let new = toggle_body_checkbox(body, 0);
        assert_eq!(new, "- [X] one\n");
    }

    #[test]
    fn toggle_noop_on_non_checkbox_line() {
        let body = "just prose\n- [ ] cb";
        let new = toggle_body_checkbox(body, 0);
        assert_eq!(new, body);
    }

    #[test]
    fn count_body_checkboxes_done_total() {
        let body = "- [X] a\n- [ ] b\n- [X] c\n- [-] d";
        let (done, total) = count_body_checkboxes(body);
        assert_eq!(done, 2);
        assert_eq!(total, 4);
    }

    #[test]
    fn count_empty_body_returns_zero_zero() {
        assert_eq!(count_body_checkboxes(""), (0, 0));
        assert_eq!(count_body_checkboxes("just prose"), (0, 0));
    }
}
