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

// Suppress unused-import warning when the test module isn't
// compiled (e.g., release builds skip integration tests). PathBuf
// is used inside the harness above for ad-hoc fixture writes.
#[allow(dead_code)]
fn _silence_unused_path() -> PathBuf {
    PathBuf::new()
}
