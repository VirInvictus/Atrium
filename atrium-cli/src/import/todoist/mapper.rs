// SPDX-License-Identifier: MIT
//! Todoist mapper — turns a parsed [`TodoistRow`] stream into
//! worker calls that materialise the import in Atrium's DB.
//!
//! Layered above [`super::parser`] (CSV → typed rows) and
//! [`super::recurrence`] (DATE → RRULE). This is the layer that
//! actually talks to a [`WorkerHandle`].
//!
//! # Mapping table (per roadmap §18)
//!
//! | Todoist | Atrium |
//! |---|---|
//! | `meta` row | recorded in `ImportSummary::meta_entries` |
//! | `section` row | `ensure_heading(project, title)` |
//! | `task` row, `INDENT=1` | `create_task(project=…, parent=None)` |
//! | `task` row, `INDENT>1` | nested under most recent ancestor |
//! | `CONTENT` `@label` tokens | `ensure_tag(label)` + `set_task_tags` |
//! | `PRIORITY` 1–3 | `priority-N` tag (4 = default, no tag) |
//! | `DATE` (NL phrase) | `repeat_rule` + `scheduled_for` |
//! | `DESCRIPTION` | `task.note` |
//! | time-of-day, timezone, duration, deadline | lossy report |
//!
//! # Position layout
//!
//! Heading positions are assigned by the worker (`next_heading_
//! position` returns 1.0, 2.0, 3.0, …). Top-level tasks get an
//! explicit position update so they slot between heading rows:
//! a task that's the i-th task under section N lands at
//! `N + i * 0.001`. The Org writer's interleave-by-position
//! contract (write.rs::build_project_tree) reads that ordering
//! and emits each section's tasks as depth-2 children of the
//! preceding heading.
//!
//! Subtasks (`INDENT > 1`) inherit positions from the worker's
//! `next_task_position(parent_id, …)` — that's already a per-
//! parent monotonic counter, no override needed.
//!
//! # Determinism
//!
//! Each task is created with a name-based UUID derived from the
//! project name + the (label-stripped) title via UUID v5. Re-
//! running the importer onto the same project produces stable
//! IDs, which keeps Org-vault `:ID:` round-trip clean across
//! re-imports.

use std::collections::HashSet;

use atrium_core::error::DbError;
use atrium_core::{NewProject, NewTask, ScheduledFor, TaskUpdate, WorkerHandle};
use chrono::NaiveDate;
use uuid::Uuid;

use super::parser::{TodoistRow, TodoistTask};
use super::recurrence::parse_recurrence;

/// Stable namespace UUID for v5 task IDs derived from
/// (project_name + content). Generated once and frozen so
/// re-runs across releases produce the same IDs.
const TODOIST_NAMESPACE: Uuid = Uuid::from_bytes([
    0x6f, 0x9b, 0x9b, 0xa1, 0x6c, 0x10, 0x5f, 0x37, 0xa3, 0x4e, 0x91, 0x3b, 0xc7, 0x76, 0x8e, 0xa1,
]);

#[derive(Debug, Clone, Default)]
pub struct ImportSummary {
    /// `None` on dry-run.
    pub project_id: Option<i64>,
    pub project_title: String,
    pub headings_created: usize,
    pub tasks_created: usize,
    /// Distinct tag names ensured during the run (labels +
    /// priority-N). The worker dedupes; this count is the
    /// label set seen in the source.
    pub tags_created: usize,
    /// Raw `meta` row values (e.g. `view_style=board`). Kept
    /// verbatim so the user sees what didn't translate.
    pub meta_entries: Vec<String>,
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
    UnparseableRecurrence,
    DroppedTimeOfDay,
    DroppedTimezone,
    DroppedDuration,
    DroppedDeadline,
}

#[derive(Debug, thiserror::Error)]
pub enum MapError {
    #[error("worker error: {0}")]
    Worker(#[from] DbError),
}

/// Apply a parsed Todoist row stream to the worker. Creates a
/// fresh project named `project_name`, walks the rows in source
/// order, and produces an [`ImportSummary`].
///
/// `today` anchors any natural-language recurrence phrasings the
/// rows carry (Todoist DATE strings like "Every Sunday").
///
/// `dry_run = true` skips DB writes entirely and just tallies
/// what *would* happen. `project_id` stays `None` in that case.
pub async fn import_todoist(
    handle: &WorkerHandle,
    rows: &[TodoistRow],
    project_name: &str,
    today: NaiveDate,
    dry_run: bool,
) -> Result<ImportSummary, MapError> {
    let mut summary = ImportSummary {
        project_title: project_name.to_string(),
        ..Default::default()
    };

    if dry_run {
        tally_dry_run(rows, today, &mut summary);
        return Ok(summary);
    }

    let project = handle
        .create_project(NewProject {
            title: project_name.to_string(),
            ..Default::default()
        })
        .await?;
    summary.project_id = Some(project.id);

    let mut section_idx: u32 = 0;
    let mut task_idx_in_section: u32 = 0;
    let mut last_indent1_id: Option<i64> = None;
    let mut last_indent2_id: Option<i64> = None;
    let mut tag_names_seen: HashSet<String> = HashSet::new();

    for row in rows {
        match row {
            TodoistRow::Meta { value } => {
                summary.meta_entries.push(value.clone());
            }
            TodoistRow::Section { title, .. } => {
                let _heading = handle.ensure_heading(project.id, title.clone()).await?;
                summary.headings_created += 1;
                section_idx += 1;
                task_idx_in_section = 0;
                last_indent1_id = None;
                last_indent2_id = None;
            }
            TodoistRow::Task(t) => {
                task_idx_in_section += 1;
                let position = compute_task_position(section_idx, task_idx_in_section);

                let parent_id = match t.indent {
                    0 | 1 => None,
                    2 => last_indent1_id,
                    _ => last_indent2_id.or(last_indent1_id),
                };

                let (title, label_tags) = strip_labels(&t.content);
                let new_task = build_new_task(t, &title, project.id, parent_id, project_name);
                let new_task = apply_recurrence(new_task, t, today, &title, &mut summary);
                record_lossy_extras(t, &title, &mut summary);

                let task = handle.create_task(new_task).await?;
                summary.tasks_created += 1;

                // Position update only meaningful for top-level
                // (indent 1) rows where heading interleaving is in
                // play. Subtasks already get sensible per-parent
                // positions from `next_task_position(parent_id, …)`.
                if t.indent <= 1 {
                    handle
                        .update_task(TaskUpdate::new(task.id).position(position))
                        .await?;
                }

                let mut tag_ids: Vec<i64> = Vec::new();
                for label in &label_tags {
                    let tag = handle.ensure_tag(label.clone()).await?;
                    if tag_names_seen.insert(tag.name.clone()) {
                        summary.tags_created += 1;
                    }
                    tag_ids.push(tag.id);
                }
                if let Some(p) = t.priority
                    && p < 4
                {
                    let name = format!("priority-{p}");
                    let tag = handle.ensure_tag(name.clone()).await?;
                    if tag_names_seen.insert(tag.name.clone()) {
                        summary.tags_created += 1;
                    }
                    tag_ids.push(tag.id);
                }
                if !tag_ids.is_empty() {
                    handle.set_task_tags(task.id, tag_ids).await?;
                }

                match t.indent {
                    0 | 1 => {
                        last_indent1_id = Some(task.id);
                        last_indent2_id = None;
                    }
                    2 => {
                        last_indent2_id = Some(task.id);
                    }
                    _ => { /* deeper indents: leave the cursors alone */ }
                }
            }
            TodoistRow::Blank => {
                // Visual separator only — explicit Section rows
                // drive grouping in the Todoist export.
            }
        }
    }

    Ok(summary)
}

fn build_new_task(
    t: &TodoistTask,
    title: &str,
    project_id: i64,
    parent_id: Option<i64>,
    project_name: &str,
) -> NewTask {
    NewTask {
        title: title.to_string(),
        note: t.description.clone(),
        project_id: Some(project_id),
        parent_id,
        uuid: Some(deterministic_task_uuid(project_name, title)),
        ..Default::default()
    }
}

fn apply_recurrence(
    mut task: NewTask,
    t: &TodoistTask,
    today: NaiveDate,
    title: &str,
    summary: &mut ImportSummary,
) -> NewTask {
    let Some(date_str) = t.date.as_deref() else {
        return task;
    };
    match parse_recurrence(date_str, today) {
        Some(parse) => {
            if let Some(rrule) = parse.rrule {
                task.repeat_rule = Some(rrule);
            }
            task.scheduled_for = Some(ScheduledFor::Date(parse.scheduled_date));
            if parse.time.is_some() {
                summary.lossy.push(LossyEntry {
                    kind: LossyKind::DroppedTimeOfDay,
                    task_title: Some(title.to_string()),
                    raw: date_str.to_string(),
                });
            }
        }
        None => summary.lossy.push(LossyEntry {
            kind: LossyKind::UnparseableRecurrence,
            task_title: Some(title.to_string()),
            raw: date_str.to_string(),
        }),
    }
    task
}

fn record_lossy_extras(t: &TodoistTask, title: &str, summary: &mut ImportSummary) {
    if let Some(tz) = &t.timezone {
        summary.lossy.push(LossyEntry {
            kind: LossyKind::DroppedTimezone,
            task_title: Some(title.to_string()),
            raw: tz.clone(),
        });
    }
    if let Some(dur) = &t.duration {
        let unit = t.duration_unit.clone().unwrap_or_default();
        summary.lossy.push(LossyEntry {
            kind: LossyKind::DroppedDuration,
            task_title: Some(title.to_string()),
            raw: format!("{dur} {unit}").trim().to_string(),
        });
    }
    if let Some(dl) = &t.deadline {
        summary.lossy.push(LossyEntry {
            kind: LossyKind::DroppedDeadline,
            task_title: Some(title.to_string()),
            raw: dl.clone(),
        });
    }
}

fn compute_task_position(section_idx: u32, task_idx_in_section: u32) -> f64 {
    f64::from(section_idx) + f64::from(task_idx_in_section) * 0.001
}

/// Pull `@label` tokens out of a Todoist content string and
/// return the cleaned-up title plus the lowercased label list.
/// A label is `@` followed by alphanumeric / `-` / `_`; anything
/// else stays in the title (so `bdkl@example.com` doesn't get
/// stripped). Empty `@` is left in the title.
fn strip_labels(content: &str) -> (String, Vec<String>) {
    let mut tags: Vec<String> = Vec::new();
    let mut kept_words: Vec<&str> = Vec::new();
    for word in content.split_whitespace() {
        if let Some(rest) = word.strip_prefix('@')
            && !rest.is_empty()
            && rest
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            tags.push(rest.to_lowercase());
        } else {
            kept_words.push(word);
        }
    }
    (kept_words.join(" "), tags)
}

fn deterministic_task_uuid(project_name: &str, title: &str) -> String {
    let composite = format!("{project_name}\0{title}");
    Uuid::new_v5(&TODOIST_NAMESPACE, composite.as_bytes()).to_string()
}

fn tally_dry_run(rows: &[TodoistRow], today: NaiveDate, summary: &mut ImportSummary) {
    for row in rows {
        match row {
            TodoistRow::Meta { value } => summary.meta_entries.push(value.clone()),
            TodoistRow::Section { .. } => summary.headings_created += 1,
            TodoistRow::Task(t) => {
                summary.tasks_created += 1;
                if let Some(date_str) = t.date.as_deref()
                    && parse_recurrence(date_str, today).is_none()
                {
                    summary.lossy.push(LossyEntry {
                        kind: LossyKind::UnparseableRecurrence,
                        task_title: Some(t.content.clone()),
                        raw: date_str.to_string(),
                    });
                }
            }
            TodoistRow::Blank => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_labels_removes_recognised_at_tokens() {
        let (title, tags) = strip_labels("Wash darks @chore @home");
        assert_eq!(title, "Wash darks");
        assert_eq!(tags, vec!["chore".to_string(), "home".to_string()]);
    }

    #[test]
    fn strip_labels_leaves_email_in_title() {
        // Email addresses are not labels — they have a `.` in
        // the middle which fails the alphanumeric/_/- check.
        let (title, tags) = strip_labels("Email bdkl@example.com about pickup");
        assert_eq!(title, "Email bdkl@example.com about pickup");
        assert!(tags.is_empty());
    }

    #[test]
    fn strip_labels_preserves_hyphenated_labels() {
        let (title, tags) = strip_labels("Plan @end-of-quarter review");
        assert_eq!(title, "Plan review");
        assert_eq!(tags, vec!["end-of-quarter".to_string()]);
    }

    #[test]
    fn strip_labels_leaves_lone_at_in_title() {
        let (title, tags) = strip_labels("Meet @ HQ");
        assert_eq!(title, "Meet @ HQ");
        assert!(tags.is_empty());
    }

    #[test]
    fn deterministic_uuid_stable_across_runs() {
        let a = deterministic_task_uuid("Weekly chores", "Wash darks");
        let b = deterministic_task_uuid("Weekly chores", "Wash darks");
        assert_eq!(a, b);
        let c = deterministic_task_uuid("Weekly chores", "Different task");
        assert_ne!(a, c);
        let d = deterministic_task_uuid("Other project", "Wash darks");
        assert_ne!(a, d);
    }

    #[test]
    fn compute_task_position_orders_within_section() {
        // Tasks in section 1: 1.001, 1.002 < 2.0 (section 2).
        assert!(compute_task_position(1, 1) < 2.0);
        assert!(compute_task_position(1, 1) < compute_task_position(1, 2));
        assert!(compute_task_position(1, 2) < compute_task_position(2, 1));
    }

    #[test]
    fn dry_run_tallies_without_writes() {
        use super::super::parser::parse_csv;
        let csv = include_str!("../../../tests/fixtures/todoist/home.csv");
        let rows = parse_csv(csv).expect("home.csv parses");
        let today = NaiveDate::from_ymd_opt(2026, 5, 9).unwrap();
        let mut summary = ImportSummary::default();
        tally_dry_run(&rows, today, &mut summary);
        // home.csv has 10 sections + 46 tasks; the acceptance
        // test pins exact numbers — here we just sanity-check.
        assert_eq!(summary.headings_created, 10);
        assert_eq!(summary.tasks_created, 46);
    }

    #[tokio::test]
    async fn import_creates_project_with_section_layout() {
        use atrium_core::spawn_worker;

        let dir = std::env::temp_dir().join(format!("atrium-mapper-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("atrium-test.db");
        let mut writer_conn = rusqlite::Connection::open(&db_path).unwrap();
        atrium_core::db::configure_pragmas(&writer_conn).unwrap();
        atrium_core::db::migrations::migrate(&mut writer_conn).unwrap();
        let (handle, _changes_rx, _library_rx) = spawn_worker(writer_conn);

        // Mini stream: one section, two tasks, blank, second
        // section, one task.
        let rows = vec![
            TodoistRow::Section {
                title: "Alpha".to_string(),
                is_collapsed: false,
            },
            TodoistRow::Task(Box::new(TodoistTask {
                content: "First @t1".to_string(),
                description: String::new(),
                is_collapsed: false,
                priority: Some(4),
                indent: 1,
                author: None,
                responsible: None,
                date: None,
                date_lang: None,
                timezone: None,
                duration: None,
                duration_unit: None,
                deadline: None,
                deadline_lang: None,
            })),
            TodoistRow::Task(Box::new(TodoistTask {
                content: "Second".to_string(),
                description: String::new(),
                is_collapsed: false,
                priority: Some(2),
                indent: 1,
                author: None,
                responsible: None,
                date: None,
                date_lang: None,
                timezone: None,
                duration: None,
                duration_unit: None,
                deadline: None,
                deadline_lang: None,
            })),
            TodoistRow::Blank,
            TodoistRow::Section {
                title: "Beta".to_string(),
                is_collapsed: false,
            },
            TodoistRow::Task(Box::new(TodoistTask {
                content: "Third @t1".to_string(),
                description: String::new(),
                is_collapsed: false,
                priority: Some(4),
                indent: 1,
                author: None,
                responsible: None,
                date: None,
                date_lang: None,
                timezone: None,
                duration: None,
                duration_unit: None,
                deadline: None,
                deadline_lang: None,
            })),
        ];

        let today = NaiveDate::from_ymd_opt(2026, 5, 9).unwrap();
        let summary = import_todoist(&handle, &rows, "Test project", today, false)
            .await
            .unwrap();

        assert!(summary.project_id.is_some());
        assert_eq!(summary.headings_created, 2);
        assert_eq!(summary.tasks_created, 3);
        // priority-2 + t1 = 2 distinct labels; priority-4 doesn't
        // emit a tag.
        assert_eq!(summary.tags_created, 2);

        // Reading back: positions should interleave as
        // heading 1.0, tasks 1.001 / 1.002, heading 2.0, task 2.001.
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let project_id = summary.project_id.unwrap();
        let headings = atrium_core::db::read::list_headings_in_project(&conn, project_id).unwrap();
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].title, "Alpha");
        assert_eq!(headings[0].position, 1.0);
        assert_eq!(headings[1].title, "Beta");
        assert_eq!(headings[1].position, 2.0);

        let tasks = atrium_core::db::read::list_all_in_project(&conn, project_id).unwrap();
        assert_eq!(tasks.len(), 3);
        let positions: Vec<f64> = tasks.iter().map(|t| t.position).collect();
        // Tasks in source order: First (1.001), Second (1.002),
        // Third (2.001). list_all_in_project already orders by
        // position, so we can compare directly.
        assert!((positions[0] - 1.001).abs() < 1e-9);
        assert!((positions[1] - 1.002).abs() < 1e-9);
        assert!((positions[2] - 2.001).abs() < 1e-9);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn import_subtasks_use_parent_id() {
        use atrium_core::spawn_worker;

        let dir =
            std::env::temp_dir().join(format!("atrium-mapper-subtask-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("atrium-test.db");
        let mut writer_conn = rusqlite::Connection::open(&db_path).unwrap();
        atrium_core::db::configure_pragmas(&writer_conn).unwrap();
        atrium_core::db::migrations::migrate(&mut writer_conn).unwrap();
        let (handle, _changes_rx, _library_rx) = spawn_worker(writer_conn);

        let rows = vec![
            TodoistRow::Task(Box::new(TodoistTask {
                content: "Parent".to_string(),
                description: String::new(),
                is_collapsed: false,
                priority: Some(4),
                indent: 1,
                author: None,
                responsible: None,
                date: None,
                date_lang: None,
                timezone: None,
                duration: None,
                duration_unit: None,
                deadline: None,
                deadline_lang: None,
            })),
            TodoistRow::Task(Box::new(TodoistTask {
                content: "Child".to_string(),
                description: String::new(),
                is_collapsed: false,
                priority: Some(4),
                indent: 2,
                author: None,
                responsible: None,
                date: None,
                date_lang: None,
                timezone: None,
                duration: None,
                duration_unit: None,
                deadline: None,
                deadline_lang: None,
            })),
        ];
        let today = NaiveDate::from_ymd_opt(2026, 5, 9).unwrap();
        let summary = import_todoist(&handle, &rows, "Subtask test", today, false)
            .await
            .unwrap();
        assert_eq!(summary.tasks_created, 2);

        let conn = rusqlite::Connection::open(&db_path).unwrap();
        let tasks =
            atrium_core::db::read::list_all_in_project(&conn, summary.project_id.unwrap()).unwrap();
        let parent = tasks.iter().find(|t| t.title == "Parent").unwrap();
        let child = tasks.iter().find(|t| t.title == "Child").unwrap();
        assert_eq!(child.parent_id, Some(parent.id));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
