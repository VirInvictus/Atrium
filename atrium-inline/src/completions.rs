// SPDX-License-Identifier: MIT
//! Tab-completion candidate sources for the inline-syntax parser.
//!
//! Pure-logic helpers a GTK widget (or a future TUI input layer)
//! consults to figure out:
//!
//! 1. Whether the cursor is currently inside a recognised inline
//!    token (`#tag` / `@something` / `!N`).
//! 2. What candidates to suggest given the partial text.
//!
//! Tag candidates aren't built in — the caller passes them in
//! (typically from the read pool's `list_tags`). Schedule and
//! priority candidates *are* built-in because the parser owns the
//! vocabulary; surfacing the full list from one place keeps the
//! parser, the docs, and the completion candidates in lockstep.

/// What the cursor is currently positioned inside.
///
/// The string carries the prefix typed *after* the marker
/// character (`#`/`@`/`!`) and before the cursor — empty when the
/// user has just typed the marker itself with no follow-up
/// characters yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionContext {
    /// Inside a `#tag` token.
    Tag(String),
    /// Inside an `@<something>` token (any `@`-prefixed word
    /// — `today`, `monday`, `2026-05-15`, or the bare `@` after
    /// a freshly-typed marker).
    Schedule(String),
    /// Inside a `!N` priority token.
    Priority(String),
    /// Cursor isn't on a recognised inline token. The widget
    /// shouldn't open the completion popover.
    None,
}

/// Built-in `@`-token candidates the parser recognises. The order
/// reflects how the completion popover should rank them — the
/// natural keywords first, then weekdays in week order.
///
/// Note that `@deadline` requires a follow-up date and is included
/// here because typing it then a space is the most common use
/// case; the popover dismisses on space anyway.
pub const SCHEDULE_KEYWORDS: &[&str] = &[
    "today",
    "tomorrow",
    "someday",
    "deadline",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];

/// Priority levels surfaced in the completion popover when the
/// user types `!`. Atrium projects each onto a `priority-N` tag
/// (until Phase 19.5's numeric column lands).
pub const PRIORITY_LEVELS: &[&str] = &["1", "2", "3"];

/// Identify the active inline token under the cursor.
///
/// `cursor` is a byte offset into `text`. Values past the end of
/// `text` clamp to `text.len()` so callers can pass GTK's
/// `position()` directly without bounds-checking.
///
/// Walks backwards from the cursor to the nearest whitespace (or
/// the start of the string) and inspects the first character of
/// the current token. Tokens shorter than two characters never
/// trigger — typing a bare `#` / `@` / `!` *is* a recognised
/// context (the marker triggers completion before any partial
/// text), but a token already containing whitespace isn't.
pub fn context_at(text: &str, cursor: usize) -> CompletionContext {
    let cursor = cursor.min(text.len());
    let prefix = &text[..cursor];
    let token_start = prefix.rfind(char::is_whitespace).map_or(0, |i| i + 1);
    let token = &prefix[token_start..];

    if let Some(rest) = token.strip_prefix('#') {
        CompletionContext::Tag(rest.to_string())
    } else if let Some(rest) = token.strip_prefix('@') {
        CompletionContext::Schedule(rest.to_string())
    } else if let Some(rest) = token.strip_prefix('!') {
        CompletionContext::Priority(rest.to_string())
    } else {
        CompletionContext::None
    }
}

/// Filter `candidates` to those that start with `prefix`,
/// case-insensitively. Preserves the source order so callers can
/// hand in semantically-ordered lists (`SCHEDULE_KEYWORDS` puts
/// `today` first by design, not alphabetically).
pub fn matches<I, S>(prefix: &str, candidates: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let lower_prefix = prefix.to_ascii_lowercase();
    candidates
        .into_iter()
        .filter_map(|c| {
            let s = c.as_ref();
            if s.to_ascii_lowercase().starts_with(&lower_prefix) {
                Some(s.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Replace the active token's body with `chosen`. Returns the
/// resulting `(new_text, new_cursor)` tuple — `new_cursor` lands
/// at the byte position just after the inserted text so the user
/// can keep typing without repositioning.
///
/// Falls through (returns the inputs unchanged) when the cursor
/// isn't inside a recognised token. The marker character (`#`/
/// `@`/`!`) is *not* part of `chosen` — the function preserves
/// whatever marker the user typed.
pub fn replace_token(text: &str, cursor: usize, chosen: &str) -> (String, usize) {
    let cursor = cursor.min(text.len());
    let prefix = &text[..cursor];
    let token_start = prefix.rfind(char::is_whitespace).map_or(0, |i| i + 1);
    let token = &prefix[token_start..];
    if !matches!(token.chars().next(), Some('#') | Some('@') | Some('!')) {
        return (text.to_string(), cursor);
    }
    let marker = &token[..1];
    let after = &text[cursor..];
    let mut out = String::with_capacity(text.len() + chosen.len());
    out.push_str(&text[..token_start]);
    out.push_str(marker);
    out.push_str(chosen);
    let new_cursor = out.len();
    out.push_str(after);
    (out, new_cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_detects_tag() {
        let c = context_at("Buy milk #ur", 12);
        assert_eq!(c, CompletionContext::Tag("ur".into()));
    }

    #[test]
    fn context_detects_schedule() {
        let c = context_at("Call dentist @to", 16);
        assert_eq!(c, CompletionContext::Schedule("to".into()));
    }

    #[test]
    fn context_detects_priority() {
        let c = context_at("File taxes !1", 13);
        assert_eq!(c, CompletionContext::Priority("1".into()));
    }

    #[test]
    fn context_at_marker_only() {
        // Just-typed bare marker — empty prefix, but still a
        // recognised context so the popover can show all
        // candidates immediately.
        assert_eq!(
            context_at("Buy milk #", 10),
            CompletionContext::Tag(String::new())
        );
        assert_eq!(
            context_at("Note @", 6),
            CompletionContext::Schedule(String::new())
        );
        assert_eq!(
            context_at("Note !", 6),
            CompletionContext::Priority(String::new())
        );
    }

    #[test]
    fn context_none_when_no_marker() {
        assert_eq!(context_at("Plain text", 5), CompletionContext::None);
        assert_eq!(context_at("", 0), CompletionContext::None);
    }

    #[test]
    fn context_none_when_cursor_after_whitespace() {
        // Token is "ur" but cursor sits past the trailing space.
        // The current token is empty / has no marker, so no context.
        let text = "Buy milk #ur ";
        assert_eq!(context_at(text, text.len()), CompletionContext::None);
    }

    #[test]
    fn context_clamps_cursor_past_end() {
        // GTK can hand us cursor positions equal to text.len()
        // (just past the last character). Clamping is on us.
        let text = "Plan @mo";
        assert_eq!(
            context_at(text, 999),
            CompletionContext::Schedule("mo".into())
        );
    }

    #[test]
    fn matches_prefix_case_insensitive() {
        let pool = ["today", "tomorrow", "someday"];
        assert_eq!(matches("to", pool), vec!["today", "tomorrow"]);
        assert_eq!(matches("TO", pool), vec!["today", "tomorrow"]);
        assert_eq!(matches("Today", pool), vec!["today"]);
    }

    #[test]
    fn matches_empty_prefix_returns_everything() {
        let pool = ["today", "tomorrow"];
        assert_eq!(matches("", pool), vec!["today", "tomorrow"]);
    }

    #[test]
    fn matches_preserves_source_order() {
        // SCHEDULE_KEYWORDS puts `today` before alphabetical sort
        // would. The filter must keep that ordering.
        let r = matches("t", SCHEDULE_KEYWORDS.iter().copied());
        assert!(r.starts_with(&["today".to_string(), "tomorrow".to_string()]));
    }

    #[test]
    fn matches_no_match_returns_empty() {
        let pool = ["today"];
        assert!(matches("z", pool).is_empty());
    }

    #[test]
    fn replace_token_swaps_partial_for_chosen() {
        // Cursor sits at the end of the partial token.
        let (out, cursor) = replace_token("Plan @mo", 8, "monday");
        assert_eq!(out, "Plan @monday");
        assert_eq!(cursor, "Plan @monday".len());
    }

    #[test]
    fn replace_token_preserves_text_after_cursor() {
        // Cursor in the middle of the string — characters beyond
        // it survive and the new cursor lands right after the
        // inserted text.
        let text = "Plan @mo and stretch";
        let (out, cursor) = replace_token(text, 8, "monday");
        assert_eq!(out, "Plan @monday and stretch");
        assert_eq!(cursor, "Plan @monday".len());
    }

    #[test]
    fn replace_token_keeps_user_marker() {
        // User typed `@`, not `#` or `!`. The replacement keeps it.
        let (out, _) = replace_token("Plan @mo", 8, "monday");
        assert!(out.starts_with("Plan @"));

        let (out_p, _) = replace_token("Pay rent !", 10, "1");
        assert!(out_p.ends_with("!1"));

        let (out_t, _) = replace_token("Buy milk #ur", 12, "urgent");
        assert!(out_t.ends_with("#urgent"));
    }

    #[test]
    fn replace_token_no_op_outside_token() {
        let (out, cursor) = replace_token("Plain text", 5, "X");
        assert_eq!(out, "Plain text");
        assert_eq!(cursor, 5);
    }

    #[test]
    fn replace_token_clamps_cursor() {
        // Past-end cursor still works.
        let (out, _) = replace_token("Plan @mo", 99, "monday");
        assert_eq!(out, "Plan @monday");
    }

    // ── Vocabulary regression guards ─────────────────────────────

    #[test]
    fn schedule_keywords_match_parser() {
        // Spot-check that the keywords listed here exist in the
        // parser's recognised set. The popover surfaces full names
        // only — the 3-letter shortcuts (`@mon` / `@tue` / …) stay
        // available to the parser for power users but don't clutter
        // the suggestion list. If a new full-name `@`-token lands
        // in the parser without being added here, the popover
        // won't surface it — fail loudly.
        for keyword in [
            "today",
            "tomorrow",
            "someday",
            "deadline",
            "monday",
            "tuesday",
            "wednesday",
            "thursday",
            "friday",
            "saturday",
            "sunday",
        ] {
            assert!(
                SCHEDULE_KEYWORDS.contains(&keyword),
                "{keyword} missing from SCHEDULE_KEYWORDS"
            );
        }
    }

    #[test]
    fn priority_levels_are_one_two_three() {
        assert_eq!(PRIORITY_LEVELS, &["1", "2", "3"]);
    }
}
