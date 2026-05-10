// SPDX-License-Identifier: MIT
//! v0.19.0 — Phase 18.5 Tier-2 Org-link parser for note bodies.
//! Recognises `[[id:UUID]]` and `[[id:UUID][label]]` patterns
//! so the Inspector can render clickable spans that focus the
//! linked task. ID-based links are what `org-roam` is built on
//! and what Karl Voit's UOMF advocates for portability.
//!
//! Discipline:
//!
//! - **The body string stays the source of truth.** This module
//!   reads the string and emits a structured view; no rewrites.
//!   Bodies round-trip verbatim through Org's body capture
//!   (the existing `OrgTask.body` field) — the writer doesn't
//!   need to do anything special, and external Emacs edits to
//!   link text flow back through the watcher unchanged.
//! - **Forgiving scanner.** Lines that don't contain a link are
//!   silently skipped. Malformed `[[`...`]]` constructs (missing
//!   `id:` prefix, unbalanced brackets, empty UUID) are left in
//!   the body verbatim — they aren't valid Org-mode links
//!   either, so we follow.
//! - **Pattern.** `[[id:UUID]]` (label-less) or
//!   `[[id:UUID][label]]` (with display text). The UUID matches
//!   the v4 shape Atrium emits but we don't enforce that — any
//!   non-empty content between `id:` and the closing `]` is
//!   accepted; a stale link to a deleted task surfaces as a
//!   resolution miss at click time, not a parse miss.

/// One Org link captured from a body string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyLink {
    /// Byte range in the body string the link spans (suitable
    /// for `&body[link.range.clone()]` to recover the literal
    /// text).
    pub range: std::ops::Range<usize>,
    /// The UUID after the `id:` prefix.
    pub target_uuid: String,
    /// Display text. For `[[id:U]]` (label-less form), this
    /// equals `target_uuid` — the renderer falls back to the
    /// UUID when the user didn't supply a label.
    pub label: String,
    /// `true` when the link source had an explicit `[label]`.
    /// Lets the renderer style label-less links differently
    /// (e.g., shorten to `id:abc-…` rather than displaying the
    /// full UUID inline).
    pub has_explicit_label: bool,
}

/// Parse a body string and return one `BodyLink` per
/// `[[id:UUID][label]]` (or `[[id:UUID]]`) match. Returns in
/// source order. Bodies with no links return an empty Vec.
pub fn parse_body_links(body: &str) -> Vec<BodyLink> {
    let mut out = Vec::new();
    let bytes = body.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        // Look for the link's opening `[[`.
        if bytes[i] != b'[' || bytes[i + 1] != b'[' {
            i += 1;
            continue;
        }
        let open = i;
        // The path part starts after `[[`.
        let path_start = open + 2;
        // Path must start with `id:` to be an Atrium-recognised
        // task link. Other Org link types (`file:`, `https:`,
        // `mailto:`) round-trip via the body string verbatim
        // but aren't surfaced as task-jumpable.
        if !body[path_start..].starts_with("id:") {
            i = path_start;
            continue;
        }
        let uuid_start = path_start + 3; // skip "id:"
        // The path ends at either `]` (start of optional label)
        // or `]]` (label-less close). Scan up to whichever comes
        // first.
        let Some(close_path) = find_in_byte_range(bytes, uuid_start, b']') else {
            // No closing bracket on the path; skip past the open
            // and keep scanning.
            i = path_start;
            continue;
        };
        let target_uuid = &body[uuid_start..close_path];
        if target_uuid.is_empty() {
            i = close_path + 1;
            continue;
        }
        // Two cases for what follows the path's closing `]`:
        //   (a) Another `]` → label-less form `[[id:U]]`. Total
        //       link length = close_path + 2.
        //   (b) `[` → start of label. Find the next `]]` for
        //       the close.
        let next_byte = bytes.get(close_path + 1);
        match next_byte {
            Some(b']') => {
                let total_close = close_path + 2;
                out.push(BodyLink {
                    range: open..total_close,
                    target_uuid: target_uuid.to_string(),
                    label: target_uuid.to_string(),
                    has_explicit_label: false,
                });
                i = total_close;
            }
            Some(b'[') => {
                let label_start = close_path + 2;
                let Some(label_end) = find_double_close(bytes, label_start) else {
                    // Unterminated label — bail past the open.
                    i = path_start;
                    continue;
                };
                let label = &body[label_start..label_end];
                let total_close = label_end + 2;
                out.push(BodyLink {
                    range: open..total_close,
                    target_uuid: target_uuid.to_string(),
                    label: label.to_string(),
                    has_explicit_label: true,
                });
                i = total_close;
            }
            _ => {
                // Neither `]` nor `[` after the path's closer —
                // not a valid Org link. Bail past the open.
                i = path_start;
            }
        }
    }
    out
}

/// Search byte slice for `target` starting at `from`. Returns
/// the index of the match, or `None` if absent. Linear; the
/// link bodies are short enough that this beats pulling in a
/// search crate.
fn find_in_byte_range(bytes: &[u8], from: usize, target: u8) -> Option<usize> {
    bytes
        .iter()
        .enumerate()
        .skip(from)
        .find_map(|(i, &b)| if b == target { Some(i) } else { None })
}

/// Search for `]]` (the link close) starting at `from`. Returns
/// the index of the first `]` of the pair.
fn find_double_close(bytes: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i + 1 < bytes.len() {
        if bytes[i] == b']' && bytes[i + 1] == b']' {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_label_less_link() {
        let body = "see [[id:abc-123]] for context";
        let links = parse_body_links(body);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target_uuid, "abc-123");
        assert_eq!(links[0].label, "abc-123");
        assert!(!links[0].has_explicit_label);
        // Range covers the whole `[[id:abc-123]]` span.
        assert_eq!(&body[links[0].range.clone()], "[[id:abc-123]]");
    }

    #[test]
    fn parses_link_with_label() {
        let body = "blocked by [[id:xyz-789][the staging migration]]";
        let links = parse_body_links(body);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target_uuid, "xyz-789");
        assert_eq!(links[0].label, "the staging migration");
        assert!(links[0].has_explicit_label);
    }

    #[test]
    fn parses_multiple_links_in_one_body() {
        let body = "depends on [[id:a][A]] and [[id:b]] then [[id:c][C]]";
        let links = parse_body_links(body);
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].target_uuid, "a");
        assert_eq!(links[1].target_uuid, "b");
        assert_eq!(links[2].target_uuid, "c");
    }

    #[test]
    fn skips_non_id_link_protocols() {
        // file: / https: / mailto: aren't task links — they
        // round-trip via the body but aren't surfaced.
        let body = "see [[file:./other.org][external]] and [[https://example.com]]";
        let links = parse_body_links(body);
        assert!(links.is_empty());
    }

    #[test]
    fn skips_empty_uuid() {
        // `[[id:]]` and `[[id:][label]]` aren't useful; skip.
        let body = "[[id:]] and [[id:][bad]]";
        let links = parse_body_links(body);
        assert!(links.is_empty());
    }

    #[test]
    fn skips_unterminated_link() {
        // Open `[[` with no close — not a link. The scanner
        // shouldn't loop or panic; it should just bail past
        // the construct.
        let body = "weird [[id:abc and then more text";
        let links = parse_body_links(body);
        assert!(links.is_empty());
    }

    #[test]
    fn skips_unterminated_label() {
        // Path closes but label doesn't.
        let body = "[[id:abc][missing close";
        let links = parse_body_links(body);
        assert!(links.is_empty());
    }

    #[test]
    fn parses_link_at_body_start_and_end() {
        // Boundary cases: link is the entire body, link opens
        // at byte 0, link closes at the final byte.
        let body = "[[id:only]]";
        let links = parse_body_links(body);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].range, 0..body.len());
    }

    #[test]
    fn no_links_returns_empty() {
        assert!(parse_body_links("").is_empty());
        assert!(parse_body_links("just prose").is_empty());
        assert!(parse_body_links("[brackets] but not link shape").is_empty());
    }

    #[test]
    fn preserves_byte_ranges_for_substring_recovery() {
        // The renderer uses `&body[link.range.clone()]` to
        // recover the literal text for display; ranges must be
        // exact byte boundaries (not char boundaries — Rust
        // strings are byte-indexed and string slicing checks
        // char boundaries on debug; the byte boundaries we
        // produce land cleanly on `]` and `[` which are ASCII).
        let body = "prefix [[id:U][label]] suffix";
        let links = parse_body_links(body);
        let span = &body[links[0].range.clone()];
        assert_eq!(span, "[[id:U][label]]");
    }
}
