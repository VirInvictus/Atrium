// SPDX-License-Identifier: MIT
//! Taskwarrior mapper — turns parsed [`TaskwarriorTask`]s into
//! worker calls plus a [`LossyEntry`] report. v0.26.0.
//!
//! Mirrors the Todoist + VTODO mapper shapes: dry-run support,
//! per-source LossyKind, stable v5 UUID namespace (unused here
//! because Taskwarrior UIDs are already RFC 4122 UUIDs, but
//! kept exported for symmetry with the other importers).
//!
//! Field mapping is documented in `foamy-churning-summit.md`.
//! The mapper preserves the documented spec §7.5 lossy-report
//! shape (one entry per dropped construct, attached to the
//! task title when surfaced inside a VTODO/task block).

use std::collections::HashSet;

use atrium_core::error::DbError;
use atrium_core::{NewProject, NewTask, ScheduledFor, WorkerHandle};
use uuid::Uuid;

use super::UdaPolicy;

use super::parser::{Annotation, TaskwarriorTask};

/// Stable namespace UUID for any future v5 derivations the
/// Taskwarrior importer might need (e.g. a fallback for tasks
/// whose `uuid` field is malformed). Frozen byte pattern;
/// changes break re-import stability.
///
/// Unused on the happy path — Taskwarrior UUIDs are RFC 4122
/// already — but exported for parity with the other importers'
/// stable-ID story.
#[allow(dead_code)]
pub const TASKWARRIOR_NAMESPACE: Uuid = Uuid::from_bytes([
    0x5e, 0xa2, 0x9b, 0xc4, 0x7f, 0x37, 0x4b, 0x12, 0xaf, 0x4c, 0x73, 0x16, 0xe5, 0xb1, 0x4a, 0x90,
]);

#[derive(Debug, Clone, Default)]
pub struct ImportSummary {
    /// `None` on dry-run.
    pub project_id: Option<i64>,
    pub project_title: String,
    pub tasks_created: usize,
    /// Distinct tag names ensured during the run (raw `tags`
    /// plus `priority-N` plus UDA tags). The worker dedupes;
    /// this is the label set seen by the run.
    pub tags_created: usize,
    /// Per-row notes about fields Atrium dropped on import.
    pub lossy: Vec<LossyEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LossyEntry {
    pub kind: LossyKind,
    pub task_title: Option<String>,
    pub raw: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LossyKind {
    /// `status:deleted` rows are dropped from import (deleted
    /// state isn't a productive Atrium surface).
    Deleted,
    /// Task had a `start` field — Taskwarrior's "active task"
    /// marker. Atrium has no per-task active state.
    ActiveAtImport,
    /// `until` date — recurring-series end date. Atrium's
    /// `repeat_rule` accepts `UNTIL=` but we don't emit it for
    /// `recur:` translations in v0.26.0.
    DroppedUntil,
    /// `recur` string couldn't be translated to RFC 5545
    /// RRULE. The task lands without a recurrence rule and
    /// gets one lossy entry.
    UnparseableRecurrence,
    /// `parent` / `mask` / `imask` — the Taskwarrior child-row
    /// machinery for recurring tasks. Each row becomes a
    /// standalone Atrium task in v0.26.0.
    DroppedRecurringChild,
    /// `status:recurring` parent template — its per-occurrence
    /// children land normally, but the template itself drops.
    DroppedRecurringTemplate,
    /// `depends` UUIDs — Atrium will gain dependencies at
    /// v0.29.0. Re-import after that ships will round-trip.
    DroppedDepends,
    /// One UDA field surfaced as a lossy entry because the
    /// user picked `--uda-as drop`.
    DroppedUda,
}

#[derive(Debug, thiserror::Error)]
pub enum MapError {
    #[error("worker error: {0}")]
    Worker(#[from] DbError),
}

/// Apply a parsed Taskwarrior task stream to the worker.
/// Creates a fresh project named `project_name`, walks the
/// tasks in source order, and writes one Atrium row per
/// non-skipped Taskwarrior task.
pub async fn import_taskwarrior(
    handle: &WorkerHandle,
    tasks: &[TaskwarriorTask],
    project_name: &str,
    uda_as: UdaPolicy,
    dry_run: bool,
) -> Result<ImportSummary, MapError> {
    let mut summary = ImportSummary {
        project_title: project_name.to_string(),
        ..Default::default()
    };

    if dry_run {
        let mut ensured_tags: HashSet<String> = HashSet::new();
        for task in tasks {
            if !should_create(task, &mut summary) {
                continue;
            }
            summary.tasks_created += 1;
            count_dry_run_tags(task, uda_as, &mut ensured_tags, &mut summary);
            record_lossy(task, uda_as, &mut summary);
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

    let mut ensured_tags: HashSet<String> = HashSet::new();

    for task in tasks {
        if !should_create(task, &mut summary) {
            continue;
        }
        record_lossy(task, uda_as, &mut summary);

        let new_task = build_new_task(task, project.id, uda_as, &mut summary);
        let created = handle.create_task(new_task).await?;
        summary.tasks_created += 1;

        let tag_names = collect_tag_names(task, uda_as);
        let mut tag_ids: Vec<i64> = Vec::with_capacity(tag_names.len());
        for name in &tag_names {
            let tag = handle.ensure_tag(name.clone()).await?;
            if ensured_tags.insert(name.clone()) {
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

/// Decide whether the task earns a row. `status:deleted` and
/// `status:recurring` (the parent template) both skip with a
/// lossy entry; everything else lands.
fn should_create(task: &TaskwarriorTask, summary: &mut ImportSummary) -> bool {
    match task.status.as_deref() {
        Some("deleted") => {
            summary.lossy.push(LossyEntry {
                kind: LossyKind::Deleted,
                task_title: task.description.clone(),
                raw: "status:deleted".to_string(),
            });
            false
        }
        Some("recurring") => {
            summary.lossy.push(LossyEntry {
                kind: LossyKind::DroppedRecurringTemplate,
                task_title: task.description.clone(),
                raw: "status:recurring parent template".to_string(),
            });
            false
        }
        _ => true,
    }
}

fn build_new_task(
    task: &TaskwarriorTask,
    project_id: i64,
    uda_as: UdaPolicy,
    summary: &mut ImportSummary,
) -> NewTask {
    let title = task
        .description
        .clone()
        .unwrap_or_else(|| "(untitled task)".to_string());

    let mut note_lines: Vec<String> = Vec::new();
    for ann in &task.annotations {
        note_lines.push(format_annotation(ann));
    }
    if uda_as == UdaPolicy::Note {
        for (name, value) in &task.udas {
            note_lines.push(format!("UDA: {name}={value}"));
        }
    }
    let note = note_lines.join("\n");

    let scheduled_for = task.scheduled.map(|d| ScheduledFor::Date(d.date()));
    let scheduled_time = task.scheduled.and_then(|d| d.time());
    let deadline = task.due.map(|d| d.date());
    let defer_until = task.wait.map(|d| d.date());

    let (orig_keyword, completed_at) = match task.status.as_deref() {
        Some("completed") => (None, task.end),
        Some("waiting") => (Some("WAITING".to_string()), None),
        _ => (None, None),
    };

    let repeat_rule = task
        .recur
        .as_deref()
        .and_then(|raw| parse_recur(raw, &title, summary));

    NewTask {
        title,
        note,
        project_id: Some(project_id),
        scheduled_for,
        scheduled_time,
        deadline,
        defer_until,
        completed_at,
        repeat_rule,
        uuid: normalise_uuid(task.uuid.as_deref()),
        orig_keyword,
        ..Default::default()
    }
}

fn normalise_uuid(raw: Option<&str>) -> Option<String> {
    raw.and_then(|s| Uuid::parse_str(s).ok())
        .map(|u| u.to_string())
}

fn format_annotation(ann: &Annotation) -> String {
    match ann.entry {
        Some(when) => format!("[{}] {}", when.format("%Y-%m-%d"), ann.description),
        None => format!("[note] {}", ann.description),
    }
}

fn collect_tag_names(task: &TaskwarriorTask, uda_as: UdaPolicy) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in &task.tags {
        out.push(tag.clone());
    }
    if let Some(name) = priority_tag_name(task.priority.as_deref()) {
        out.push(name);
    }
    if uda_as == UdaPolicy::Tag {
        for (name, value) in &task.udas {
            out.push(format!("{name}-{value}"));
        }
    }
    out
}

fn priority_tag_name(priority: Option<&str>) -> Option<String> {
    match priority {
        Some("H") => Some("priority-1".to_string()),
        Some("M") => Some("priority-2".to_string()),
        Some("L") => Some("priority-3".to_string()),
        _ => None,
    }
}

/// Tiny subset of Taskwarrior's `recur` grammar. Supports
/// `<N><unit>` where N is a positive integer and unit is one
/// of `d` / `days` / `wks` / `weeks` / `mo` / `month` / `months`
/// / `yr` / `years` / `year`. Anything else returns None +
/// records a lossy entry.
fn parse_recur(raw: &str, title: &str, summary: &mut ImportSummary) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some(rrule) = parse_recur_inner(raw) {
        return Some(rrule);
    }
    summary.lossy.push(LossyEntry {
        kind: LossyKind::UnparseableRecurrence,
        task_title: Some(title.to_string()),
        raw: format!("recur:{raw}"),
    });
    None
}

fn parse_recur_inner(raw: &str) -> Option<String> {
    let split = raw
        .char_indices()
        .find(|(_, ch)| ch.is_ascii_alphabetic())
        .map(|(i, _)| i)?;
    let (num, unit) = raw.split_at(split);
    let n: u32 = num.parse().ok()?;
    if n == 0 {
        return None;
    }
    let interval = if n == 1 {
        String::new()
    } else {
        format!(";INTERVAL={n}")
    };
    let freq = match unit.to_ascii_lowercase().as_str() {
        "d" | "day" | "days" | "daily" => "DAILY",
        "w" | "wk" | "wks" | "week" | "weeks" | "weekly" => "WEEKLY",
        "mo" | "mon" | "mth" | "month" | "months" | "monthly" => "MONTHLY",
        "y" | "yr" | "yrs" | "year" | "years" | "yearly" | "annually" => "YEARLY",
        _ => return None,
    };
    Some(format!("FREQ={freq}{interval}"))
}

fn record_lossy(task: &TaskwarriorTask, uda_as: UdaPolicy, summary: &mut ImportSummary) {
    let title = task.description.clone();
    if task.start.is_some() {
        summary.lossy.push(LossyEntry {
            kind: LossyKind::ActiveAtImport,
            task_title: title.clone(),
            raw: "start (task was active)".to_string(),
        });
    }
    if task.until.is_some() {
        summary.lossy.push(LossyEntry {
            kind: LossyKind::DroppedUntil,
            task_title: title.clone(),
            raw: "until".to_string(),
        });
    }
    if task.parent.is_some() || task.mask.is_some() || task.imask.is_some() {
        summary.lossy.push(LossyEntry {
            kind: LossyKind::DroppedRecurringChild,
            task_title: title.clone(),
            raw: "recurring child (parent / mask / imask)".to_string(),
        });
    }
    if task.depends.is_some() {
        summary.lossy.push(LossyEntry {
            kind: LossyKind::DroppedDepends,
            task_title: title.clone(),
            raw: "depends — task dependencies arrive at v0.29.0".to_string(),
        });
    }
    if uda_as == UdaPolicy::Drop {
        // Group all dropped UDAs into a single entry per task
        // (one line is enough; the user already opted for
        // "drop" so detail granularity isn't load-bearing).
        if !task.udas.is_empty() {
            let names: Vec<&str> = task.udas.keys().map(String::as_str).collect();
            summary.lossy.push(LossyEntry {
                kind: LossyKind::DroppedUda,
                task_title: title,
                raw: format!("UDA: {}", names.join(", ")),
            });
        }
    }
}

fn count_dry_run_tags(
    task: &TaskwarriorTask,
    uda_as: UdaPolicy,
    ensured: &mut HashSet<String>,
    summary: &mut ImportSummary,
) {
    let names = collect_tag_names(task, uda_as);
    for name in names {
        if ensured.insert(name) {
            summary.tags_created += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import::taskwarrior::parser::{DateOrDateTime, TaskwarriorTask};
    use chrono::{NaiveDate, TimeZone, Utc};

    fn task(description: &str) -> TaskwarriorTask {
        TaskwarriorTask {
            uuid: Some("11111111-2222-3333-4444-555555555555".to_string()),
            description: Some(description.to_string()),
            status: Some("pending".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn priority_tag_name_covers_h_m_l() {
        assert_eq!(priority_tag_name(Some("H")).as_deref(), Some("priority-1"));
        assert_eq!(priority_tag_name(Some("M")).as_deref(), Some("priority-2"));
        assert_eq!(priority_tag_name(Some("L")).as_deref(), Some("priority-3"));
        assert_eq!(priority_tag_name(Some("X")), None);
        assert_eq!(priority_tag_name(None), None);
    }

    #[test]
    fn parse_recur_inner_handles_supported_units() {
        assert_eq!(
            parse_recur_inner("3wks"),
            Some("FREQ=WEEKLY;INTERVAL=3".into())
        );
        assert_eq!(parse_recur_inner("1d"), Some("FREQ=DAILY".into()));
        assert_eq!(
            parse_recur_inner("2month"),
            Some("FREQ=MONTHLY;INTERVAL=2".into())
        );
        assert_eq!(parse_recur_inner("1yearly"), Some("FREQ=YEARLY".into()));
        assert_eq!(parse_recur_inner("4hours"), None);
        assert_eq!(parse_recur_inner("foo"), None);
        assert_eq!(parse_recur_inner("0wks"), None);
    }

    #[test]
    fn normalise_uuid_round_trips_v4() {
        let uuid = "11111111-2222-3333-4444-555555555555";
        assert_eq!(normalise_uuid(Some(uuid)).as_deref(), Some(uuid));
    }

    #[test]
    fn normalise_uuid_rejects_garbage() {
        assert_eq!(normalise_uuid(Some("not a uuid")), None);
    }

    #[test]
    fn collect_tag_names_uda_as_tag_includes_uda_pairs() {
        let mut t = task("with udas");
        t.tags = vec!["home".into(), "work".into()];
        t.priority = Some("H".into());
        t.udas.insert("effort".into(), "large".into());
        t.udas.insert("client".into(), "Acme".into());
        let names = collect_tag_names(&t, UdaPolicy::Tag);
        // tags + priority + UDA(name-value)
        assert!(names.contains(&"home".to_string()));
        assert!(names.contains(&"work".to_string()));
        assert!(names.contains(&"priority-1".to_string()));
        assert!(names.contains(&"client-Acme".to_string()));
        assert!(names.contains(&"effort-large".to_string()));
    }

    #[test]
    fn collect_tag_names_uda_as_note_excludes_udas() {
        let mut t = task("with udas");
        t.udas.insert("effort".into(), "large".into());
        let names = collect_tag_names(&t, UdaPolicy::Note);
        assert!(!names.iter().any(|n| n.starts_with("effort-")));
    }

    #[test]
    fn format_annotation_includes_entry_date() {
        let ann = Annotation {
            entry: Some(Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap()),
            description: "first note".into(),
        };
        assert_eq!(format_annotation(&ann), "[2026-01-01] first note");
    }

    #[test]
    fn should_create_skips_deleted_and_recurring_template() {
        let mut summary = ImportSummary::default();
        let mut t = task("deleted one");
        t.status = Some("deleted".into());
        assert!(!should_create(&t, &mut summary));
        let mut t2 = task("recurring template");
        t2.status = Some("recurring".into());
        assert!(!should_create(&t2, &mut summary));
        assert_eq!(summary.lossy.len(), 2);
        assert!(summary.lossy.iter().any(|l| l.kind == LossyKind::Deleted));
        assert!(
            summary
                .lossy
                .iter()
                .any(|l| l.kind == LossyKind::DroppedRecurringTemplate),
        );
    }

    #[test]
    fn record_lossy_flags_start_until_recurring_child_depends_and_udas() {
        let mut summary = ImportSummary::default();
        let mut t = task("complex");
        t.start = Some(Utc.with_ymd_and_hms(2026, 5, 1, 10, 0, 0).unwrap());
        t.until = Some(DateOrDateTime::Date(
            NaiveDate::from_ymd_opt(2026, 12, 31).unwrap(),
        ));
        t.parent = Some("11111111-1111-1111-1111-111111111111".into());
        t.mask = Some("--".into());
        t.imask = Some(2);
        t.depends = Some("a,b,c".into());
        t.udas.insert("custom".into(), "v".into());
        record_lossy(&t, UdaPolicy::Drop, &mut summary);
        let kinds: Vec<LossyKind> = summary.lossy.iter().map(|l| l.kind).collect();
        assert!(kinds.contains(&LossyKind::ActiveAtImport));
        assert!(kinds.contains(&LossyKind::DroppedUntil));
        assert!(kinds.contains(&LossyKind::DroppedRecurringChild));
        assert!(kinds.contains(&LossyKind::DroppedDepends));
        assert!(kinds.contains(&LossyKind::DroppedUda));
    }

    #[test]
    fn build_new_task_routes_modeled_fields() {
        let mut t = task("file taxes");
        t.priority = Some("H".into());
        t.due = Some(DateOrDateTime::Date(
            NaiveDate::from_ymd_opt(2026, 4, 15).unwrap(),
        ));
        t.scheduled = Some(DateOrDateTime::DateTime(
            Utc.with_ymd_and_hms(2026, 3, 1, 9, 0, 0).unwrap(),
        ));
        t.wait = Some(DateOrDateTime::Date(
            NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
        ));
        t.status = Some("completed".into());
        t.end = Some(Utc.with_ymd_and_hms(2026, 4, 16, 10, 0, 0).unwrap());
        t.annotations.push(Annotation {
            entry: Some(Utc.with_ymd_and_hms(2026, 3, 5, 8, 0, 0).unwrap()),
            description: "remember the W-2".into(),
        });
        let mut summary = ImportSummary::default();
        let nt = build_new_task(&t, 7, UdaPolicy::Tag, &mut summary);
        assert_eq!(nt.title, "file taxes");
        assert_eq!(nt.project_id, Some(7));
        assert_eq!(
            nt.deadline,
            Some(NaiveDate::from_ymd_opt(2026, 4, 15).unwrap()),
        );
        assert!(matches!(
            nt.scheduled_for,
            Some(ScheduledFor::Date(d)) if d == NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()
        ));
        assert!(nt.scheduled_time.is_some());
        assert_eq!(
            nt.defer_until,
            Some(NaiveDate::from_ymd_opt(2026, 2, 1).unwrap()),
        );
        assert!(nt.completed_at.is_some());
        assert!(nt.note.contains("[2026-03-05] remember the W-2"));
    }

    #[test]
    fn build_new_task_uda_as_note_appends_uda_lines() {
        let mut t = task("with udas");
        t.udas.insert("client".into(), "Acme".into());
        t.udas.insert("effort".into(), "large".into());
        let mut summary = ImportSummary::default();
        let nt = build_new_task(&t, 1, UdaPolicy::Note, &mut summary);
        assert!(nt.note.contains("UDA: client=Acme"));
        assert!(nt.note.contains("UDA: effort=large"));
    }

    #[test]
    fn build_new_task_waiting_status_stashes_orig_keyword() {
        let mut t = task("waiting one");
        t.status = Some("waiting".into());
        let mut summary = ImportSummary::default();
        let nt = build_new_task(&t, 1, UdaPolicy::Tag, &mut summary);
        assert_eq!(nt.orig_keyword.as_deref(), Some("WAITING"));
    }

    #[test]
    fn build_new_task_recur_translates_to_rrule() {
        let mut t = task("recurring");
        t.recur = Some("3wks".into());
        let mut summary = ImportSummary::default();
        let nt = build_new_task(&t, 1, UdaPolicy::Tag, &mut summary);
        assert_eq!(nt.repeat_rule.as_deref(), Some("FREQ=WEEKLY;INTERVAL=3"));
    }

    #[test]
    fn build_new_task_unparseable_recur_emits_lossy_entry() {
        let mut t = task("bad recur");
        t.recur = Some("hourly".into());
        let mut summary = ImportSummary::default();
        let nt = build_new_task(&t, 1, UdaPolicy::Tag, &mut summary);
        assert!(nt.repeat_rule.is_none());
        assert!(
            summary
                .lossy
                .iter()
                .any(|l| l.kind == LossyKind::UnparseableRecurrence),
        );
    }

    #[tokio::test]
    async fn import_dry_run_counts_without_db_write() {
        let conn = atrium_core::db::open(std::path::Path::new(":memory:")).unwrap();
        let (handle, _changes, _library) = atrium_core::spawn_worker(conn);

        let mut t1 = task("one");
        t1.tags = vec!["home".into()];
        t1.priority = Some("H".into());
        let mut t2 = task("two");
        t2.status = Some("deleted".into());
        let summary = import_taskwarrior(&handle, &[t1, t2], "Inbox", UdaPolicy::Tag, true)
            .await
            .unwrap();
        assert_eq!(summary.project_id, None);
        // t2 is skipped (deleted); t1 lands.
        assert_eq!(summary.tasks_created, 1);
        // home + priority-1 = 2 distinct tags.
        assert_eq!(summary.tags_created, 2);
        assert!(summary.lossy.iter().any(|l| l.kind == LossyKind::Deleted));
    }
}
