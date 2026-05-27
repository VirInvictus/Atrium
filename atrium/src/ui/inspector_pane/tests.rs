// SPDX-License-Identifier: MIT
//! Tests for the Builder Mode Inspector pane. Extracted from
//! inspector_pane/mod.rs in v0.22.0 split.

use super::*;

#[test]
fn tag_count_formatter() {
    assert_eq!(format_tag_count(0), "No tags");
    assert_eq!(format_tag_count(1), "1 tag");
    assert_eq!(format_tag_count(5), "5 tags");
}

// Phase 15 — preset / interval round-trip helpers.

#[test]
fn preset_recognition() {
    assert_eq!(preset_from_rule(None), RepeatPreset::None);
    assert_eq!(preset_from_rule(Some("FREQ=DAILY")), RepeatPreset::Daily);
    assert_eq!(preset_from_rule(Some("FREQ=WEEKLY")), RepeatPreset::Weekly);
    assert_eq!(
        preset_from_rule(Some("FREQ=MONTHLY")),
        RepeatPreset::Monthly
    );
    assert_eq!(preset_from_rule(Some("FREQ=YEARLY")), RepeatPreset::Yearly);
    // INTERVAL keeps the preset simple.
    assert_eq!(
        preset_from_rule(Some("FREQ=WEEKLY;INTERVAL=2")),
        RepeatPreset::Weekly
    );
    // BYDAY / COUNT / UNTIL fall through to Custom.
    assert_eq!(
        preset_from_rule(Some("FREQ=WEEKLY;BYDAY=MO,WE")),
        RepeatPreset::Custom
    );
    assert_eq!(
        preset_from_rule(Some("FREQ=DAILY;COUNT=5")),
        RepeatPreset::Custom
    );
}

#[test]
fn interval_round_trip() {
    assert_eq!(interval_from_rule(Some("FREQ=DAILY")), Some(1));
    assert_eq!(interval_from_rule(Some("FREQ=WEEKLY;INTERVAL=3")), Some(3));
    assert_eq!(interval_from_rule(None), None);
}

#[test]
fn rule_emit() {
    assert_eq!(rule_from_freq("DAILY", 1), "FREQ=DAILY");
    assert_eq!(rule_from_freq("WEEKLY", 2), "FREQ=WEEKLY;INTERVAL=2");
}

#[test]
fn mode_index_round_trip() {
    for m in [RepeatMode::Cumulative, RepeatMode::Next, RepeatMode::Basic] {
        assert_eq!(mode_from_index(mode_index(m)), m);
    }
}
