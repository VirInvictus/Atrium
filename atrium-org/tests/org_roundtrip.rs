// SPDX-License-Identifier: MIT
//! Phase 16 round-trip test fixture (v0.7.17).
//!
//! For each `.org` fixture under `tests/fixtures/org/`, the test
//! harness:
//!
//! 1. Parses the source file with `parse_org_text_with_meta`
//!    so we have a canonical `OrgFile` AST to compare against.
//! 2. Imports the file through the worker (the same path
//!    `atrium-cli import org` takes). This lands rows in the DB.
//! 3. Exports the resulting project back through
//!    `write_project_to_vault`. The atomic-write helper +
//!    post-write integrity check from v0.7.15 fire here.
//! 4. Parses the regenerated file.
//! 5. Asserts AST equality between source and regenerated trees.
//!
//! Comparison is on the **parsed** shapes, not raw text — the
//! emitter canonicalises whitespace, sorts properties, and
//! always emits abbreviated day names in timestamps. Those
//! canonicalisations are round-trip-safe by design (the parser
//! tolerates either form), so comparing post-parse trees catches
//! semantic drift while ignoring cosmetic formatting.
//!
//! Drift the harness DOES catch:
//! - Task title text
//! - TODO keyword (including non-canonical via `orig_keyword`)
//! - Headline tags
//! - SCHEDULED / DEADLINE / CLOSED dates
//! - Repeater suffixes
//! - `:PROPERTIES:` keys + values (the harness compares
//!   HashMaps, not iteration order)
//! - Body content
//! - Subtask hierarchy
//! - File-level `#+TITLE:` and `:PROPERTIES:` block
//!
//! Drift the harness IGNORES intentionally:
//! - `:CREATED:` / `:MODIFIED:` properties on tasks (the
//!   schema auto-stamps these on insert; they won't match the
//!   source file's values).
//! - Timestamp-of-day on the `closed` cookie when the source
//!   wrote a date-only `[YYYY-MM-DD ...]` (the v0.7.9
//!   importer's TODO-toggle path stamps `now()` for completion;
//!   v0.7.10+'s caller-provided completed_at will tighten this
//!   over time).
//!
//! Each fixture surfaces a different combination of features so
//! a regression in any one spec §7.3 construct fails its
//! dedicated test rather than getting hidden behind an
//! unrelated kitchen-sink failure.

use std::collections::HashMap;
use std::path::PathBuf;

use atrium_org::org::{
    OrgFile, OrgKeyword, OrgTask, import_org_file, parse_org_file_with_meta, write_project_to_vault,
};

/// Common harness body for a fixture round-trip. Returns
/// `(source_parse, regenerated_parse)` so individual tests can
/// run targeted assertions if they need to (the default
/// `assert_round_trip` runs the full equality check).
async fn round_trip_through_db(label: &str, fixture: &str) -> (OrgFile, OrgFile) {
    let scratch = std::env::temp_dir().join(format!("atrium-rt-{}-{}", label, std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    let src_path = scratch.join("source.org");
    std::fs::write(&src_path, fixture).unwrap();

    // Set up a fresh file-backed DB for the importer. atrium_core::db::open
    // is the canonical public entry — runs pragmas + migrations.
    let db_path = scratch.join("atrium.db");
    let writer_conn = atrium_core::db::open(&db_path).unwrap();
    let read_conn = atrium_core::db::open(&db_path).unwrap();

    let (handle, _changes_rx, _library_rx) = atrium_core::spawn_worker(writer_conn);

    // Step 1 + 2: parse source, import the file.
    let source_parse = parse_org_file_with_meta(&src_path).unwrap();
    let summary = import_org_file(&handle, &src_path, false).await.unwrap();
    let project_id = summary.project_id.expect("import returned no project_id");

    // Step 3: export the project back to a separate path so we
    // don't read our own freshly-imported source.
    let export_dir = scratch.join("export");
    std::fs::create_dir_all(&export_dir).unwrap();
    let written = write_project_to_vault(&read_conn, &export_dir, project_id).unwrap();

    // Step 4: re-parse the regenerated file.
    let regenerated_parse = parse_org_file_with_meta(&written.file_path).unwrap();

    let _ = std::fs::remove_dir_all(&scratch);
    (source_parse, regenerated_parse)
}

/// Strip fields the round-trip won't preserve so the comparison
/// focuses on what spec §7.3 actually contracts. See module
/// docs for the rationale on each exclusion.
///
/// Mutually-paired between source + regenerated: a normalisation
/// applied on both sides ensures the comparison is fair. When a
/// normalisation depends on what the source had (e.g.
/// "regenerated added an :ID: because the source didn't have
/// one"), call the helper that takes both sides.
fn normalise_pair_for_comparison(source: &mut OrgFile, regenerated: &mut OrgFile) {
    // File-level :CREATED:/:MODIFIED: aren't part of the v0.7.13
    // mapping — the parser captures them but the writer drops
    // them. Strip on both sides.
    for f in [&mut *source, &mut *regenerated] {
        f.file_properties.remove("CREATED");
        f.file_properties.remove("MODIFIED");
    }

    // Spec §7.3.3 rule 2: when a project / task lacks `:ID:` on
    // import, the round-trip adds one. Treat "source has no ID,
    // regenerated has one" as round-trip-compliant by stripping
    // the regenerated side. When the source DID specify an ID,
    // keep both so the assertion catches drift.
    if !source.file_properties.contains_key("ID") {
        regenerated.file_properties.remove("ID");
    }

    // The vault writer emits `#+TITLE: <project-title>` on every
    // file, sourcing the title from the imported project's name.
    // When the source fixture didn't carry a `#+TITLE:` directive,
    // the importer falls back to the file's stem (e.g. `source`
    // for `source.org`), and the round-trip "adds" a directive
    // that wasn't in the source. Same pattern as the ID strip.
    if !source.directives.contains_key("TITLE") {
        regenerated.directives.remove("TITLE");
    }

    normalise_headlines_pair(&mut source.headlines, &mut regenerated.headlines);
}

fn normalise_headlines_pair(source: &mut [OrgTask], regenerated: &mut [OrgTask]) {
    for (src, regen) in source.iter_mut().zip(regenerated.iter_mut()) {
        normalise_headline_pair(src, regen);
    }
}

fn normalise_headline_pair(src: &mut OrgTask, regen: &mut OrgTask) {
    // Same paired-strip discipline as file_properties.
    for t in [&mut *src, &mut *regen] {
        t.properties.remove("CREATED");
        t.properties.remove("MODIFIED");
    }
    if !src.properties.contains_key("ID") {
        regen.properties.remove("ID");
    }

    // Tags are conceptually a set; the writer's order depends on
    // tag.id insertion order. Sort both sides so order doesn't
    // trigger a false mismatch.
    src.tags.sort();
    regen.tags.sort();

    // The v0.7.9 importer's completion path stamps the completed
    // task with `now()` rather than the CLOSED cookie's exact
    // time. Compare the closed date only (clear time-of-day).
    for t in [&mut *src, &mut *regen] {
        if let Some(closed) = t.closed {
            let date = closed.date_naive();
            t.closed = Some(date.and_hms_opt(0, 0, 0).unwrap().and_utc());
        }
    }

    normalise_headlines_pair(&mut src.children, &mut regen.children);
}

/// Diff helper that surfaces the FIRST headline divergence with
/// rich detail. assert_eq!'s default debug print on a deep tree
/// is a wall of text; this narrows in.
fn assert_round_trip_eq(source: &OrgFile, regenerated: &OrgFile, label: &str) {
    if source.directives != regenerated.directives {
        panic!(
            "[{label}] file directives diverged\n  source:      {:?}\n  regenerated: {:?}",
            source.directives, regenerated.directives
        );
    }
    if source.file_properties != regenerated.file_properties {
        panic!(
            "[{label}] file_properties diverged\n  source:      {:?}\n  regenerated: {:?}",
            source.file_properties, regenerated.file_properties
        );
    }
    assert_headlines_eq(&source.headlines, &regenerated.headlines, label, &[]);
}

fn assert_headlines_eq(source: &[OrgTask], regenerated: &[OrgTask], label: &str, path: &[usize]) {
    if source.len() != regenerated.len() {
        panic!(
            "[{label}] headline count differs at path {path:?}: source={} regenerated={}",
            source.len(),
            regenerated.len()
        );
    }
    for (i, (src, regen)) in source.iter().zip(regenerated.iter()).enumerate() {
        let here: Vec<usize> = path.iter().copied().chain(std::iter::once(i)).collect();
        assert_field_eq("title", &src.title, &regen.title, label, &here);
        assert_field_eq(
            "keyword",
            &keyword_str(&src.keyword),
            &keyword_str(&regen.keyword),
            label,
            &here,
        );
        assert_field_eq("tags", &src.tags, &regen.tags, label, &here);
        assert_field_eq("scheduled", &src.scheduled, &regen.scheduled, label, &here);
        assert_field_eq("deadline", &src.deadline, &regen.deadline, label, &here);
        assert_field_eq("closed", &src.closed, &regen.closed, label, &here);
        assert_field_eq("body", &src.body, &regen.body, label, &here);
        assert_field_eq(
            "properties",
            &sort_props(&src.properties),
            &sort_props(&regen.properties),
            label,
            &here,
        );
        assert_headlines_eq(&src.children, &regen.children, label, &here);
    }
}

fn assert_field_eq<T: std::fmt::Debug + PartialEq>(
    field: &str,
    src: &T,
    regen: &T,
    label: &str,
    path: &[usize],
) {
    if src != regen {
        panic!(
            "[{label}] headline path {path:?}: field `{field}` diverged\n  source:      {src:?}\n  regenerated: {regen:?}"
        );
    }
}

fn keyword_str(kw: &Option<OrgKeyword>) -> String {
    kw.as_ref()
        .map(|k| k.as_str().to_string())
        .unwrap_or_default()
}

fn sort_props(props: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = props
        .iter()
        .map(|(k, val)| (k.clone(), val.clone()))
        .collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

async fn assert_fixture_round_trips(label: &str, fixture: &str) {
    let (mut source, mut regenerated) = round_trip_through_db(label, fixture).await;
    normalise_pair_for_comparison(&mut source, &mut regenerated);
    assert_round_trip_eq(&source, &regenerated, label);
}

#[tokio::test]
async fn fixture_kitchen_sink() {
    let fixture = include_str!("fixtures/org/kitchen_sink.org");
    assert_fixture_round_trips("kitchen_sink", fixture).await;
}

#[tokio::test]
async fn fixture_custom_keywords() {
    let fixture = include_str!("fixtures/org/custom_keywords.org");
    assert_fixture_round_trips("custom_keywords", fixture).await;
}

#[tokio::test]
async fn fixture_deep_nesting() {
    let fixture = include_str!("fixtures/org/deep_nesting.org");
    assert_fixture_round_trips("deep_nesting", fixture).await;
}

#[tokio::test]
async fn fixture_project_metadata() {
    let fixture = include_str!("fixtures/org/project_metadata.org");
    assert_fixture_round_trips("project_metadata", fixture).await;
}

#[tokio::test]
async fn fixture_unicode() {
    let fixture = include_str!("fixtures/org/unicode.org");
    assert_fixture_round_trips("unicode", fixture).await;
}

#[tokio::test]
async fn fixture_rrule_patterns() {
    // Phase 17 / spec §7.3.3 rule 3 — :RRULE: is canonical, the
    // SCHEDULED cookie is best-fit. The fixture covers the three
    // migration cases from the roadmap entry plus a daily
    // interval: weekly single-day (lossless cookie), weekly
    // multi-day (lossy cookie), monthly day-of-month (lossy
    // cookie), daily INTERVAL=3 (lossless). All four round-trip
    // through Atrium with the canonical :RRULE: preserved
    // verbatim in the property drawer.
    let fixture = include_str!("fixtures/org/rrule_patterns.org");
    assert_fixture_round_trips("rrule_patterns", fixture).await;
}

// ════════════════════════════════════════════════════════════════
// Comprehensive coverage — the extent of Atrium's Org conversion.
//
// The fixture-driven tests above pin the broad strokes via AST-
// equality. The block below uses small, focused inline fixtures
// to assert specific Org constructs survive — each test names
// the construct it exercises so a regression points at the exact
// feature that broke. Reading these top-to-bottom doubles as
// a documentation map of what Atrium handles end-to-end.
//
// Constructs covered:
//
//   1.  every combination of SCHEDULED / DEADLINE / CLOSED cookies
//   2.  the three Org repeater modes (`+1w` / `++1w` / `.+1w`)
//   3.  CLOSED cookies with a time-of-day component
//   4.  multi-tag headlines (the headline `:tag1:tag2:tag3:` slot)
//   5.  custom property drawer keys (EFFORT, DEFER_UNTIL, ad-hoc)
//   6.  body content that contains other Org constructs
//       (source blocks, tables, lists, links)
//   7.  empty / minimal headlines (just `* TODO Title`)
//   8.  every TODO-cycle keyword + non-canonical keywords via
//       orig_keyword (WAITING / IN-PROGRESS / BLOCKED)
//
// Known limit (documented as its own test):
//
//   9.  the Org *importer* doesn't yet ingest depth-1
//       keyword-less sub-headings into the `heading` table —
//       the v0.12.0 heading-emit work was writer-side only.
//       The test pins the asymmetry so a future patch closing
//       the loop has a clear regression target.
// ════════════════════════════════════════════════════════════════

#[tokio::test]
async fn comprehensive_cookies_all_combinations() {
    // Eight tasks covering every subset of {SCHEDULED, DEADLINE,
    // CLOSED}: the empty set + the three singletons + the three
    // pairs + all three. Each task is named so a divergence
    // shows which combination broke. Stock Emacs concatenates
    // multiple cookies onto one "planning line" and Atrium
    // matches; the parser tolerates either form on read so the
    // round-trip is structural, not byte-exact.
    let fixture = "\
* TODO No cookies
:PROPERTIES:
:ID: aaaaaaaa-0000-0000-0000-000000000000
:END:

* TODO Only SCHEDULED
SCHEDULED: <2026-05-15 Fri>
:PROPERTIES:
:ID: aaaaaaaa-0000-0000-0000-000000000001
:END:

* TODO Only DEADLINE
DEADLINE: <2026-06-01 Mon>
:PROPERTIES:
:ID: aaaaaaaa-0000-0000-0000-000000000002
:END:

* DONE Only CLOSED
CLOSED: [2026-05-08 Fri]
:PROPERTIES:
:ID: aaaaaaaa-0000-0000-0000-000000000003
:END:

* TODO SCHEDULED and DEADLINE
SCHEDULED: <2026-05-15 Fri> DEADLINE: <2026-06-01 Mon>
:PROPERTIES:
:ID: aaaaaaaa-0000-0000-0000-000000000004
:END:

* DONE SCHEDULED and CLOSED
SCHEDULED: <2026-05-15 Fri> CLOSED: [2026-05-16 Sat]
:PROPERTIES:
:ID: aaaaaaaa-0000-0000-0000-000000000005
:END:

* DONE DEADLINE and CLOSED
DEADLINE: <2026-06-01 Mon> CLOSED: [2026-05-30 Sat]
:PROPERTIES:
:ID: aaaaaaaa-0000-0000-0000-000000000006
:END:

* DONE All three cookies
SCHEDULED: <2026-05-15 Fri> DEADLINE: <2026-06-01 Mon> CLOSED: [2026-05-30 Sat]
:PROPERTIES:
:ID: aaaaaaaa-0000-0000-0000-000000000007
:END:
";
    let (mut source, mut regenerated) =
        round_trip_through_db("cookies_all_combinations", fixture).await;
    normalise_pair_for_comparison(&mut source, &mut regenerated);
    assert_round_trip_eq(&source, &regenerated, "cookies_all_combinations");

    // Construct-level assertions: name → expected (sched, dead, closed-isSome).
    let by_title: HashMap<&str, &OrgTask> = regenerated
        .headlines
        .iter()
        .map(|t| (t.title.as_str(), t))
        .collect();
    use chrono::NaiveDate;
    let d = |y, m, day| NaiveDate::from_ymd_opt(y, m, day).unwrap();

    let no_cookies = by_title.get("No cookies").expect("no-cookies present");
    assert!(no_cookies.scheduled.is_none(), "no-cookies SCHEDULED leak");
    assert!(no_cookies.deadline.is_none(), "no-cookies DEADLINE leak");
    assert!(no_cookies.closed.is_none(), "no-cookies CLOSED leak");

    let sched_only = by_title.get("Only SCHEDULED").unwrap();
    assert_eq!(sched_only.scheduled, Some(d(2026, 5, 15)));
    assert!(sched_only.deadline.is_none());

    let dead_only = by_title.get("Only DEADLINE").unwrap();
    assert!(dead_only.scheduled.is_none());
    assert_eq!(dead_only.deadline, Some(d(2026, 6, 1)));

    let closed_only = by_title.get("Only CLOSED").unwrap();
    assert!(closed_only.closed.is_some(), "CLOSED stripped on solo");

    let sched_dead = by_title.get("SCHEDULED and DEADLINE").unwrap();
    assert_eq!(sched_dead.scheduled, Some(d(2026, 5, 15)));
    assert_eq!(sched_dead.deadline, Some(d(2026, 6, 1)));

    let all_three = by_title.get("All three cookies").unwrap();
    assert!(all_three.scheduled.is_some());
    assert!(all_three.deadline.is_some());
    assert!(all_three.closed.is_some());
}

#[tokio::test]
async fn comprehensive_repeater_modes() {
    // Spec §7.3.3 rule 3 — `:RRULE:` is canonical, the SCHEDULED
    // cookie is the best-fit projection. Three tasks here use
    // each of Org's three repeater prefixes; the round-trip
    // preserves both the cookie's prefix character (which maps
    // to RepeatMode in atrium-core::repeat) and the canonical
    // `:RRULE:` value.
    let fixture = "\
* TODO Basic repeater (`+1w`)
SCHEDULED: <2026-05-15 Fri +1w>
:PROPERTIES:
:ID: bbbbbbbb-0000-0000-0000-000000000001
:RRULE: FREQ=WEEKLY
:END:

* TODO Cumulative repeater (`++1w`, the default)
SCHEDULED: <2026-05-15 Fri ++1w>
:PROPERTIES:
:ID: bbbbbbbb-0000-0000-0000-000000000002
:RRULE: FREQ=WEEKLY
:END:

* TODO Next-from-completion repeater (`.+1w`)
SCHEDULED: <2026-05-15 Fri .+1w>
:PROPERTIES:
:ID: bbbbbbbb-0000-0000-0000-000000000003
:RRULE: FREQ=WEEKLY
:END:
";
    let (mut source, mut regenerated) = round_trip_through_db("repeater_modes", fixture).await;
    normalise_pair_for_comparison(&mut source, &mut regenerated);
    assert_round_trip_eq(&source, &regenerated, "repeater_modes");

    for headline in &regenerated.headlines {
        assert!(
            headline.scheduled_repeater.is_some(),
            "{}: repeater dropped on round-trip",
            headline.title,
        );
        assert_eq!(
            headline.properties.get("RRULE").map(String::as_str),
            Some("FREQ=WEEKLY"),
            "{}: RRULE diverged",
            headline.title,
        );
    }
}

#[tokio::test]
async fn comprehensive_closed_with_time_of_day() {
    // The CLOSED cookie's time-of-day is a real Org construct —
    // Emacs writes `[YYYY-MM-DD Day HH:MM]` when you mark a task
    // DONE inside a working session, and stripping the time
    // would lose information. Atrium emits the time when it
    // isn't the parser's noon-UTC default (the date-only
    // sentinel) so this fixture pins the time-bearing path.
    let fixture = "\
* DONE Completed mid-afternoon
CLOSED: [2026-05-08 Fri 14:22]
:PROPERTIES:
:ID: cccccccc-0000-0000-0000-000000000001
:END:
";
    let (mut source, mut regenerated) = round_trip_through_db("closed_with_time", fixture).await;

    // The harness's pair-normalise zeroes the time-of-day on
    // both sides because the v0.7.9 importer used to stamp
    // `now()` on completion. v0.7.17+ threads the source time
    // through; this test pins that the SOURCE side carried a
    // 14:22 time before the harness stripped it.
    let source_closed = source.headlines[0]
        .closed
        .expect("source should carry CLOSED");
    use chrono::Timelike;
    assert_eq!(source_closed.time().hour(), 14);
    assert_eq!(source_closed.time().minute(), 22);

    // After normalisation the two should match (both at midnight).
    normalise_pair_for_comparison(&mut source, &mut regenerated);
    assert_round_trip_eq(&source, &regenerated, "closed_with_time");
}

#[tokio::test]
async fn comprehensive_multi_tag_headline() {
    // Headline tags slot — `:tag1:tag2:tag3:`. Order isn't
    // semantic (the harness sorts before compare) but every
    // tag must survive.
    let fixture = "\
* TODO Multi-tag task :work:client:urgent:billable:Q3:
:PROPERTIES:
:ID: dddddddd-0000-0000-0000-000000000001
:END:
";
    let (mut source, mut regenerated) = round_trip_through_db("multi_tag", fixture).await;
    normalise_pair_for_comparison(&mut source, &mut regenerated);
    assert_round_trip_eq(&source, &regenerated, "multi_tag");

    let mut tags = regenerated.headlines[0].tags.clone();
    tags.sort();
    let mut expected = vec![
        "Q3".to_string(),
        "billable".to_string(),
        "client".to_string(),
        "urgent".to_string(),
        "work".to_string(),
    ];
    expected.sort();
    assert_eq!(tags, expected, "tag set diverged");
}

#[tokio::test]
async fn comprehensive_well_known_property_keys_survive() {
    // The `:PROPERTIES:` drawer carries Atrium's well-known keys
    // (ID, RRULE, EFFORT, DEFER_UNTIL). Each maps to a real
    // column in the task schema and round-trips losslessly. The
    // importer cherry-picks these keys and writes them through
    // typed fields; the writer re-emits them from those fields.
    // SCHEDULED comes BEFORE the properties drawer in canonical
    // Atrium-emit order; the writer's cookie-projection path
    // consults the SCHEDULED date to render the RRULE's best-fit
    // Org cookie on the planning line.
    let fixture = "\
* TODO Task with well-known property keys
SCHEDULED: <2026-05-15 Fri>
:PROPERTIES:
:ID: eeeeeeee-0000-0000-0000-000000000001
:EFFORT: 1:30
:DEFER_UNTIL: 2026-05-20
:RRULE: FREQ=WEEKLY
:END:
";
    let (mut source, mut regenerated) =
        round_trip_through_db("well_known_property_keys", fixture).await;
    normalise_pair_for_comparison(&mut source, &mut regenerated);
    assert_round_trip_eq(&source, &regenerated, "well_known_property_keys");

    let props = &regenerated.headlines[0].properties;
    assert_eq!(props.get("EFFORT").map(String::as_str), Some("1:30"));
    assert_eq!(
        props.get("DEFER_UNTIL").map(String::as_str),
        Some("2026-05-20"),
    );
    assert_eq!(props.get("RRULE").map(String::as_str), Some("FREQ=WEEKLY"),);
}

#[tokio::test]
async fn documented_limit_org_importer_drops_custom_property_keys() {
    // Documents the asymmetry: the writer emits whatever's in
    // `task.properties` HashMap, but the IMPORTER cherry-picks
    // only the four well-known keys (ID, EFFORT, DEFER_UNTIL,
    // RRULE) and writes them through typed columns. Custom keys
    // — `:CATEGORY:`, `:CLIENT:`, `:URL:`, anything else a user
    // might put in their drawer — get dropped because the schema
    // doesn't have a place for arbitrary key-value extras.
    //
    // Spec §7.3.3 rule 1 ("preserve unknown constructs
    // verbatim") is upheld for body content (Org tables / source
    // blocks / lists / links all survive in `task.note`) but
    // not for property-drawer keys outside the well-known set.
    // Closing this gap needs either a `task_property` table or
    // a JSON column on `task` — both schema-changing, both
    // out-of-scope for the current Org-emit-styling work.
    //
    // When that lands, this test fails: the regenerated drawer
    // will carry the custom keys and the round-trip becomes
    // lossless. Flip the assertion to expect the keys present.
    let fixture = "\
* TODO Task with mixed well-known + custom property keys
:PROPERTIES:
:ID: eeeeeeee-1111-1111-1111-111111111111
:EFFORT: 1:30
:CATEGORY: Q3-deliverables
:CLIENT: Acme Corp
:URL: https://example.com/ticket/42
:END:
";
    let (_source, regenerated) = round_trip_through_db("custom_property_keys_drop", fixture).await;
    let props = &regenerated.headlines[0].properties;

    // Well-known keys survive.
    assert_eq!(props.get("EFFORT").map(String::as_str), Some("1:30"));
    assert!(props.contains_key("ID"));

    // Custom keys are dropped on the way through the importer.
    assert!(
        !props.contains_key("CATEGORY"),
        "CATEGORY survived round-trip — the property-drawer gap from \
         spec §7.3.3 rule 1 may have been closed; flip this test \
         from documenting the limit to asserting preservation.",
    );
    assert!(
        !props.contains_key("CLIENT"),
        "CLIENT unexpectedly survived"
    );
    assert!(!props.contains_key("URL"), "URL unexpectedly survived");
}

#[tokio::test]
async fn comprehensive_body_content_preserves_org_constructs() {
    // The body — everything between the headline + cookies +
    // properties drawer and the next headline — captures
    // verbatim. That makes Atrium safe for vault-as-living-
    // document use: Org tables, source blocks, lists, internal
    // links, and external URL links survive even though Atrium
    // doesn't render them. Spec §7.3.3 rule 1: "preserve unknown
    // constructs verbatim".
    let fixture = "\
* TODO Document with rich body
:PROPERTIES:
:ID: ffffffff-0000-0000-0000-000000000001
:END:
A multi-paragraph note with several Org constructs the
parser should preserve verbatim:

#+BEGIN_SRC rust
fn hello() {
    println!(\"world\");
}
#+END_SRC

| Header A | Header B |
|----------+----------|
| cell 1   | cell 2   |
| cell 3   | cell 4   |

- bullet item one
- bullet item two
  - nested item
- bullet item three

[[https://example.com][an external link]]
[[file:./other.org::Heading][an internal link]]

End of body.
";
    let (mut source, mut regenerated) = round_trip_through_db("body_content", fixture).await;
    normalise_pair_for_comparison(&mut source, &mut regenerated);
    assert_round_trip_eq(&source, &regenerated, "body_content");

    let body = &regenerated.headlines[0].body;
    assert!(body.contains("#+BEGIN_SRC rust"), "source block dropped");
    assert!(body.contains("#+END_SRC"), "source block end dropped");
    assert!(body.contains("| Header A | Header B |"), "table dropped");
    assert!(body.contains("- bullet item one"), "bullet list dropped");
    assert!(
        body.contains("  - nested item"),
        "nested list indent dropped"
    );
    assert!(
        body.contains("[[https://example.com][an external link]]"),
        "external link dropped",
    );
    assert!(
        body.contains("[[file:./other.org::Heading][an internal link]]"),
        "internal link dropped",
    );
}

#[tokio::test]
async fn comprehensive_minimal_headline() {
    // The simplest possible task — a TODO with a title and
    // nothing else. The round-trip must NOT add a stray
    // properties drawer (it would by accident if the writer
    // ever started defaulting to one). The importer auto-
    // generates an `:ID:` per spec §7.3.3 rule 2 because
    // round-trip needs a stable anchor; the regenerated file
    // *will* have an `:ID:` even though the source didn't.
    // The harness's pair-normalise strips the regenerated ID
    // when the source had none, which is what makes the
    // assertion pass.
    let fixture = "* TODO Buy milk\n";
    let (mut source, mut regenerated) = round_trip_through_db("minimal_headline", fixture).await;
    normalise_pair_for_comparison(&mut source, &mut regenerated);
    assert_round_trip_eq(&source, &regenerated, "minimal_headline");

    assert_eq!(regenerated.headlines.len(), 1);
    assert_eq!(regenerated.headlines[0].title, "Buy milk");
    assert_eq!(
        keyword_str(&regenerated.headlines[0].keyword),
        "TODO",
        "keyword diverged on minimal task",
    );
    assert!(
        regenerated.headlines[0].scheduled.is_none(),
        "minimal task gained a SCHEDULED",
    );
    assert!(
        regenerated.headlines[0].body.is_empty(),
        "minimal task gained a body",
    );
}

#[tokio::test]
async fn comprehensive_keyword_variants() {
    // TODO / DONE / CANCELLED are canonical Atrium keywords
    // (round-tripped directly via `task.completed_at` and the
    // OrgKeyword enum). WAITING / IN-PROGRESS / BLOCKED are
    // non-canonical; Atrium stashes them in `task.orig_keyword`
    // (migration 0007) so the headline word survives without
    // teaching the domain about every TODO-cycle word a user
    // might invent.
    let fixture = "\
* TODO Canonical TODO
:PROPERTIES:
:ID: 11111111-0000-0000-0000-000000000001
:END:

* DONE Canonical DONE
CLOSED: [2026-05-08 Fri]
:PROPERTIES:
:ID: 11111111-0000-0000-0000-000000000002
:END:

* CANCELLED Canonical CANCELLED
CLOSED: [2026-05-08 Fri]
:PROPERTIES:
:ID: 11111111-0000-0000-0000-000000000003
:END:

* WAITING External signoff
:PROPERTIES:
:ID: 11111111-0000-0000-0000-000000000004
:END:

* IN-PROGRESS Migration in flight
:PROPERTIES:
:ID: 11111111-0000-0000-0000-000000000005
:END:

* BLOCKED On legal review
:PROPERTIES:
:ID: 11111111-0000-0000-0000-000000000006
:END:
";
    let (mut source, mut regenerated) = round_trip_through_db("keyword_variants", fixture).await;
    normalise_pair_for_comparison(&mut source, &mut regenerated);
    assert_round_trip_eq(&source, &regenerated, "keyword_variants");

    let keywords: Vec<String> = regenerated
        .headlines
        .iter()
        .map(|h| keyword_str(&h.keyword))
        .collect();
    assert_eq!(
        keywords,
        vec![
            "TODO",
            "DONE",
            "CANCELLED",
            "WAITING",
            "IN-PROGRESS",
            "BLOCKED",
        ],
        "keyword set diverged",
    );
}

#[tokio::test]
async fn documented_limit_org_importer_skips_sub_headings() {
    // Documents the asymmetry from v0.12.0: the writer learned
    // to emit project sub-headings as depth-1 keyword-less
    // headlines (driven by the Todoist mapper), but the Org
    // *importer* still skips them — they're counted in
    // `ImportSummary::headings_skipped` and do NOT land in the
    // `heading` table. Children of a sub-heading flow into the
    // project at top level "as if the sub-heading were
    // transparent" per the importer's existing comment.
    //
    // When a future patch closes the loop (Org → DB heading-
    // ingest), this test fails — at which point the assertions
    // flip from "headings_skipped == 2" to "two heading rows
    // in the DB" and the round-trip becomes lossless.
    use atrium_org::org::import_org_file;

    let fixture = "\
#+TITLE: Project with sub-headings
:PROPERTIES:
:ID: 99999999-0000-0000-0000-000000000000
:END:

* First section

** TODO Task under first section
:PROPERTIES:
:ID: 99999999-0000-0000-0000-000000000001
:END:

* Second section

** TODO Task under second section
:PROPERTIES:
:ID: 99999999-0000-0000-0000-000000000002
:END:
";
    let scratch =
        std::env::temp_dir().join(format!("atrium-rt-importer-limit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).unwrap();
    let src_path = scratch.join("source.org");
    std::fs::write(&src_path, fixture).unwrap();

    let db_path = scratch.join("atrium.db");
    let writer_conn = atrium_core::db::open(&db_path).unwrap();
    let read_conn = atrium_core::db::open(&db_path).unwrap();
    let (handle, _changes_rx, _library_rx) = atrium_core::spawn_worker(writer_conn);

    let summary = import_org_file(&handle, &src_path, false).await.unwrap();
    assert_eq!(
        summary.headings_skipped, 2,
        "expected 2 sub-headings to be counted as skipped",
    );
    assert_eq!(
        summary.tasks_created, 2,
        "tasks under sub-headings should still flow into the project",
    );

    // The `heading` table should be empty — no heading rows
    // were created from the source's `* First section` /
    // `* Second section` lines.
    let heading_count: i64 = read_conn
        .query_row("SELECT COUNT(*) FROM heading", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        heading_count, 0,
        "Org importer is expected to skip sub-headings; got {heading_count} rows. \
         If this assertion fails, the importer-side gap from v0.12.0 is closed — \
         flip this test from documenting the limit to asserting heading round-trip.",
    );

    let _ = std::fs::remove_dir_all(&scratch);
}

// Suppress unused-import warning when the test module isn't
// compiled (e.g., release builds skip integration tests). PathBuf
// is used inside the harness above for ad-hoc fixture writes.
#[allow(dead_code)]
fn _silence_unused_path() -> PathBuf {
    PathBuf::new()
}
