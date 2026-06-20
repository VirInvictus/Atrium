// SPDX-License-Identifier: MIT
//! Tests for atrium/src/ui/window.rs.
//!
//! Loaded as the window module's tests submodule via
//! `#[cfg(test)] #[path = "window_tests.rs"] mod tests;`.
//! Extracted from window.rs in v0.22.0's structural split (Pass 1)
//! to keep the production code path focused for review.

use super::*;

#[test]
fn primary_menu_has_four_sections_no_debug() {
    let menu = build_primary_menu(false);
    // New + Library + Mode + Import (v0.34.0) + About sections.
    assert_eq!(menu.n_items(), 5);
}

// ── v0.4.1 search-history helpers ──────────────────────────────

#[test]
fn push_history_entry_appends_normal_case() {
    let mut h = vec!["a".to_string()];
    push_history_entry(&mut h, "b".into(), 5);
    assert_eq!(h, vec!["a", "b"]);
}

#[test]
fn push_history_entry_dedupes_against_last() {
    let mut h = vec!["a".to_string(), "b".into()];
    push_history_entry(&mut h, "b".into(), 5);
    assert_eq!(h, vec!["a", "b"]);
}

#[test]
fn push_history_entry_does_not_dedupe_non_consecutive() {
    // "a" appears then "b" then "a" again — both "a" entries
    // are kept because they're not adjacent.
    let mut h = vec!["a".to_string(), "b".into()];
    push_history_entry(&mut h, "a".into(), 5);
    assert_eq!(h, vec!["a", "b", "a"]);
}

#[test]
fn push_history_entry_caps_at_max() {
    let mut h: Vec<String> = (0..5).map(|i| format!("q{i}")).collect();
    push_history_entry(&mut h, "q5".into(), 5);
    // Oldest dropped from the front; newest at the end.
    assert_eq!(h, vec!["q1", "q2", "q3", "q4", "q5"]);
}

#[test]
fn push_history_entry_ignores_empty_input() {
    let mut h = vec!["a".to_string()];
    push_history_entry(&mut h, "".into(), 5);
    push_history_entry(&mut h, "   ".into(), 5);
    assert_eq!(h, vec!["a"]);
}

#[test]
fn cycle_history_cursor_empty_history_stays_none() {
    assert_eq!(cycle_history_cursor(None, 0, HistoryDirection::Older), None);
    assert_eq!(cycle_history_cursor(None, 0, HistoryDirection::Newer), None);
}

#[test]
fn cycle_history_cursor_older_from_live_lands_on_most_recent() {
    // history len 3 → most recent index is 2
    assert_eq!(
        cycle_history_cursor(None, 3, HistoryDirection::Older),
        Some(2)
    );
}

#[test]
fn cycle_history_cursor_older_walks_back() {
    assert_eq!(
        cycle_history_cursor(Some(2), 3, HistoryDirection::Older),
        Some(1)
    );
    assert_eq!(
        cycle_history_cursor(Some(1), 3, HistoryDirection::Older),
        Some(0)
    );
}

#[test]
fn cycle_history_cursor_older_clamps_at_oldest() {
    // Already at the oldest entry; ↑ shouldn't underflow.
    assert_eq!(
        cycle_history_cursor(Some(0), 3, HistoryDirection::Older),
        Some(0)
    );
}

#[test]
fn cycle_history_cursor_newer_returns_to_live_past_most_recent() {
    // Walking forward off the end of history → live entry (None).
    assert_eq!(
        cycle_history_cursor(Some(2), 3, HistoryDirection::Newer),
        None
    );
}

#[test]
fn cycle_history_cursor_newer_walks_forward() {
    assert_eq!(
        cycle_history_cursor(Some(0), 3, HistoryDirection::Newer),
        Some(1)
    );
    assert_eq!(
        cycle_history_cursor(Some(1), 3, HistoryDirection::Newer),
        Some(2)
    );
}

#[test]
fn cycle_history_cursor_newer_from_live_stays_live() {
    assert_eq!(cycle_history_cursor(None, 3, HistoryDirection::Newer), None);
}

#[test]
fn primary_menu_includes_debug_section_when_enabled() {
    let menu = build_primary_menu(true);
    // New + Library + Mode + Import (v0.34.0) + Debug + About sections.
    assert_eq!(menu.n_items(), 6);
}

#[test]
fn sidebar_lists_cover_simple_mode() {
    // v0.6.16 — Logbook moved to the trailing slot of
    // top_tier_extras; CANONICAL_LISTS holds five rows now.
    assert_eq!(CANONICAL_LISTS.len(), 5);
    assert!(CANONICAL_LISTS.contains(&ActiveList::Inbox));
    assert!(CANONICAL_LISTS.contains(&ActiveList::Today));
    assert!(!CANONICAL_LISTS.contains(&ActiveList::Logbook));
}

#[test]
fn top_tier_extras_simple_mode_has_agenda_and_logbook() {
    let extras = top_tier_extras(false);
    // v0.6.16 — Agenda + Logbook trail the canonical set in
    // both modes; Forecast and Review only appear in Builder.
    assert_eq!(extras.len(), 2);
    assert_eq!(extras[0].0, ActiveList::Agenda);
    assert_eq!(extras[1].0, ActiveList::Logbook);
}

#[test]
fn top_tier_extras_builder_mode_inserts_calendar_and_review() {
    let extras = top_tier_extras(true);
    // v0.39.0 — Forecast merged into Agenda as a Builder-only Strip
    // layout (no own row). Builder top tier is now Agenda / Calendar /
    // Review between the bookends, with Logbook trailing.
    assert_eq!(extras.len(), 4);
    assert_eq!(extras[0].0, ActiveList::Agenda);
    assert_eq!(extras[1].0, ActiveList::Calendar);
    assert_eq!(extras[2].0, ActiveList::Review);
    assert_eq!(extras[3].0, ActiveList::Logbook);
}

#[test]
fn agenda_forecast_review_have_accent_classes() {
    // v0.6.7 — top-tier extras tint their icons just like the
    // canonical rows. Pinning the class names so a future tweak
    // doesn't quietly drop the accent and turn the icons grey.
    assert_eq!(
        canonical_accent_class(&ActiveList::Agenda),
        Some("atrium-canonical-agenda")
    );
    assert_eq!(
        canonical_accent_class(&ActiveList::Forecast),
        Some("atrium-canonical-forecast")
    );
    assert_eq!(
        canonical_accent_class(&ActiveList::Review),
        Some("atrium-canonical-review")
    );
}

// Build a fake sidebar layout: 2 canonical, then "Areas" header
// + 2 areas, then "Tags" header + 2 tags. (We use 2 canonical
// instead of 6 to keep the fixtures small; the helper takes the
// canonical count as a parameter.)
fn fake_sidebar() -> (Vec<Option<ActiveList>>, Vec<Option<String>>) {
    let targets = vec![
        Some(ActiveList::Inbox),    // 0
        Some(ActiveList::Today),    // 1
        None,                       // 2 — Areas header
        Some(ActiveList::Area(10)), // 3 — "Work"
        Some(ActiveList::Area(11)), // 4 — "Home"
        None,                       // 5 — Tags header
        Some(ActiveList::Tag(20)),  // 6 — "errand"
        Some(ActiveList::Tag(21)),  // 7 — "work-focus"
    ];
    let titles = vec![
        None,
        None,
        None,
        Some("Work".into()),
        Some("Home".into()),
        None,
        Some("errand".into()),
        Some("work-focus".into()),
    ];
    (targets, titles)
}

#[test]
fn empty_query_shows_everything() {
    let (t, n) = fake_sidebar();
    let v = compute_sidebar_visibility("", 2, &t, &n);
    assert_eq!(v, vec![true; 8]);
}

#[test]
fn filter_matches_one_section_hides_other_header() {
    let (t, n) = fake_sidebar();
    let v = compute_sidebar_visibility("err", 2, &t, &n);
    // canonical kept; Areas hidden (no match); errand visible,
    // work-focus hidden; Tags header lifted.
    assert_eq!(v[0..2], [true, true]);
    assert!(!v[2]); // Areas header
    assert!(!v[3] && !v[4]); // areas
    assert!(v[5]); // Tags header
    assert!(v[6] && !v[7]);
}

#[test]
fn filter_promotes_header_when_any_child_matches() {
    let (t, n) = fake_sidebar();
    let v = compute_sidebar_visibility("work", 2, &t, &n);
    // "Work" area matches → Areas header lifts.
    // "work-focus" tag matches → Tags header lifts.
    assert!(v[2]); // Areas header
    assert!(v[3]); // Work
    assert!(!v[4]); // Home
    assert!(v[5]); // Tags header
    assert!(!v[6]); // errand
    assert!(v[7]); // work-focus
}

#[test]
fn filter_is_case_insensitive() {
    let (t, n) = fake_sidebar();
    let lower = compute_sidebar_visibility("home", 2, &t, &n);
    let upper = compute_sidebar_visibility("HOME", 2, &t, &n);
    let mixed = compute_sidebar_visibility("HoMe", 2, &t, &n);
    assert_eq!(lower, upper);
    assert_eq!(lower, mixed);
    assert!(lower[4]); // "Home"
}

#[test]
fn no_match_leaves_only_canonical_visible() {
    let (t, n) = fake_sidebar();
    let v = compute_sidebar_visibility("zzzzz", 2, &t, &n);
    assert_eq!(v[0..2], [true, true]);
    assert!(v[2..].iter().all(|b| !b));
}

#[test]
fn whitespace_query_treated_as_empty() {
    let (t, n) = fake_sidebar();
    let v = compute_sidebar_visibility("   ", 2, &t, &n);
    assert_eq!(v, vec![true; 8]);
}

// Phase 11 — available-task badge math.

#[test]
fn available_parallel_project_shows_open_count() {
    // Parallel project: every open task is available.
    assert_eq!(available_count(0, false), 0);
    assert_eq!(available_count(1, false), 1);
    assert_eq!(available_count(7, false), 7);
}

#[test]
fn available_sequential_project_caps_at_one() {
    // Sequential project: only the head row is available.
    assert_eq!(available_count(0, true), 0);
    assert_eq!(available_count(1, true), 1);
    assert_eq!(available_count(7, true), 1);
}
