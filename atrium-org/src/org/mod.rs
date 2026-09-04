// SPDX-License-Identifier: MIT
//! Hand-rolled Org-mode parser + emitter for the Atrium vault
//! projection (Phase 16, v0.7.7).
//!
//! `atrium-core::sync::org` exposes a focused, passthrough parser
//! for the Org subset spec §7.3 maps to: headlines, TODO/DONE/
//! CANCELLED keywords, SCHEDULED/DEADLINE/CLOSED cookies, headline
//! tags, `:PROPERTIES:` drawers, and body text. Anything
//! Atrium doesn't model (custom TODO keywords, source blocks,
//! tables, latex, links, drawers other than :PROPERTIES:) is
//! captured into the task's `unknown_lines` field and re-emitted
//! verbatim on write — satisfying spec §7.3.3 rule 1 ("Never
//! destroy data").
//!
//! No third-party crates; the parser fits the CalibreQuarry
//! stdlib-only ethos. See CLAUDE.md's dependency-discipline
//! section for the full reasoning behind choosing this over
//! `orgize` / `starsector`.

mod emit;
mod import;
mod parse;
mod write;

pub use emit::{emit_org_file, emit_org_file_with_meta, emit_org_text, emit_org_text_with_meta};
pub use import::{
    ImportError, ImportSummary, import_org_directory, import_org_file, import_org_file_with_area,
};
pub use parse::{
    OrgClockEntry, OrgFile, OrgKeyword, OrgRepeater, OrgTask, parse_org_file,
    parse_org_file_with_meta, parse_org_text, parse_org_text_with_meta,
};
pub use write::{
    WriteError, WriteSummary, project_vault_path, render_project_to_string,
    write_all_projects_to_vault, write_project_to_vault,
};

use std::collections::{BTreeMap, HashMap};

use chrono::{NaiveDate, NaiveTime};

/// v0.24.0 — Property-drawer keys the schema models through typed
/// columns. The Org importer + vault watcher consume these into
/// `NewTask` fields directly; everything else stashes into
/// `task.extra_properties` via [`extras_from_properties`] for
/// verbatim round-trip per spec §7.3.3 rule 1.
///
/// `CREATED` / `MODIFIED` aren't currently read by the importer
/// (Atrium's `created_at` / `modified_at` triggers stamp them at
/// write time), but they're listed here defensively — a manual
/// user-set `:CREATED:` value would otherwise round-trip-conflict
/// with the schema-managed timestamp on a re-emit.
///
/// `ORIG_KEYWORD` isn't emitted as a property today (the keyword
/// itself sits on the headline), but listing it future-proofs
/// against a writer-side change.
pub const MODELED_PROPERTY_KEYS: &[&str] = &[
    "ID",
    "CREATED",
    "MODIFIED",
    "DEFER_UNTIL",
    "EFFORT",
    "RRULE",
    "ORIG_KEYWORD",
];

/// Partition a parsed `:PROPERTIES:` drawer into the unmodeled-key
/// extras Atrium stashes on `task.extra_properties`. The
/// modeled-key set ([`MODELED_PROPERTY_KEYS`]) is filtered out;
/// everything else lands in the returned [`BTreeMap`]. Case-
/// sensitive uppercase match — the parser uppercases keys on
/// capture, so the same casing applies on both ends.
pub fn extras_from_properties(properties: &HashMap<String, String>) -> BTreeMap<String, String> {
    properties
        .iter()
        .filter(|(k, _)| !MODELED_PROPERTY_KEYS.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Task-level drawer/cookie fields both capture paths derive from a
/// parsed headline: the one-shot importer (`org::import`) and the
/// vault watcher's create/update paths (`vault_watcher`). One
/// derivation for both, so create-time and update-time property
/// coverage can't drift apart — the Phase 23 sweep found exactly
/// that drift: the watcher's create path dropped `:EFFORT:` /
/// `:DEFER_UNTIL:` and its update path ignored the note body too.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PropertyFields {
    /// `:EFFORT:` → `task.estimated_minutes` (`H:MM`, `30m`, `1h`,
    /// `1h30m`).
    pub estimated_minutes: Option<i64>,
    /// `:DEFER_UNTIL:` → `task.defer_until` (`YYYY-MM-DD`).
    pub defer_until: Option<NaiveDate>,
    /// `:RRULE:` → `task.repeat_rule` (canonical per spec §7.3.3
    /// rule 3).
    pub repeat_rule: Option<String>,
    /// DEADLINE cookie warning suffix (`-Nd`) →
    /// `task.deadline_warn_days`.
    pub deadline_warn_days: Option<i64>,
    /// SCHEDULED cookie time-of-day → `task.scheduled_time`.
    pub scheduled_time: Option<NaiveTime>,
    /// Unmodeled drawer keys → `task.extra_properties`.
    pub extra_properties: BTreeMap<String, String>,
    /// `:EFFORT:` was present but unparseable. The importer
    /// surfaces a lossy note; the watcher degrades silently.
    pub effort_lossy: bool,
    /// `:DEFER_UNTIL:` was present but not a `YYYY-MM-DD` date.
    pub defer_lossy: bool,
}

impl PropertyFields {
    pub fn from_org(org: &OrgTask) -> Self {
        let effort_raw = org.properties.get("EFFORT");
        let estimated_minutes = effort_raw.and_then(|v| parse_effort(v));
        let effort_lossy = effort_raw.is_some() && estimated_minutes.is_none();
        let defer_raw = org.properties.get("DEFER_UNTIL");
        let defer_until = defer_raw.and_then(|v| parse_defer_until(v));
        let defer_lossy = defer_raw.is_some() && defer_until.is_none();
        Self {
            estimated_minutes,
            defer_until,
            repeat_rule: org.properties.get("RRULE").cloned(),
            deadline_warn_days: org.deadline_warning.map(i64::from),
            scheduled_time: org.scheduled_time,
            extra_properties: extras_from_properties(&org.properties),
            effort_lossy,
            defer_lossy,
        }
    }
}

/// Parse Org's `:EFFORT:` value into integer minutes. Supports
/// `H:MM` (`"1:30"` → 90) and the abbreviated forms `"30m"`,
/// `"1h"`, `"1h30m"`. Returns `None` for unparseable input.
pub fn parse_effort(value: &str) -> Option<i64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    // H:MM form.
    if let Some((h, m)) = trimmed.split_once(':')
        && let (Ok(hours), Ok(minutes)) = (h.parse::<i64>(), m.parse::<i64>())
        && hours >= 0
        && (0..60).contains(&minutes)
    {
        return Some(hours * 60 + minutes);
    }

    // Hh / Mm / HhMm form.
    let mut total_minutes: i64 = 0;
    let mut buf = String::new();
    let mut consumed_any = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_digit() {
            buf.push(ch);
        } else if ch == 'h' || ch == 'H' {
            let n: i64 = buf.parse().ok()?;
            total_minutes += n * 60;
            buf.clear();
            consumed_any = true;
        } else if ch == 'm' || ch == 'M' {
            let n: i64 = buf.parse().ok()?;
            total_minutes += n;
            buf.clear();
            consumed_any = true;
        } else {
            return None;
        }
    }
    if !consumed_any || !buf.is_empty() {
        return None;
    }
    Some(total_minutes)
}

/// Parse `:DEFER_UNTIL:` (`YYYY-MM-DD`) into a date.
pub fn parse_defer_until(value: &str) -> Option<NaiveDate> {
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_effort_hour_minute_form() {
        assert_eq!(parse_effort("1:30"), Some(90));
        assert_eq!(parse_effort("0:30"), Some(30));
        assert_eq!(parse_effort("2:00"), Some(120));
    }

    #[test]
    fn parses_effort_hm_form() {
        assert_eq!(parse_effort("30m"), Some(30));
        assert_eq!(parse_effort("1h"), Some(60));
        assert_eq!(parse_effort("1h30m"), Some(90));
        assert_eq!(parse_effort("2h"), Some(120));
    }

    #[test]
    fn parses_effort_rejects_invalid() {
        assert_eq!(parse_effort(""), None);
        assert_eq!(parse_effort("abc"), None);
        assert_eq!(parse_effort("1:75"), None);
        assert_eq!(parse_effort("1x"), None);
    }

    #[test]
    fn property_fields_extracts_modeled_set() {
        let mut org = OrgTask::default_test_node(1);
        org.keyword = Some(OrgKeyword::Todo);
        org.deadline_warning = Some(3);
        org.properties = HashMap::from([
            ("EFFORT".to_string(), "1:30".to_string()),
            ("DEFER_UNTIL".to_string(), "2026-09-01".to_string()),
            ("RRULE".to_string(), "FREQ=DAILY".to_string()),
            ("CLIENT".to_string(), "acme".to_string()),
        ]);
        let f = PropertyFields::from_org(&org);
        assert_eq!(f.estimated_minutes, Some(90));
        assert_eq!(
            f.defer_until,
            Some(NaiveDate::from_ymd_opt(2026, 9, 1).unwrap())
        );
        assert_eq!(f.repeat_rule.as_deref(), Some("FREQ=DAILY"));
        assert_eq!(f.deadline_warn_days, Some(3));
        assert_eq!(
            f.extra_properties.get("CLIENT").map(String::as_str),
            Some("acme")
        );
        assert!(!f.effort_lossy);
        assert!(!f.defer_lossy);
    }

    #[test]
    fn property_fields_flags_unparseable_values() {
        let mut org = OrgTask::default_test_node(1);
        org.keyword = Some(OrgKeyword::Todo);
        org.properties = HashMap::from([
            ("EFFORT".to_string(), "soon".to_string()),
            ("DEFER_UNTIL".to_string(), "next tuesday".to_string()),
        ]);
        let f = PropertyFields::from_org(&org);
        assert_eq!(f.estimated_minutes, None);
        assert_eq!(f.defer_until, None);
        assert!(f.effort_lossy);
        assert!(f.defer_lossy);
    }
}
