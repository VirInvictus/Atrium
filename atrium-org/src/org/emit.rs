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

use super::parse::{OrgFile, OrgRepeater, OrgTask, StatisticsCookie};
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
    // v0.15.0 — statistics cookie sits between title and tags
    // per Org spec. Whitespace-separated from both sides so
    // `parse_headline`'s strip_trailing_cookie pass picks it up
    // cleanly on a re-read.
    if let Some(cookie) = task.statistics_cookie {
        out.push(' ');
        out.push_str(&render_cookie(cookie));
    }
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

    // v0.17.0 — Phase 18.5 Tier-1 :LOGBOOK: drawer. Emit only
    // when the task has at least one closed entry or any
    // logbook_unknown_lines we promised to round-trip verbatim.
    // In-progress entries (ended IS None) are deliberately
    // suppressed — the file would churn every clock-running
    // second; the next clock_out flushes the now-closed entry.
    let has_closed_entries = task.clock_entries.iter().any(|e| e.ended.is_some());
    if has_closed_entries || !task.logbook_unknown_lines.is_empty() {
        out.push_str(":LOGBOOK:\n");
        for entry in &task.clock_entries {
            let Some(ended) = entry.ended else {
                continue; // skip in-progress
            };
            // Emit in UTC so the parse-emit round-trip is
            // byte-stable. Mirrors the CLOSED-cookie convention
            // — Atrium treats Org timestamps as UTC throughout
            // the parser/emitter pair. Users in non-UTC zones
            // see UTC clock times in the file (documented
            // limitation; same as CLOSED has had since Phase 16).
            let started_naive = entry.started.naive_utc();
            let ended_naive = ended.naive_utc();
            let duration = ended.signed_duration_since(entry.started);
            let total_minutes = duration.num_minutes().max(0);
            let h = total_minutes / 60;
            let m = total_minutes % 60;
            out.push_str(&format!(
                "CLOCK: [{}]--[{}] =>  {}:{:02}",
                started_naive.format("%Y-%m-%d %a %H:%M"),
                ended_naive.format("%Y-%m-%d %a %H:%M"),
                h,
                m
            ));
            if !entry.note.is_empty() {
                out.push(' ');
                out.push_str(&entry.note);
            }
            out.push('\n');
        }
        for unknown in &task.logbook_unknown_lines {
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

    // Trailing blank line after every headline's content. Matches
    // Emacs's `org-blank-before-new-entry` default — the line is
    // *before* the next headline (or the parent's next sibling
    // when this headline has no following peer at its depth).
    // The org-agenda + Doom rendering treats the blank as part
    // of the source convention; without it adjacent headlines
    // read as a single visual block. Spec §7.3.3 round-trip
    // rules don't constrain whitespace, so the parser ignores
    // the line on read — emit and re-emit are byte-stable.
    out.push('\n');

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
        let stamp = render_active(
            date,
            task.scheduled_time,
            task.scheduled_repeater.as_ref(),
            task.scheduled_warning,
        );
        chunks.push(format!("SCHEDULED: {stamp}"));
    }
    if let Some(date) = task.deadline {
        let stamp = render_active(
            date,
            None,
            task.deadline_repeater.as_ref(),
            task.deadline_warning,
        );
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

fn render_active(
    date: chrono::NaiveDate,
    time: Option<chrono::NaiveTime>,
    repeater: Option<&OrgRepeater>,
    warning_days: Option<u32>,
) -> String {
    // Build the optional `<sp>+1w<sp>-7d` suffix run. The two
    // pieces are independent — both, either, or neither may be
    // present. Org accepts the warning before or after the
    // repeater; we always emit repeater-then-warning so byte-stable
    // round-trips have a fixed canonical order. Atrium normalises
    // the warning prefix onto `-` (single dash) since Atrium has
    // no global-default-override concept that would distinguish
    // `-` from `--`.
    let mut suffix = String::new();
    // v0.19.0 — Phase 18.5 Tier-2 time-of-day. When the
    // SCHEDULED has a time, slot it after the day name and
    // before the repeater/warning suffixes. Org's canonical
    // ordering is `<DATE Day HH:MM +Nx -Md>`.
    if let Some(t) = time {
        suffix.push(' ');
        suffix.push_str(&format!("{}", t.format("%H:%M")));
    }
    if let Some(r) = repeater {
        suffix.push(' ');
        suffix.push_str(&format!("{}{}{}", r.mode, r.interval, r.unit));
    }
    if let Some(days) = warning_days {
        suffix.push(' ');
        suffix.push_str(&format!("-{days}d"));
    }
    format!(
        "<{} {}{}>",
        date.format("%Y-%m-%d"),
        date.format("%a"),
        suffix
    )
}

/// v0.15.0 — render a statistics cookie back to its `[N/M]` or
/// `[N%]` text shape. Variant choice preserves the user's
/// original form across a round-trip; values come from whatever
/// the projection populated.
fn render_cookie(cookie: StatisticsCookie) -> String {
    match cookie {
        StatisticsCookie::Counter { done, total } => format!("[{done}/{total}]"),
        StatisticsCookie::Percent { value } => format!("[{value}%]"),
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

    // v0.14.0 — DEADLINE warning suffix round-trips through the
    // emitter unchanged. Phase 18.5 Tier-1.
    #[test]
    fn roundtrip_deadline_with_warning_suffix() {
        assert_roundtrip("* TODO File taxes\nDEADLINE: <2026-04-15 Wed -7d>\n");
    }

    #[test]
    fn roundtrip_deadline_with_repeater_and_warning() {
        // Order canonicalised on emit (repeater first, warning
        // second); both shapes parse cleanly so the round-trip is
        // stable.
        assert_roundtrip("* TODO Renew domain\nDEADLINE: <2026-12-01 Tue +1y -30d>\n");
    }

    #[test]
    fn emit_normalises_double_dash_warning_to_single_dash() {
        // Atrium has no global-default-override concept, so
        // `--Nd` and `-Nd` mean the same thing. The emitter
        // canonicalises onto `-`. After one round-trip the input's
        // `--7d` becomes `-7d`; the parsed shape stays equal.
        let input = "* TODO Audit\nDEADLINE: <2026-09-01 Tue --7d>\n";
        let first = super::super::parse::parse_org_text(input);
        let emitted = emit_org_text(&first);
        assert!(
            emitted.contains("-7d"),
            "expected canonical -7d in emitted text, got:\n{emitted}"
        );
        assert!(
            !emitted.contains("--7d"),
            "expected --7d to normalise away, got:\n{emitted}"
        );
        let second = super::super::parse::parse_org_text(&emitted);
        assert_eq!(first, second);
    }

    #[test]
    fn roundtrip_scheduled_warning_suffix_is_preserved() {
        // Atrium has no DB column for SCHEDULED-side warnings,
        // but the OrgTask round-trip preserves them verbatim.
        assert_roundtrip("* TODO Pay rent\nSCHEDULED: <2026-05-01 Fri -3d>\n");
    }

    #[test]
    fn roundtrip_headline_tags() {
        assert_roundtrip("* TODO Run errands :errand:home:\n");
    }

    // v0.15.0 — Phase 18.5 Tier-1 statistics cookies. Both shapes
    // round-trip; cookie sits between title and tags as the
    // parser expects.
    #[test]
    fn roundtrip_counter_cookie() {
        // Synthesise an OrgTask with children so the writer's
        // projection populates the cookie.
        let parent = OrgTask {
            depth: 1,
            keyword: None,
            title: "Project".to_string(),
            tags: Vec::new(),
            scheduled: None,
            scheduled_time: None,
            scheduled_repeater: None,
            scheduled_warning: None,
            deadline: None,
            deadline_repeater: None,
            deadline_warning: None,
            statistics_cookie: Some(StatisticsCookie::Counter { done: 2, total: 5 }),
            clock_entries: Vec::new(),
            logbook_unknown_lines: Vec::new(),
            closed: None,
            properties: HashMap::new(),
            body: String::new(),
            unknown_lines: Vec::new(),
            children: Vec::new(),
        };
        let text = emit_org_text(&[parent]);
        assert!(text.contains("[2/5]"), "expected [2/5] in:\n{text}");
        // Re-parse + check the cookie survives.
        let reparsed = super::super::parse::parse_org_text(&text);
        assert_eq!(
            reparsed[0].statistics_cookie,
            Some(StatisticsCookie::Counter { done: 2, total: 5 })
        );
    }

    #[test]
    fn roundtrip_percent_cookie_preserves_shape() {
        // Source-shape preservation: a `[40%]` source emits back
        // as `[N%]`, not `[N/M]`.
        let input = "* TODO Big initiative [40%]\n";
        let first = super::super::parse::parse_org_text(input);
        let emitted = emit_org_text(&first);
        assert!(emitted.contains("[40%]"), "expected [40%] in:\n{emitted}");
        let second = super::super::parse::parse_org_text(&emitted);
        assert_eq!(first, second);
    }

    // v0.17.0 — Phase 18.5 Tier-1 :LOGBOOK: + CLOCK lines.
    #[test]
    fn roundtrip_logbook_with_closed_entry() {
        let input = "\
* TODO Plan
:LOGBOOK:
CLOCK: [2026-05-15 Fri 09:00]--[2026-05-15 Fri 11:30] =>  2:30
:END:
";
        let first = super::super::parse::parse_org_text(input);
        let emitted = emit_org_text(&first);
        assert!(
            emitted.contains(":LOGBOOK:"),
            "expected :LOGBOOK: drawer in:\n{emitted}"
        );
        assert!(
            emitted.contains("CLOCK:"),
            "expected CLOCK line in:\n{emitted}"
        );
        let second = super::super::parse::parse_org_text(&emitted);
        assert_eq!(first[0].clock_entries, second[0].clock_entries);
    }

    #[test]
    fn emit_suppresses_in_progress_clock_entries() {
        // A running clock (no ended_at) doesn't emit a CLOCK
        // line. The :LOGBOOK: drawer itself is suppressed when
        // there are no closed entries to write.
        use super::super::parse::OrgClockEntry;
        let started = chrono::DateTime::parse_from_rfc3339("2026-05-15T09:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let mut task = super::super::parse::OrgTask::default_test_node(1);
        task.title = "Plan".to_string();
        task.keyword = Some(super::super::parse::OrgKeyword::Todo);
        task.clock_entries.push(OrgClockEntry {
            started,
            ended: None,
            note: String::new(),
        });
        let emitted = emit_org_text(&[task]);
        assert!(
            !emitted.contains(":LOGBOOK:"),
            "in-progress entry should suppress the whole drawer; got:\n{emitted}"
        );
        assert!(
            !emitted.contains("CLOCK:"),
            "in-progress entry should not emit a CLOCK line; got:\n{emitted}"
        );
    }

    #[test]
    fn emit_logbook_only_emits_closed_entries() {
        use super::super::parse::OrgClockEntry;
        let started_open = chrono::DateTime::parse_from_rfc3339("2026-05-16T08:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let started_closed = chrono::DateTime::parse_from_rfc3339("2026-05-15T09:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let ended_closed = chrono::DateTime::parse_from_rfc3339("2026-05-15T11:30:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let mut task = super::super::parse::OrgTask::default_test_node(1);
        task.title = "Plan".to_string();
        task.keyword = Some(super::super::parse::OrgKeyword::Todo);
        task.clock_entries.push(OrgClockEntry {
            started: started_closed,
            ended: Some(ended_closed),
            note: String::new(),
        });
        task.clock_entries.push(OrgClockEntry {
            started: started_open,
            ended: None,
            note: String::new(),
        });
        let emitted = emit_org_text(&[task]);
        // Drawer present (closed entry exists).
        assert!(emitted.contains(":LOGBOOK:"));
        // Exactly one CLOCK line — the open one was suppressed.
        assert_eq!(
            emitted.matches("CLOCK:").count(),
            1,
            "expected exactly one CLOCK line; got:\n{emitted}"
        );
    }

    // v0.19.0 — Phase 18.5 Tier-2 SCHEDULED time-of-day emit.
    #[test]
    fn roundtrip_scheduled_with_time() {
        let input = "* TODO Standup\nSCHEDULED: <2026-05-15 Fri 09:00>\n";
        let first = super::super::parse::parse_org_text(input);
        let emitted = emit_org_text(&first);
        assert!(emitted.contains("09:00"), "expected 09:00 in:\n{emitted}");
        let second = super::super::parse::parse_org_text(&emitted);
        assert_eq!(first[0].scheduled_time, second[0].scheduled_time);
    }

    #[test]
    fn roundtrip_scheduled_with_time_and_repeater() {
        let input = "* TODO Daily\nSCHEDULED: <2026-05-15 Fri 09:00 +1d>\n";
        let first = super::super::parse::parse_org_text(input);
        let emitted = emit_org_text(&first);
        // Both pieces present in the canonical order: time
        // before repeater.
        assert!(
            emitted.contains("09:00 +1d"),
            "expected `09:00 +1d` in:\n{emitted}"
        );
        let second = super::super::parse::parse_org_text(&emitted);
        assert_eq!(first[0].scheduled_time, second[0].scheduled_time);
        assert_eq!(first[0].scheduled_repeater, second[0].scheduled_repeater);
    }

    #[test]
    fn roundtrip_cookie_with_tags() {
        let input = "* TODO Project :work:focus:\n";
        let mut parsed = super::super::parse::parse_org_text(input);
        parsed[0].statistics_cookie = Some(StatisticsCookie::Counter { done: 1, total: 3 });
        let emitted = emit_org_text(&parsed);
        // Cookie should sit between title and tags.
        assert!(
            emitted.contains("Project [1/3] :work:focus:"),
            "expected `Project [1/3] :work:focus:` in:\n{emitted}"
        );
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
        // Headline + the `org-blank-before-new-entry` trailing
        // blank we now emit between every headline and whatever
        // follows. No cookie / properties / body lines are
        // present, which is what this test pins.
        assert_eq!(out, "* TODO Plain\n\n");
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
