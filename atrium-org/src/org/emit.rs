// SPDX-License-Identifier: MIT
//! Org-mode emitter — `Vec<OrgTask>` → text. Pairs with the
//! parser in [`super::parse`] to satisfy the round-trip
//! discipline of spec §7.3.3.
//!
//! Output shape per task:
//!
//! ```text
//! ** KEYWORD Title :tag1:tag2:
//! SCHEDULED: <2026-05-15 Fri ++1w> DEADLINE: <2026-06-01 Mon> CLOSED: [2026-05-10 Sun 14:30]
//! :PROPERTIES:
//! :ID: …
//! :CREATED: …
//! :END:
//! Body text preserved verbatim.
//! ```
//!
//! Children render at `depth + 1` immediately after the parent's
//! body. The cookie line, properties drawer, and body each emit
//! only when there's something to write — empty fields produce no
//! line at all so we don't bloat output with placeholders.
//!
//! **Round-trip rule.** `parse_org_text(emit_org_text(parse_org_text(x))) == parse_org_text(x)`
//! holds for every spec §7.3 construct the parser recognises. The
//! emitter renders dates with the abbreviated day name (`%Y-%m-%d %a`)
//! because that's what Emacs writes; on parse we ignore the day name
//! anyway, so emitting it doesn't perturb the parsed shape.

use std::io;
use std::path::Path;

use super::parse::{OrgFile, OrgRepeater, OrgTask};
use atrium_core::sync::atomic::write_atomic;

/// Emit a tree of `OrgTask` values back to Org text. No preamble
/// — use [`emit_org_text_with_meta`] when the caller has
/// `#+TITLE:` / file-level properties to surface.
pub fn emit_org_text(tasks: &[OrgTask]) -> String {
    let mut out = String::new();
    for task in tasks {
        emit_task(task, &mut out);
    }
    out
}

/// Emit an `OrgFile` (preamble + headlines) back to Org text.
/// Directives render as `#+KEY: value` lines (sorted for stable
/// output); the file-level `:PROPERTIES:` block follows when
/// non-empty; a blank line separates preamble from the first
/// headline. Callers that want only the legacy headline-stream
/// shape pass through [`emit_org_text`].
pub fn emit_org_text_with_meta(file: &OrgFile) -> String {
    let mut out = String::new();

    // #+DIRECTIVES first. Sort keys so HashMap iteration order
    // doesn't rotate the preamble on each emit.
    let mut directive_keys: Vec<&String> = file.directives.keys().collect();
    directive_keys.sort();
    for key in directive_keys {
        let value = &file.directives[key];
        out.push_str("#+");
        out.push_str(key);
        out.push(':');
        if !value.is_empty() {
            out.push(' ');
            out.push_str(value);
        }
        out.push('\n');
    }

    // File-level :PROPERTIES: block. Same sorted-keys discipline
    // as the headline-attached drawer in `emit_task`.
    if !file.file_properties.is_empty() {
        out.push_str(":PROPERTIES:\n");
        let mut keys: Vec<&String> = file.file_properties.keys().collect();
        keys.sort();
        for key in keys {
            let value = &file.file_properties[key];
            out.push(':');
            out.push_str(key);
            out.push(':');
            if !value.is_empty() {
                out.push(' ');
                out.push_str(value);
            }
            out.push('\n');
        }
        out.push_str(":END:\n");
    }

    // Blank line between preamble and the first headline so the
    // file is human-readable. Only emit when both preamble and
    // headlines are present.
    let had_preamble = !file.directives.is_empty() || !file.file_properties.is_empty();
    if had_preamble && !file.headlines.is_empty() {
        out.push('\n');
    }

    for task in &file.headlines {
        emit_task(task, &mut out);
    }
    out
}

/// Atomically write a `Vec<OrgTask>` tree to `path`. Goes
/// through [`atrium_core::sync::atomic::write_atomic`] so a crash
/// mid-write never corrupts the destination (spec §7.3.3 rule 6).
pub fn emit_org_file(path: &Path, tasks: &[OrgTask]) -> io::Result<()> {
    let text = emit_org_text(tasks);
    write_atomic(path, text.as_bytes())
}

/// Atomically write an `OrgFile` (preamble + headlines) to
/// `path`. Goes through the same `write_atomic` helper as
/// [`emit_org_file`].
///
/// Runs a post-write integrity check (per spec §7.3.3: "newly-
/// written file parses cleanly with Atrium's own reader"). After
/// the atomic rename, the file is re-read and parsed; if parsing
/// fails, the function returns an `io::Error::Other` describing
/// the divergence. Logs a `tracing::warn` so the failure is
/// visible even if the caller swallows the error.
///
/// Rollback to a `.atrium.bak.<timestamp>` (spec §7.3.3 rule 5)
/// is a sibling concern that lives with the auto-debounced
/// worker write hook in `atrium_org::vault_writer`; the hook
/// owns recovery decisions. For now an integrity failure still
/// leaves the just-written (possibly questionable) file on
/// disk; the `Err` lets the caller decide whether to surface a
/// toast, retry, or quietly accept the file.
pub fn emit_org_file_with_meta(path: &Path, file: &OrgFile) -> io::Result<()> {
    let text = emit_org_text_with_meta(file);
    write_atomic(path, text.as_bytes())?;
    verify_emitted_file(path)
}

/// Re-parse the file we just wrote. Returns the parse failure
/// as an `io::Error::Other` when the parser rejects the file
/// outright. (The hand-rolled parser is permissive — any
/// unrecognised line lands in body or unknown_lines — so
/// "rejects" in practice means an `io::Error` from the read
/// itself, e.g. a permission flip mid-write.)
fn verify_emitted_file(path: &Path) -> io::Result<()> {
    match super::parse::parse_org_file_with_meta(path) {
        Ok(_) => Ok(()),
        Err(e) => {
            tracing::warn!(
                path = %path.display(),
                error = %e,
                "post-write Org integrity check failed; file remains on disk"
            );
            Err(io::Error::other(format!(
                "post-write integrity check failed for {}: {e}",
                path.display()
            )))
        }
    }
}

fn emit_task(task: &OrgTask, out: &mut String) {
    // Headline line.
    out.push_str(&"*".repeat(task.depth));
    out.push(' ');
    if let Some(kw) = &task.keyword {
        out.push_str(kw.as_str());
        out.push(' ');
    }
    out.push_str(&task.title);
    if !task.tags.is_empty() {
        out.push(' ');
        out.push(':');
        for tag in &task.tags {
            out.push_str(tag);
            out.push(':');
        }
    }
    out.push('\n');

    // Cookie line (SCHEDULED / DEADLINE / CLOSED). Emit only the
    // cookies that are present, separated by single spaces.
    let cookie_chunks = render_cookies(task);
    if !cookie_chunks.is_empty() {
        out.push_str(&cookie_chunks.join(" "));
        out.push('\n');
    }

    // :PROPERTIES: drawer. We emit it whenever we have at least
    // one property or any unknown_lines that originated inside
    // the drawer (the parser captures malformed property entries
    // into unknown_lines on the assumption they came from inside
    // the drawer; v0.7.7 doesn't distinguish other sources). If a
    // future patch starts using unknown_lines for non-drawer
    // origins the emitter will need to split them.
    if !task.properties.is_empty() || !task.unknown_lines.is_empty() {
        out.push_str(":PROPERTIES:\n");
        // Stable iteration: collect keys + sort by name so
        // round-trips don't reorder unrelated properties on each
        // emit. The HashMap iteration order is otherwise random.
        let mut keys: Vec<&String> = task.properties.keys().collect();
        keys.sort();
        for key in keys {
            let value = &task.properties[key];
            out.push(':');
            out.push_str(key);
            out.push(':');
            if !value.is_empty() {
                out.push(' ');
                out.push_str(value);
            }
            out.push('\n');
        }
        for unknown in &task.unknown_lines {
            out.push_str(unknown);
            out.push('\n');
        }
        out.push_str(":END:\n");
    }

    // Body. Already stored without the trailing newline (parser
    // strips it on read); we add one here to terminate.
    if !task.body.is_empty() {
        out.push_str(&task.body);
        out.push('\n');
    }

    // Children at depth + 1. Each child re-enters this function
    // and emits its own headline + content recursively.
    for child in &task.children {
        emit_task(child, out);
    }
}

/// Render the cookie line components for a task. Returns the
/// individual `SCHEDULED: <…>` / `DEADLINE: <…>` / `CLOSED: […]`
/// chunks; the caller joins them with single spaces.
fn render_cookies(task: &OrgTask) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    if let Some(date) = task.scheduled {
        let stamp = render_active(date, task.scheduled_repeater.as_ref());
        chunks.push(format!("SCHEDULED: {stamp}"));
    }
    if let Some(date) = task.deadline {
        let stamp = render_active(date, task.deadline_repeater.as_ref());
        chunks.push(format!("DEADLINE: {stamp}"));
    }
    if let Some(closed) = task.closed {
        // CLOSED uses an inactive timestamp [...]. We emit the
        // time-of-day if it isn't the parser's noon-UTC default,
        // since that's how Emacs writes it.
        let date = closed.date_naive();
        let day = date.format("%a");
        let time = closed.time();
        let chunk = if time.hour() == 12 && time.minute() == 0 && time.second() == 0 {
            format!("CLOSED: [{} {}]", date.format("%Y-%m-%d"), day)
        } else {
            format!(
                "CLOSED: [{} {} {}]",
                date.format("%Y-%m-%d"),
                day,
                time.format("%H:%M")
            )
        };
        chunks.push(chunk);
    }
    chunks
}

fn render_active(date: chrono::NaiveDate, repeater: Option<&OrgRepeater>) -> String {
    let day = date.format("%a");
    match repeater {
        Some(r) => format!(
            "<{} {} {}{}{}>",
            date.format("%Y-%m-%d"),
            day,
            r.mode,
            r.interval,
            r.unit
        ),
        None => format!("<{} {}>", date.format("%Y-%m-%d"), day),
    }
}

// chrono::NaiveTime exposes hour/minute/second through the
// Timelike trait. Pull it in for the closed-cookie rendering above.
use chrono::Timelike;

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::super::parse::{OrgKeyword, parse_org_text};
    use super::*;

    /// Assert the emit/parse round-trip is stable: parsing a text,
    /// emitting it, then parsing again should produce the same
    /// task tree. We compare the parsed-tree shape rather than the
    /// raw text because emit may canonicalise whitespace, ordering,
    /// or day-name formatting that the parser tolerates.
    fn assert_roundtrip(input: &str) {
        let first = parse_org_text(input);
        let emitted = emit_org_text(&first);
        let second = parse_org_text(&emitted);
        assert_eq!(
            first, second,
            "round-trip differs.\noriginal:\n{input}\nemitted:\n{emitted}"
        );
    }

    #[test]
    fn roundtrip_simple_todo() {
        assert_roundtrip("* TODO Email João\n");
    }

    #[test]
    fn roundtrip_done_with_closed() {
        assert_roundtrip(
            "\
* DONE Audit the schema
CLOSED: [2026-05-01 Fri 14:30]
",
        );
    }

    #[test]
    fn roundtrip_scheduled_and_deadline() {
        assert_roundtrip(
            "\
* TODO Plan Q3
SCHEDULED: <2026-05-15 Fri> DEADLINE: <2026-06-01 Mon>
",
        );
    }

    #[test]
    fn roundtrip_repeater_modes() {
        assert_roundtrip("* TODO Daily\nSCHEDULED: <2026-05-15 Fri +1d>\n");
        assert_roundtrip("* TODO Cumulative\nSCHEDULED: <2026-05-15 Fri ++1w>\n");
        assert_roundtrip("* TODO Next-from-completion\nSCHEDULED: <2026-05-15 Fri .+2w>\n");
    }

    #[test]
    fn roundtrip_headline_tags() {
        assert_roundtrip("* TODO Run errands :errand:home:\n");
    }

    #[test]
    fn roundtrip_properties() {
        assert_roundtrip(
            "\
* TODO With Properties
:PROPERTIES:
:ID: abc-123
:CREATED: [2026-04-01 Wed]
:EFFORT: 0:30
:END:
",
        );
    }

    #[test]
    fn roundtrip_body_verbatim() {
        assert_roundtrip(
            "\
* TODO Brainstorm
Some prose body.

  - bullet 1
  - bullet 2

#+BEGIN_SRC rust
fn foo() {}
#+END_SRC
",
        );
    }

    #[test]
    fn roundtrip_nested_subtasks() {
        assert_roundtrip(
            "\
* TODO Parent
** TODO Child A
** TODO Child B
*** TODO Grandchild
* TODO Sibling
",
        );
    }

    #[test]
    fn roundtrip_project_subheading_no_keyword() {
        assert_roundtrip("* Backlog\n** TODO Real task\n");
    }

    #[test]
    fn roundtrip_custom_keyword() {
        assert_roundtrip("* WAITING External signoff\n");
    }

    #[test]
    fn roundtrip_unknown_lines_in_properties() {
        assert_roundtrip(
            "\
* TODO Thing
:PROPERTIES:
:ID: abc
this is not a property line
:END:
",
        );
    }

    #[test]
    fn roundtrip_kitchen_sink() {
        // Combines every supported feature in one document. If
        // anything regresses, this is the canary.
        assert_roundtrip(
            "\
* Q3 Backlog
** TODO Email João :work:
SCHEDULED: <2026-05-15 Fri ++1w> DEADLINE: <2026-05-22 Fri>
:PROPERTIES:
:ID: 9c2f9c0e-1a1b-44e2-9f9c-0e1a1b44e29f
:CREATED: [2026-04-01 Wed]
:EFFORT: 0:45
:END:
Need to follow up on the contract terms.

Open questions:
  - Pricing tier
  - Renewal date
** DONE Refactor the dashboard :work:refactor:
CLOSED: [2026-05-08 Fri 14:22]
:PROPERTIES:
:ID: another-uuid
:END:
*** DONE Subtask one
CLOSED: [2026-05-07 Thu]
*** DONE Subtask two
CLOSED: [2026-05-08 Fri 09:00]
* TODO Sibling project audit
",
        );
    }

    #[test]
    fn emit_uses_canonical_keyword_order() {
        let tasks = parse_org_text("* TODO First\n* DONE Second\n* CANCELLED Third\n");
        let out = emit_org_text(&tasks);
        // All three keywords render correctly.
        assert!(out.contains("* TODO First\n"));
        assert!(out.contains("* DONE Second\n"));
        assert!(out.contains("* CANCELLED Third\n"));
    }

    #[test]
    fn emit_writes_properties_in_sorted_order() {
        // HashMap iteration order is otherwise random; we sort
        // keys for stable round-trips. Verify by checking the
        // output position of two known keys.
        let mut tasks = parse_org_text(
            "\
* TODO Sample
:PROPERTIES:
:ZEBRA: z
:APPLE: a
:MIDDLE: m
:END:
",
        );
        // Force a re-emit so we know we're not just echoing the
        // order from the input.
        let emitted = emit_org_text(&tasks);
        let apple_pos = emitted.find(":APPLE:").unwrap();
        let middle_pos = emitted.find(":MIDDLE:").unwrap();
        let zebra_pos = emitted.find(":ZEBRA:").unwrap();
        assert!(apple_pos < middle_pos);
        assert!(middle_pos < zebra_pos);

        // Mutate the parsed task and re-emit; order should still
        // be stable.
        tasks[0].properties.insert("BANANA".into(), "b".into());
        let emitted2 = emit_org_text(&tasks);
        let apple = emitted2.find(":APPLE:").unwrap();
        let banana = emitted2.find(":BANANA:").unwrap();
        let middle = emitted2.find(":MIDDLE:").unwrap();
        let zebra = emitted2.find(":ZEBRA:").unwrap();
        assert!(apple < banana);
        assert!(banana < middle);
        assert!(middle < zebra);
    }

    #[test]
    fn emit_skips_empty_property_value() {
        // A property with an empty string value should emit just
        // `:KEY:` with no trailing space (Org-canonical form for
        // present-but-empty properties).
        let mut tasks = parse_org_text("* TODO Sample\n");
        tasks[0].properties.insert("FLAG".into(), String::new());
        let out = emit_org_text(&tasks);
        assert!(out.contains(":FLAG:\n"));
        assert!(!out.contains(":FLAG: \n"));
    }

    #[test]
    fn emit_no_drawer_when_no_properties() {
        // No properties + no unknown_lines → no :PROPERTIES: drawer.
        let tasks = parse_org_text("* TODO Plain\n");
        let out = emit_org_text(&tasks);
        assert!(!out.contains(":PROPERTIES:"));
        assert!(!out.contains(":END:"));
    }

    #[test]
    fn emit_no_cookie_line_when_no_dates() {
        let tasks = parse_org_text("* TODO Plain\n");
        let out = emit_org_text(&tasks);
        // The only newline-terminated line should be the headline.
        assert_eq!(out, "* TODO Plain\n");
    }

    #[test]
    fn emit_keyword_as_str_round_trips() {
        // Sanity check the OrgKeyword::as_str() table.
        assert_eq!(OrgKeyword::Todo.as_str(), "TODO");
        assert_eq!(OrgKeyword::Done.as_str(), "DONE");
        assert_eq!(OrgKeyword::Cancelled.as_str(), "CANCELLED");
        assert_eq!(
            OrgKeyword::Custom("WAITING".to_string()).as_str(),
            "WAITING"
        );
    }

    #[test]
    fn roundtrip_with_meta_directives_and_file_properties() {
        // file-level metadata round-trips through the
        // new with_meta path: parse → emit → re-parse and check
        // the directives / file_properties / headlines come back
        // equal.
        use super::super::parse::parse_org_text_with_meta;

        let input = "\
#+CATEGORY: work
#+TITLE: Q3 Plans
:PROPERTIES:
:REVIEW_INTERVAL: 14
:SEQUENTIAL: t
:END:

* TODO Real headline
:PROPERTIES:
:ID: per-task-uuid
:END:
";
        let first = parse_org_text_with_meta(input);
        let emitted = emit_org_text_with_meta(&first);
        let second = parse_org_text_with_meta(&emitted);
        assert_eq!(
            first.directives, second.directives,
            "directives drifted on round-trip\nemitted:\n{emitted}"
        );
        assert_eq!(
            first.file_properties, second.file_properties,
            "file_properties drifted on round-trip\nemitted:\n{emitted}"
        );
        assert_eq!(
            first.headlines, second.headlines,
            "headlines drifted on round-trip\nemitted:\n{emitted}"
        );
    }

    #[test]
    fn emit_with_meta_writes_no_blank_line_when_no_preamble() {
        // No directives, no file properties → output should
        // start directly with the headline (no leading blank
        // line that the parser would tolerate but a human
        // reader would call ugly).
        use super::super::parse::OrgFile;

        let file = OrgFile {
            directives: HashMap::new(),
            file_properties: HashMap::new(),
            headlines: parse_org_text("* TODO Plain\n"),
        };
        let emitted = emit_org_text_with_meta(&file);
        assert!(emitted.starts_with("* TODO Plain"), "got: {emitted}");
    }

    #[test]
    fn emit_with_meta_writes_a_file_that_parses_back() {
        // post-write integrity check is wired into
        // emit_org_file_with_meta. A round-trip-eligible OrgFile
        // should write and verify cleanly. This test just makes
        // sure the success path doesn't surface a spurious
        // integrity error.
        use super::super::parse::{OrgFile, parse_org_file_with_meta};

        let dir =
            std::env::temp_dir().join(format!("atrium-emit-integrity-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("ok.org");

        let mut directives = HashMap::new();
        directives.insert("TITLE".to_string(), "Round-trip OK".to_string());
        let file = OrgFile {
            directives,
            file_properties: HashMap::new(),
            headlines: parse_org_text("* TODO Sample\n"),
        };
        emit_org_file_with_meta(&path, &file).expect("integrity check should pass");

        // Sanity: the file is on disk and parses to the same
        // shape we wrote.
        let parsed = parse_org_file_with_meta(&path).unwrap();
        assert_eq!(
            parsed.directives.get("TITLE").map(String::as_str),
            Some("Round-trip OK")
        );
        assert_eq!(parsed.headlines.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn emit_with_meta_writes_blank_line_between_preamble_and_first_headline() {
        // With preamble, a blank line separates it from the
        // first headline so the file reads cleanly in Emacs.
        use super::super::parse::OrgFile;

        let mut directives = HashMap::new();
        directives.insert("TITLE".to_string(), "Q3 Plans".to_string());
        let file = OrgFile {
            directives,
            file_properties: HashMap::new(),
            headlines: parse_org_text("* TODO Plain\n"),
        };
        let emitted = emit_org_text_with_meta(&file);
        assert!(
            emitted.contains("#+TITLE: Q3 Plans\n\n* TODO Plain\n"),
            "got: {emitted}"
        );
    }
}
