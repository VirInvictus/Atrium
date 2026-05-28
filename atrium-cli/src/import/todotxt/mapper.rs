// SPDX-License-Identifier: MIT
//! todo.txt mapper. v0.27.0. Turns parsed [`TodoTxtTask`]s
//! into worker calls plus a [`LossyEntry`] report.

use std::collections::HashSet;

use atrium_core::error::DbError;
use atrium_core::{NewProject, NewTask, ScheduledFor, WorkerHandle};
use chrono::{NaiveDate, TimeZone, Utc};
use uuid::Uuid;

use super::parser::TodoTxtTask;

/// Frozen v5 namespace for todo.txt rows. Lines don't carry
/// UIDs, so we derive `task.uuid = UUIDv5(TODOTXT_NAMESPACE,
/// "<project_name>|<line content>")` to keep re-imports
/// idempotent. Distinct byte pattern from the other namespaces.
pub const TODOTXT_NAMESPACE: Uuid = Uuid::from_bytes([
    0x42, 0xf0, 0x1d, 0x88, 0x9a, 0xe6, 0x40, 0x3c, 0xb7, 0x5f, 0x21, 0x99, 0xc4, 0x6d, 0x87, 0x14,
]);

#[derive(Debug, Clone, Default)]
pub struct ImportSummary {
    /// `None` on dry-run.
    pub project_id: Option<i64>,
    pub project_title: String,
    pub tasks_created: usize,
    /// Distinct tag names ensured during the run (`@context`
    /// values plus `priority-N`).
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
    /// Inline `+project` tokens dropped — `--into` wins.
    DroppedInlineProject,
    /// Priority `(D)` through `(Z)` — todo.txt convention
    /// rarely uses these and Atrium has no schema home beyond
    /// the `priority-1/2/3` mirror of Todoist + Taskwarrior.
    PriorityBelowC,
    /// Unknown `key:value` extension that wasn't `due:` or
    /// `t:`. The value drops with one lossy entry per
    /// occurrence.
    DroppedKeyValue,
}

#[derive(Debug, thiserror::Error)]
pub enum MapError {
    #[error("worker error: {0}")]
    Worker(#[from] DbError),
}

/// Apply a parsed todo.txt task stream to the worker.
pub async fn import_todotxt(
    handle: &WorkerHandle,
    tasks: &[TodoTxtTask],
    project_name: &str,
    dry_run: bool,
) -> Result<ImportSummary, MapError> {
    let mut summary = ImportSummary {
        project_title: project_name.to_string(),
        ..Default::default()
    };

    if dry_run {
        let mut ensured: HashSet<String> = HashSet::new();
        for t in tasks {
            summary.tasks_created += 1;
            record_lossy(t, &mut summary);
            for name in collect_tag_names(t) {
                if ensured.insert(name) {
                    summary.tags_created += 1;
                }
            }
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

    let mut ensured: HashSet<String> = HashSet::new();

    for t in tasks {
        record_lossy(t, &mut summary);

        let new_task = build_new_task(t, project.id, project_name);
        let created = handle.create_task(new_task).await?;
        summary.tasks_created += 1;

        let tag_names = collect_tag_names(t);
        let mut tag_ids: Vec<i64> = Vec::with_capacity(tag_names.len());
        for name in &tag_names {
            let tag = handle.ensure_tag(name.clone()).await?;
            if ensured.insert(name.clone()) {
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

fn build_new_task(t: &TodoTxtTask, project_id: i64, project_name: &str) -> NewTask {
    let title = if t.description.is_empty() {
        "(untitled task)".to_string()
    } else {
        t.description.clone()
    };

    let deadline = key_value_date(t, "due");
    let defer_until = key_value_date(t, "t");
    let completed_at = t
        .completion_date
        .map(naive_date_to_utc_midnight)
        .or_else(|| {
            // `x ` without an explicit completion date — stamp now.
            if t.completed { Some(Utc::now()) } else { None }
        });

    let uuid_seed = format!("{project_name}|{title}|{}", t.creation_date_as_string());
    let uuid = Some(Uuid::new_v5(&TODOTXT_NAMESPACE, uuid_seed.as_bytes()).to_string());

    NewTask {
        title,
        project_id: Some(project_id),
        deadline,
        defer_until,
        completed_at,
        scheduled_for: t
            .creation_date
            .map(ScheduledFor::Date)
            .filter(|_| !t.completed),
        uuid,
        ..Default::default()
    }
}

fn collect_tag_names(t: &TodoTxtTask) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for ctx in &t.contexts {
        out.push(ctx.clone());
    }
    if let Some(name) = priority_tag_name(t.priority) {
        out.push(name);
    }
    out
}

fn priority_tag_name(priority: Option<char>) -> Option<String> {
    match priority {
        Some('A') => Some("priority-1".to_string()),
        Some('B') => Some("priority-2".to_string()),
        Some('C') => Some("priority-3".to_string()),
        _ => None,
    }
}

fn key_value_date(t: &TodoTxtTask, key: &str) -> Option<NaiveDate> {
    t.key_values
        .iter()
        .find(|(k, _)| k == key)
        .and_then(|(_, v)| NaiveDate::parse_from_str(v, "%Y-%m-%d").ok())
}

fn naive_date_to_utc_midnight(d: NaiveDate) -> chrono::DateTime<Utc> {
    Utc.from_utc_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
}

fn record_lossy(t: &TodoTxtTask, summary: &mut ImportSummary) {
    let title = if t.description.is_empty() {
        None
    } else {
        Some(t.description.clone())
    };
    for project in &t.projects {
        summary.lossy.push(LossyEntry {
            kind: LossyKind::DroppedInlineProject,
            task_title: title.clone(),
            raw: format!("+{project}"),
        });
    }
    if let Some(letter) = t.priority
        && !matches!(letter, 'A' | 'B' | 'C')
    {
        summary.lossy.push(LossyEntry {
            kind: LossyKind::PriorityBelowC,
            task_title: title.clone(),
            raw: format!("({letter})"),
        });
    }
    for (key, value) in &t.key_values {
        if matches!(key.as_str(), "due" | "t") {
            continue;
        }
        summary.lossy.push(LossyEntry {
            kind: LossyKind::DroppedKeyValue,
            task_title: title.clone(),
            raw: format!("{key}:{value}"),
        });
    }
}

trait CreationDateAsString {
    fn creation_date_as_string(&self) -> String;
}

impl CreationDateAsString for TodoTxtTask {
    fn creation_date_as_string(&self) -> String {
        self.creation_date
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "-".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(description: &str) -> TodoTxtTask {
        TodoTxtTask {
            description: description.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn priority_a_b_c_map_to_priority_n_tags() {
        assert_eq!(priority_tag_name(Some('A')).as_deref(), Some("priority-1"));
        assert_eq!(priority_tag_name(Some('B')).as_deref(), Some("priority-2"));
        assert_eq!(priority_tag_name(Some('C')).as_deref(), Some("priority-3"));
        assert_eq!(priority_tag_name(Some('D')), None);
        assert_eq!(priority_tag_name(None), None);
    }

    #[test]
    fn collect_tag_names_includes_contexts_and_priority() {
        let mut t = task("buy milk");
        t.contexts = vec!["home".into(), "errand".into()];
        t.priority = Some('A');
        let names = collect_tag_names(&t);
        assert!(names.contains(&"home".to_string()));
        assert!(names.contains(&"errand".to_string()));
        assert!(names.contains(&"priority-1".to_string()));
    }

    #[test]
    fn key_value_date_extracts_due_field() {
        let mut t = task("d");
        t.key_values.push(("due".into(), "2026-05-01".into()));
        assert_eq!(
            key_value_date(&t, "due"),
            Some(NaiveDate::from_ymd_opt(2026, 5, 1).unwrap()),
        );
        assert_eq!(key_value_date(&t, "t"), None);
    }

    #[test]
    fn record_lossy_flags_inline_project_priority_d_and_unknown_keys() {
        let mut t = task("complex");
        t.projects = vec!["bills".into()];
        t.priority = Some('D');
        t.key_values.push(("due".into(), "2026-05-01".into()));
        t.key_values.push(("foo".into(), "bar".into()));
        let mut summary = ImportSummary::default();
        record_lossy(&t, &mut summary);
        let kinds: Vec<LossyKind> = summary.lossy.iter().map(|l| l.kind).collect();
        assert!(kinds.contains(&LossyKind::DroppedInlineProject));
        assert!(kinds.contains(&LossyKind::PriorityBelowC));
        assert!(kinds.contains(&LossyKind::DroppedKeyValue));
        // `due:` doesn't surface as lossy.
        assert!(!summary.lossy.iter().any(|l| l.raw == "due:2026-05-01"));
    }

    #[test]
    fn build_new_task_routes_modeled_fields() {
        let mut t = task("file taxes");
        t.priority = Some('A');
        t.creation_date = Some(NaiveDate::from_ymd_opt(2026, 3, 1).unwrap());
        t.key_values.push(("due".into(), "2026-04-15".into()));
        t.key_values.push(("t".into(), "2026-04-01".into()));
        let nt = build_new_task(&t, 7, "Inbox");
        assert_eq!(nt.title, "file taxes");
        assert_eq!(nt.project_id, Some(7));
        assert_eq!(
            nt.deadline,
            Some(NaiveDate::from_ymd_opt(2026, 4, 15).unwrap())
        );
        assert_eq!(
            nt.defer_until,
            Some(NaiveDate::from_ymd_opt(2026, 4, 1).unwrap())
        );
        // Creation date threads through as scheduled_for (open
        // tasks); completed tasks skip this path.
        assert!(matches!(
            nt.scheduled_for,
            Some(ScheduledFor::Date(d)) if d == NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()
        ));
        assert!(nt.uuid.is_some());
    }

    #[test]
    fn build_new_task_completed_marker_stamps_completed_at() {
        let mut t = task("done thing");
        t.completed = true;
        t.completion_date = Some(NaiveDate::from_ymd_opt(2026, 4, 20).unwrap());
        let nt = build_new_task(&t, 1, "Inbox");
        assert!(nt.completed_at.is_some());
        assert!(nt.scheduled_for.is_none());
    }

    #[test]
    fn build_new_task_uuid_is_deterministic() {
        let mut t = task("repeat me");
        t.creation_date = Some(NaiveDate::from_ymd_opt(2026, 3, 1).unwrap());
        let a = build_new_task(&t, 1, "Inbox").uuid;
        let b = build_new_task(&t, 1, "Inbox").uuid;
        assert!(a.is_some());
        assert_eq!(a, b, "same input → same v5 UUID");
    }

    #[tokio::test]
    async fn import_dry_run_counts_without_db_write() {
        let conn = atrium_core::db::open(std::path::Path::new(":memory:")).unwrap();
        let (handle, _changes, _library) = atrium_core::spawn_worker(conn);

        let mut t1 = task("one");
        t1.contexts = vec!["home".into()];
        t1.priority = Some('A');
        let mut t2 = task("two");
        t2.projects = vec!["bills".into()];
        let summary = import_todotxt(&handle, &[t1, t2], "Inbox", true)
            .await
            .unwrap();
        assert_eq!(summary.project_id, None);
        assert_eq!(summary.tasks_created, 2);
        // home + priority-1 = 2 distinct.
        assert_eq!(summary.tags_created, 2);
        assert!(
            summary
                .lossy
                .iter()
                .any(|l| l.kind == LossyKind::DroppedInlineProject),
        );
    }
}
