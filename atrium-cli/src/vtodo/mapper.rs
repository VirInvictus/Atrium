// SPDX-License-Identifier: MIT
//! VTODO ↔ Atrium DB mapper.
//!
//! Bridges parsed [`crate::vtodo::parser::VtodoComponent`]s to
//! worker calls on import, and shapes [`atrium_core::Task`] rows
//! into [`crate::vtodo::emit::VtodoOutput`]s on export.
//!
//! # Field mapping (spec §7.5)
//!
//! | VTODO | Atrium |
//! |---|---|
//! | `SUMMARY` | `task.title` |
//! | `DESCRIPTION` | `task.note` |
//! | `DUE` | `task.deadline` (date portion) |
//! | `DTSTART` | `task.scheduled_for` + `task.scheduled_time` |
//! | `COMPLETED` | `task.completed_at` |
//! | `STATUS:COMPLETED` | sets `completed_at = now()` if no COMPLETED |
//! | `STATUS:IN-PROCESS` / `CANCELLED` | `task.orig_keyword` |
//! | `PRIORITY` 1–4 | `priority-N` tag |
//! | `CATEGORIES` | `task.tag` rows via `ensure_tag` |
//! | `RRULE` | `task.repeat_rule` (verbatim) |
//! | `UID` | `task.uuid` (UUID-shaped) or v5 derive + `extra_properties["VTODO_UID"]` |
//! | `LOCATION` | `task.extra_properties["VTODO_LOCATION"]` |
//! | `X-*` | `task.extra_properties[X-*]` |
//! | other | lossy report |
//!
//! # UID round-trip
//!
//! A VTODO UID is free-form text (`task@nextcloud.example.com`,
//! `12345`, anything). Atrium's `task.uuid` is UUID-v4 by
//! contract. The mapper handles both:
//!
//! - When the UID parses as a UUID, use it directly — round-trip
//!   is identity.
//! - When the UID doesn't, derive a v5 UUID from
//!   [`VTODO_NAMESPACE`] + the original UID and stash the
//!   original in `task.extra_properties["VTODO_UID"]`. The
//!   exporter prefers the stashed value on emit, so external
//!   apps see their UID unchanged.

use std::collections::BTreeMap;

use atrium_core::error::DbError;
use atrium_core::{NewProject, NewTask, ScheduledFor, Task, WorkerHandle};
use chrono::NaiveDate;
use rusqlite::Connection;
use uuid::Uuid;

use super::emit::{DateOrDateTime as EmitDate, VtodoOutput};
use super::parser::VtodoComponent;

/// Stable namespace UUID for v5 derivations from non-UUID
/// VTODO UIDs. Distinct byte pattern from `TODOIST_NAMESPACE`
/// so cross-source collisions are impossible. Frozen once;
/// changes break re-import stability.
pub const VTODO_NAMESPACE: Uuid = Uuid::from_bytes([
    0x84, 0xb1, 0xc2, 0x7e, 0x3f, 0x09, 0x4d, 0x52, 0x9a, 0x6e, 0x1b, 0x84, 0xfa, 0x55, 0xd1, 0x07,
]);

/// Key the importer uses to stash the original VTODO UID when
/// it's not UUID-shaped. The exporter reads this same key.
pub const VTODO_UID_KEY: &str = "VTODO_UID";

/// Key the importer uses for LOCATION (no typed column in
/// Atrium; round-trips through `extra_properties`).
pub const VTODO_LOCATION_KEY: &str = "VTODO_LOCATION";

#[derive(Debug, Clone, Default)]
pub struct ImportSummary {
    /// `None` on dry-run.
    pub project_id: Option<i64>,
    pub project_title: String,
    pub tasks_created: usize,
    /// Distinct tag names ensured during the run (CATEGORIES
    /// plus priority-N). The worker dedupes; this count is the
    /// label set seen in the source.
    pub tags_created: usize,
    /// Names of top-level non-VTODO components encountered
    /// (VEVENT, VJOURNAL, etc.). Surfaced verbatim for the
    /// user's awareness.
    pub unsupported_top_level: Vec<String>,
    /// Per-row notes about fields Atrium dropped on import.
    pub lossy: Vec<LossyEntry>,
}

#[derive(Debug, Clone, Default)]
pub struct ExportSummary {
    /// Path the exporter wrote to.
    pub path: String,
    pub tasks_exported: usize,
    /// Bytes written. Useful for the human/JSON summary
    /// without re-reading the file.
    pub bytes_written: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LossyEntry {
    pub kind: LossyKind,
    pub task_title: Option<String>,
    pub raw: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LossyKind {
    /// A top-level non-VTODO component (VEVENT, VJOURNAL,
    /// VFREEBUSY, VTIMEZONE). Atrium drops the whole block.
    UnsupportedComponent,
    /// One or more VALARM blocks inside a VTODO. Atrium's
    /// reminders are separate (`task.reminder_at`); cross-
    /// mapping VALARM ↔ reminder is deferred.
    DroppedAlarm,
    /// ATTENDEE / ORGANIZER properties. Atrium is single-user;
    /// these have no schema home.
    DroppedAttendee,
    /// GEO property. No schema home.
    DroppedGeo,
    /// PERCENT-COMPLETE. Atrium tracks completion as a
    /// boolean; partial progress drops on the way in.
    DroppedPercentComplete,
    /// DURATION without a paired DUE/DTSTART. We don't
    /// compute the implied end date.
    DroppedDuration,
    /// At least one timestamp carried a TZID parameter.
    /// Atrium normalises everything to UTC (or date-only);
    /// the timezone label is lost.
    DroppedTimezone,
    /// A property whose name we don't model (and which isn't
    /// an X-* extension). Stashed name only, value dropped.
    UnknownProperty,
}

#[derive(Debug, thiserror::Error)]
pub enum MapError {
    #[error("worker error: {0}")]
    Worker(#[from] DbError),
}

/// Apply a parsed VTODO stream to the worker. Creates a fresh
/// project named `project_name`, walks the components in
/// source order, and writes one Atrium task per VTODO.
///
/// On dry-run, no DB writes happen — the summary still counts
/// what *would* land, and the lossy report still surfaces.
pub async fn import_vtodo(
    handle: &WorkerHandle,
    parsed: &crate::vtodo::parser::ParsedIcs,
    project_name: &str,
    dry_run: bool,
) -> Result<ImportSummary, MapError> {
    let mut summary = ImportSummary {
        project_title: project_name.to_string(),
        unsupported_top_level: parsed.unsupported_top_level.clone(),
        ..Default::default()
    };

    for name in &parsed.unsupported_top_level {
        summary.lossy.push(LossyEntry {
            kind: LossyKind::UnsupportedComponent,
            task_title: None,
            raw: name.clone(),
        });
    }

    if dry_run {
        for vtodo in &parsed.vtodos {
            summary.tasks_created += 1;
            record_vtodo_lossy(&mut summary, vtodo);
            count_tags_for_dry_run(&mut summary, vtodo);
        }
        return Ok(summary);
    }

    let project = handle
        .create_project(NewProject {
            title: project_name.to_string(),
            ..Default::default()
        })
        .await?;
    summary.project_id = Some(project.id);

    let mut ensured_tag_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    for vtodo in &parsed.vtodos {
        record_vtodo_lossy(&mut summary, vtodo);

        let title = vtodo
            .summary
            .clone()
            .unwrap_or_else(|| "(untitled VTODO)".to_string());

        let (uuid, vtodo_uid_stash) = resolve_uid(vtodo.uid.as_deref());

        let scheduled = vtodo.dtstart.map(|d| ScheduledFor::Date(d.date()));
        let scheduled_time = vtodo.dtstart.and_then(|d| d.time());
        let deadline = vtodo.due.map(|d| d.date());

        // Completion handling: COMPLETED property wins;
        // STATUS:COMPLETED without a paired property still
        // marks the task done at chrono::Utc::now().
        let status_upper = vtodo.status.as_deref().unwrap_or("");
        let status_done = matches!(status_upper, "COMPLETED" | "CANCELLED");
        let completed_at = vtodo
            .completed
            .or_else(|| status_done.then(chrono::Utc::now));

        // Atrium's domain has TODO/DONE only; IN-PROCESS /
        // CANCELLED / NEEDS-ACTION stash to orig_keyword so
        // the surface label survives a round-trip.
        let orig_keyword = match status_upper {
            "" | "NEEDS-ACTION" | "COMPLETED" => None,
            other => Some(other.to_string()),
        };

        let mut extras: BTreeMap<String, String> = BTreeMap::new();
        if let Some(uid) = vtodo_uid_stash {
            extras.insert(VTODO_UID_KEY.to_string(), uid);
        }
        if let Some(location) = &vtodo.location {
            extras.insert(VTODO_LOCATION_KEY.to_string(), location.clone());
        }
        for (key, value) in &vtodo.x_properties {
            extras.insert(key.clone(), value.clone());
        }

        let new = NewTask {
            title: title.clone(),
            note: vtodo.description.clone().unwrap_or_default(),
            project_id: Some(project.id),
            scheduled_for: scheduled,
            scheduled_time,
            deadline,
            completed_at,
            repeat_rule: vtodo.rrule.clone(),
            uuid: Some(uuid),
            orig_keyword,
            extra_properties: extras,
            ..Default::default()
        };
        let created = handle.create_task(new).await?;
        summary.tasks_created += 1;

        // Tags: CATEGORIES (raw labels) + a priority-N tag
        // for priorities 1–4 (1=highest in RFC 5545 too;
        // 5–9 are "normal-ish" and produce no tag).
        let mut tag_ids: Vec<i64> = Vec::new();
        for name in &vtodo.categories {
            let tag = handle.ensure_tag(name.clone()).await?;
            if ensured_tag_names.insert(name.clone()) {
                summary.tags_created += 1;
            }
            tag_ids.push(tag.id);
        }
        if let Some(p) = priority_tag_name(vtodo.priority) {
            let tag = handle.ensure_tag(p.clone()).await?;
            if ensured_tag_names.insert(p) {
                summary.tags_created += 1;
            }
            tag_ids.push(tag.id);
        }
        if !tag_ids.is_empty() {
            handle.set_task_tags(created.id, tag_ids).await?;
        }
    }

    Ok(summary)
}

/// Read every task in the DB and shape them into [`VtodoOutput`]s
/// the emitter renders. Tags map back to CATEGORIES; the
/// `priority-N` tag (if present) maps to PRIORITY N. Original
/// UIDs preserved via [`VTODO_UID_KEY`] in `extra_properties`
/// take precedence on emit so external CalDAV apps see their
/// UID unchanged.
pub fn export_vtodo(conn: &Connection) -> Result<Vec<VtodoOutput>, DbError> {
    let tasks = atrium_core::db::read::list_all_tasks(conn)?;
    let tags = atrium_core::db::read::list_tags(conn)?;
    let task_tags = atrium_core::db::read::list_task_tags(conn)?;

    let tag_by_id: std::collections::HashMap<i64, String> =
        tags.into_iter().map(|t| (t.id, t.name)).collect();
    let mut tags_by_task: std::collections::HashMap<i64, Vec<String>> =
        std::collections::HashMap::new();
    for (task_id, tag_id) in task_tags {
        if let Some(name) = tag_by_id.get(&tag_id) {
            tags_by_task.entry(task_id).or_default().push(name.clone());
        }
    }

    let mut out: Vec<VtodoOutput> = Vec::with_capacity(tasks.len());
    for task in tasks {
        out.push(task_to_vtodo(&task, tags_by_task.get(&task.id)));
    }
    Ok(out)
}

fn task_to_vtodo(task: &Task, tags: Option<&Vec<String>>) -> VtodoOutput {
    // UID preference: stashed VTODO_UID wins (preserves source
    // app's free-form UID); otherwise the task's own UUID.
    let uid = task
        .extra_properties
        .get(VTODO_UID_KEY)
        .cloned()
        .unwrap_or_else(|| task.uuid.clone());

    // CATEGORIES = task tags minus the priority-N tag (if any).
    // PRIORITY is derived from the priority-N tag.
    let mut categories: Vec<String> = Vec::new();
    let mut priority: Option<u8> = None;
    if let Some(names) = tags {
        for name in names {
            if let Some(p) = priority_from_tag(name) {
                priority = Some(p);
            } else {
                categories.push(name.clone());
            }
        }
    }

    let status = derive_status(task);

    let dtstart = task.scheduled_for.as_ref().and_then(|s| match s {
        ScheduledFor::Date(d) => Some(date_or_datetime_from_task(*d, task.scheduled_time)),
        ScheduledFor::Someday => None,
    });
    let due = task.deadline.map(EmitDate::from_date);

    // LOCATION pulled from the round-trip stash; extras minus
    // VTODO_UID and VTODO_LOCATION render as X-*.
    let location = task.extra_properties.get(VTODO_LOCATION_KEY).cloned();

    let mut extra_properties: Vec<(String, String)> = Vec::new();
    for (key, value) in &task.extra_properties {
        if key == VTODO_UID_KEY || key == VTODO_LOCATION_KEY {
            continue;
        }
        extra_properties.push((key.clone(), value.clone()));
    }

    VtodoOutput {
        uid,
        dtstamp: task.modified_at,
        summary: Some(task.title.clone()),
        description: (!task.note.is_empty()).then(|| task.note.clone()),
        dtstart,
        due,
        completed: task.completed_at,
        status: Some(status),
        priority,
        categories,
        rrule: task.repeat_rule.clone(),
        location,
        extra_properties,
    }
}

fn date_or_datetime_from_task(date: NaiveDate, time: Option<chrono::NaiveTime>) -> EmitDate {
    match time {
        Some(t) => EmitDate::from_date_time(date, t),
        None => EmitDate::from_date(date),
    }
}

fn derive_status(task: &Task) -> String {
    // orig_keyword wins for round-trip parity — IN-PROCESS /
    // CANCELLED stash there during import.
    if let Some(ok) = &task.orig_keyword
        && matches!(
            ok.as_str(),
            "NEEDS-ACTION" | "IN-PROCESS" | "COMPLETED" | "CANCELLED"
        )
    {
        return ok.clone();
    }
    if task.completed_at.is_some() {
        "COMPLETED".to_string()
    } else {
        "NEEDS-ACTION".to_string()
    }
}

fn priority_tag_name(priority: Option<u8>) -> Option<String> {
    match priority {
        Some(n @ 1..=4) => Some(format!("priority-{n}")),
        _ => None,
    }
}

fn priority_from_tag(name: &str) -> Option<u8> {
    name.strip_prefix("priority-").and_then(|n| n.parse().ok())
}

/// Convert a parsed VTODO UID into `(task.uuid, Option<stash>)`.
/// UUID-shaped UIDs thread directly; everything else derives
/// a v5 UUID and surfaces the original for stashing.
fn resolve_uid(uid: Option<&str>) -> (String, Option<String>) {
    let Some(raw) = uid else {
        return (Uuid::new_v4().to_string(), None);
    };
    if Uuid::parse_str(raw).is_ok() {
        return (raw.to_string(), None);
    }
    let derived = Uuid::new_v5(&VTODO_NAMESPACE, raw.as_bytes()).to_string();
    (derived, Some(raw.to_string()))
}

fn record_vtodo_lossy(summary: &mut ImportSummary, vtodo: &VtodoComponent) {
    let title = vtodo.summary.clone();
    if vtodo.alarm_count > 0 {
        summary.lossy.push(LossyEntry {
            kind: LossyKind::DroppedAlarm,
            task_title: title.clone(),
            raw: format!("{} alarm(s)", vtodo.alarm_count),
        });
    }
    if vtodo.attendee_count > 0 {
        summary.lossy.push(LossyEntry {
            kind: LossyKind::DroppedAttendee,
            task_title: title.clone(),
            raw: format!("{} attendee(s)", vtodo.attendee_count),
        });
    }
    if vtodo.has_geo {
        summary.lossy.push(LossyEntry {
            kind: LossyKind::DroppedGeo,
            task_title: title.clone(),
            raw: "GEO".to_string(),
        });
    }
    if let Some(pct) = vtodo.percent_complete {
        summary.lossy.push(LossyEntry {
            kind: LossyKind::DroppedPercentComplete,
            task_title: title.clone(),
            raw: format!("PERCENT-COMPLETE={pct}"),
        });
    }
    if vtodo.has_duration {
        summary.lossy.push(LossyEntry {
            kind: LossyKind::DroppedDuration,
            task_title: title.clone(),
            raw: "DURATION".to_string(),
        });
    }
    if vtodo.had_timezone {
        summary.lossy.push(LossyEntry {
            kind: LossyKind::DroppedTimezone,
            task_title: title.clone(),
            raw: "TZID parameter".to_string(),
        });
    }
    // Group unknown property names — one entry per distinct
    // name, with a count when the same name appeared more
    // than once.
    let mut seen: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for name in &vtodo.unknown_property_names {
        *seen.entry(name.as_str()).or_insert(0) += 1;
    }
    for (name, count) in seen {
        summary.lossy.push(LossyEntry {
            kind: LossyKind::UnknownProperty,
            task_title: title.clone(),
            raw: if count == 1 {
                name.to_string()
            } else {
                format!("{name} (×{count})")
            },
        });
    }
}

fn count_tags_for_dry_run(summary: &mut ImportSummary, vtodo: &VtodoComponent) {
    for _ in &vtodo.categories {
        summary.tags_created += 1;
    }
    if priority_tag_name(vtodo.priority).is_some() {
        summary.tags_created += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vtodo::parser::DateOrDateTime;

    #[test]
    fn resolve_uid_threads_through_when_uuid() {
        let uid = "11111111-2222-3333-4444-555555555555";
        let (out, stash) = resolve_uid(Some(uid));
        assert_eq!(out, uid);
        assert_eq!(stash, None);
    }

    #[test]
    fn resolve_uid_derives_v5_and_stashes_when_not_uuid() {
        let raw = "task-1234@nextcloud.example.com";
        let (derived, stash) = resolve_uid(Some(raw));
        assert!(Uuid::parse_str(&derived).is_ok());
        assert_eq!(stash.as_deref(), Some(raw));
        // Same input → same derived UUID (frozen namespace).
        let (again, _) = resolve_uid(Some(raw));
        assert_eq!(derived, again);
    }

    #[test]
    fn resolve_uid_generates_when_missing() {
        let (out, stash) = resolve_uid(None);
        assert!(Uuid::parse_str(&out).is_ok());
        assert_eq!(stash, None);
    }

    #[test]
    fn priority_tag_name_only_for_1_through_4() {
        assert_eq!(priority_tag_name(Some(1)).as_deref(), Some("priority-1"));
        assert_eq!(priority_tag_name(Some(4)).as_deref(), Some("priority-4"));
        assert_eq!(priority_tag_name(Some(5)), None);
        assert_eq!(priority_tag_name(Some(0)), None);
        assert_eq!(priority_tag_name(None), None);
    }

    #[test]
    fn priority_from_tag_round_trips_priority_n() {
        assert_eq!(priority_from_tag("priority-1"), Some(1));
        assert_eq!(priority_from_tag("priority-4"), Some(4));
        assert_eq!(priority_from_tag("priority-x"), None);
        assert_eq!(priority_from_tag("home"), None);
    }

    #[test]
    fn date_or_datetime_from_task_preserves_time_when_present() {
        let d = NaiveDate::from_ymd_opt(2026, 6, 1).unwrap();
        let t = chrono::NaiveTime::from_hms_opt(9, 0, 0).unwrap();
        let v = date_or_datetime_from_task(d, Some(t));
        assert!(matches!(v, EmitDate::DateTime(_)));
        let v2 = date_or_datetime_from_task(d, None);
        assert!(matches!(v2, EmitDate::Date(_)));
    }

    fn vtodo_component_with(summary: &str) -> VtodoComponent {
        VtodoComponent {
            uid: Some("u".into()),
            summary: Some(summary.into()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn import_dry_run_counts_without_db_write() {
        let conn = atrium_core::db::open(std::path::Path::new(":memory:")).unwrap();
        let (handle, _changes, _library) = atrium_core::spawn_worker(conn);

        let parsed = crate::vtodo::parser::ParsedIcs {
            vtodos: vec![
                {
                    let mut v = vtodo_component_with("one");
                    v.categories = vec!["home".into()];
                    v.priority = Some(1);
                    v
                },
                vtodo_component_with("two"),
            ],
            unsupported_top_level: vec!["VEVENT".into()],
        };
        let summary = import_vtodo(&handle, &parsed, "Inbox", true).await.unwrap();
        assert_eq!(summary.project_id, None);
        assert_eq!(summary.tasks_created, 2);
        // 1 CATEGORIES tag + 1 priority-1 tag.
        assert_eq!(summary.tags_created, 2);
        assert!(
            summary
                .lossy
                .iter()
                .any(|l| l.kind == LossyKind::UnsupportedComponent),
        );
    }

    #[tokio::test]
    async fn import_records_lossy_for_alarms_attendees_geo() {
        let conn = atrium_core::db::open(std::path::Path::new(":memory:")).unwrap();
        let (handle, _changes, _library) = atrium_core::spawn_worker(conn);

        let mut v = vtodo_component_with("ping");
        v.alarm_count = 2;
        v.attendee_count = 3;
        v.has_geo = true;
        v.percent_complete = Some(40);
        v.has_duration = true;
        v.had_timezone = true;
        v.unknown_property_names = vec!["RESOURCES".into(), "RESOURCES".into(), "URL".into()];

        let parsed = crate::vtodo::parser::ParsedIcs {
            vtodos: vec![v],
            unsupported_top_level: Vec::new(),
        };
        let summary = import_vtodo(&handle, &parsed, "Lossy", false)
            .await
            .unwrap();
        assert!(
            summary
                .lossy
                .iter()
                .any(|l| l.kind == LossyKind::DroppedAlarm)
        );
        assert!(
            summary
                .lossy
                .iter()
                .any(|l| l.kind == LossyKind::DroppedAttendee),
        );
        assert!(
            summary
                .lossy
                .iter()
                .any(|l| l.kind == LossyKind::DroppedGeo)
        );
        assert!(
            summary
                .lossy
                .iter()
                .any(|l| l.kind == LossyKind::DroppedPercentComplete),
        );
        assert!(
            summary
                .lossy
                .iter()
                .any(|l| l.kind == LossyKind::DroppedDuration),
        );
        assert!(
            summary
                .lossy
                .iter()
                .any(|l| l.kind == LossyKind::DroppedTimezone),
        );
        // RESOURCES (×2) collapses to a single grouped entry;
        // URL renders separately. Total unknown-property
        // entries: 2.
        let unknown: Vec<&LossyEntry> = summary
            .lossy
            .iter()
            .filter(|l| l.kind == LossyKind::UnknownProperty)
            .collect();
        assert_eq!(unknown.len(), 2);
        assert!(unknown.iter().any(|l| l.raw.contains("RESOURCES")));
        assert!(unknown.iter().any(|l| l.raw == "URL"));
    }

    #[tokio::test]
    async fn import_writes_one_task_per_vtodo_with_summary_counts() {
        // Deeper field-level assertions belong in the
        // integration test (`tests/vtodo_round_trip.rs`),
        // which uses a file-backed DB so it can spawn a
        // separate read connection. Here we exercise the
        // dispatch + count side.
        let conn = atrium_core::db::open(std::path::Path::new(":memory:")).unwrap();
        let (handle, _changes, _library) = atrium_core::spawn_worker(conn);

        let v = VtodoComponent {
            uid: Some("nc-task-1@example.com".into()),
            summary: Some("Pay rent".into()),
            description: Some("Use the new account".into()),
            dtstart: Some(DateOrDateTime::Date(
                NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            )),
            due: Some(DateOrDateTime::Date(
                NaiveDate::from_ymd_opt(2026, 6, 5).unwrap(),
            )),
            status: Some("NEEDS-ACTION".into()),
            priority: Some(1),
            categories: vec!["home".into(), "finance".into()],
            rrule: Some("FREQ=MONTHLY".into()),
            location: Some("Bank lobby".into()),
            x_properties: vec![("X-CUSTOM".into(), "v".into())],
            ..Default::default()
        };
        let parsed = crate::vtodo::parser::ParsedIcs {
            vtodos: vec![v],
            unsupported_top_level: Vec::new(),
        };
        let summary = import_vtodo(&handle, &parsed, "Bills", false)
            .await
            .unwrap();
        assert_eq!(summary.tasks_created, 1);
        assert!(summary.project_id.is_some());
        // 2 CATEGORIES tags + 1 priority-1 tag = 3 distinct.
        assert_eq!(summary.tags_created, 3);
    }
}
