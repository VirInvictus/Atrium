// SPDX-License-Identifier: MIT
//! Org-mode → Atrium DB importer (Phase 16, v0.7.9).
//!
//! [`import_org_file`] reads a single `.org` file, parses it via
//! [`super::parse::parse_org_file`], and inserts the resulting
//! tasks through the worker. The file is treated as one Project
//! (the Atrium analogue of an Org project file per spec §7.3.1);
//! every TODO-keyworded headline becomes a Task; subtask nesting
//! (deeper headlines) maps onto Atrium's `parent_id` column.
//!
//! # Field mapping (spec §7.3.2)
//!
//! | Org | Atrium |
//! |---|---|
//! | Filename / `#+TITLE:` | Project.title |
//! | TODO keyword | (open task) |
//! | DONE / CANCELLED | toggle_complete after create — completed_at = now |
//! | Custom keyword | open task; original noted in lossy |
//! | Headline | Task.title |
//! | Headline `:tags:` | Atrium tags via ensure_tag |
//! | Body | Task.note |
//! | SCHEDULED | Task.scheduled_for |
//! | DEADLINE | Task.deadline |
//! | CLOSED | (deferred: completed_at preservation lands in v0.7.10) |
//! | `:ID:` | Task.uuid |
//! | `:RRULE:` | Task.repeat_rule (verbatim) |
//! | `:EFFORT:` | Task.estimated_minutes (M:SS or "Mm") |
//! | `:DEFER_UNTIL:` | Task.defer_until |
//!
//! # Limitations
//!
//! - Single-file import only. Multi-file vault walk lands in a
//!   later patch.
//! - Project sub-headings (headlines without a TODO keyword)
//!   are skipped with a count in `ImportSummary::headings_skipped`.
//!   The `heading` table writer follows in v0.7.10+.
//! - DONE / CANCELLED tasks have `completed_at = now()`, not the
//!   CLOSED cookie's timestamp. v0.7.10 will add a worker path
//!   for caller-provided completed_at.
//! - Repeater suffixes on SCHEDULED / DEADLINE are recorded but
//!   not converted to RFC 5545 RRULE. Use `:RRULE:` for canonical
//!   round-trips.
//! - `:CREATED:` / `:MODIFIED:` properties don't override
//!   the schema-auto-set timestamps.
//!
//! Re-imports always create new rows. The full bidirectional
//! sync (Phase 17) handles upsert-by-`:ID:`.

use std::path::Path;

use crate::WorkerHandle;
use crate::domain::{NewProject, NewTask, ScheduledFor};
use crate::error::DbError;
use crate::sync::org::parse::{OrgKeyword, OrgTask, parse_org_file};

/// Result of an import. Counts + lossy notes the caller surfaces
/// to the user via the CLI / GUI.
#[derive(Debug, Clone, Default)]
pub struct ImportSummary {
    pub project_title: Option<String>,
    pub project_id: Option<i64>,
    pub tasks_created: usize,
    pub tags_ensured: usize,
    pub headings_skipped: usize,
    pub lossy: Vec<String>,
}

/// Errors specific to the import flow. Wraps the underlying
/// io / DB failures with file context.
#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("read {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("worker error: {0}")]
    Db(#[from] DbError),
}

/// Import a single `.org` file into the connected database.
///
/// On success, creates one project (named after the file stem)
/// and inserts every TODO-keyworded headline as a task. Returns
/// an [`ImportSummary`] with counts + a lossy-fields list.
///
/// `dry_run = true` walks the parse tree and tallies what *would*
/// be created without touching the DB.
pub async fn import_org_file(
    handle: &WorkerHandle,
    path: &Path,
    dry_run: bool,
) -> Result<ImportSummary, ImportError> {
    let path_display = path.display().to_string();
    let tasks = parse_org_file(path).map_err(|source| ImportError::Io {
        path: path_display.clone(),
        source,
    })?;

    let project_title = path
        .file_stem()
        .and_then(|os| os.to_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "Imported".to_string());

    let mut summary = ImportSummary {
        project_title: Some(project_title.clone()),
        ..Default::default()
    };

    // Dry-run: count what would land. We still walk the full
    // tree so the user sees an accurate picture.
    if dry_run {
        for task in &tasks {
            tally_dry_run(task, &mut summary);
        }
        return Ok(summary);
    }

    // Real import.
    let project = handle
        .create_project(NewProject {
            title: project_title.clone(),
            uuid: None,
            ..Default::default()
        })
        .await?;
    summary.project_id = Some(project.id);

    for task in &tasks {
        import_task(handle, task, project.id, None, &mut summary).await?;
    }

    Ok(summary)
}

/// Recursive walker for dry-run mode. Counts headings vs tasks
/// without dispatching any worker calls.
fn tally_dry_run(task: &OrgTask, summary: &mut ImportSummary) {
    if task.keyword.is_none() {
        summary.headings_skipped += 1;
    } else {
        summary.tasks_created += 1;
        summary.tags_ensured += task.tags.len();
    }
    for child in &task.children {
        tally_dry_run(child, summary);
    }
}

/// Convert one OrgTask into a worker insert + recurse into
/// children with the resulting task id as parent_id. Async-
/// recursive via Box::pin since direct recursion across
/// `async fn` is forbidden.
fn import_task<'a>(
    handle: &'a WorkerHandle,
    org: &'a OrgTask,
    project_id: i64,
    parent_id: Option<i64>,
    summary: &'a mut ImportSummary,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ImportError>> + 'a>> {
    Box::pin(async move {
        // Headlines without a TODO keyword are project sub-headings.
        // v0.7.9 skips them — heading-table writes follow.
        let Some(keyword) = &org.keyword else {
            summary.headings_skipped += 1;
            // Children of a sub-heading still flow into the project
            // at the same parent level (as if the sub-heading were
            // transparent), per the spec's "project sub-headings
            // are organisational, not structural" treatment.
            for child in &org.children {
                import_task(handle, child, project_id, parent_id, summary).await?;
            }
            return Ok(());
        };

        // Custom keywords are stashed on the task's orig_keyword
        // column (v0.7.12) so the writer can round-trip them
        // unchanged. The keyword itself sits at TODO sentinel
        // because Atrium's domain model only knows three
        // canonical states; the orig_keyword column carries the
        // original text. When v0.7.13's writer revision lands,
        // emitting this task to Org will use orig_keyword as the
        // headline keyword.
        let is_done = matches!(keyword, OrgKeyword::Done | OrgKeyword::Cancelled);
        let orig_keyword = if let OrgKeyword::Custom(name) = keyword {
            Some(name.clone())
        } else {
            None
        };

        // Property-derived fields. We pull each defensively so
        // a malformed value falls back to None + a lossy note.
        let estimated_minutes = org
            .properties
            .get("EFFORT")
            .and_then(|v| parse_effort(v))
            .or_else(|| {
                if org.properties.contains_key("EFFORT") {
                    summary.lossy.push(format!(
                        "task “{}”: :EFFORT: value not parseable; field left unset",
                        org.title
                    ));
                }
                None
            });
        let defer_until = org
            .properties
            .get("DEFER_UNTIL")
            .and_then(|v| chrono::NaiveDate::parse_from_str(v, "%Y-%m-%d").ok())
            .or_else(|| {
                if org.properties.contains_key("DEFER_UNTIL") {
                    summary.lossy.push(format!(
                        "task “{}”: :DEFER_UNTIL: value not a YYYY-MM-DD date; field left unset",
                        org.title
                    ));
                }
                None
            });
        let repeat_rule = org.properties.get("RRULE").cloned();

        let scheduled_for = org.scheduled.map(ScheduledFor::Date);
        let id_property = org.properties.get("ID").cloned();

        if let Some(closed_at) = org.closed
            && is_done
        {
            summary.lossy.push(format!(
                "task “{}”: CLOSED timestamp ({}) not preserved — completed_at will be set to now() on import",
                org.title, closed_at
            ));
        }

        let new = NewTask {
            title: org.title.clone(),
            note: org.body.clone(),
            project_id: Some(project_id),
            parent_id,
            scheduled_for,
            deadline: org.deadline,
            defer_until,
            estimated_minutes,
            repeat_rule,
            repeat_mode: None,
            uuid: id_property,
            orig_keyword,
        };
        let created = handle.create_task(new).await?;
        summary.tasks_created += 1;

        // Tag attach — ensure each by name (idempotent), then
        // overwrite the row's tag set.
        if !org.tags.is_empty() {
            let mut tag_ids = Vec::with_capacity(org.tags.len());
            for name in &org.tags {
                let tag = handle.ensure_tag(name.clone()).await?;
                tag_ids.push(tag.id);
                summary.tags_ensured += 1;
            }
            handle.set_task_tags(created.id, tag_ids).await?;
        }

        // DONE / CANCELLED tasks get toggled to completed
        // afterwards. v0.7.10 will add a worker path that lets
        // the caller pass completed_at directly so the original
        // CLOSED timestamp survives.
        if is_done {
            handle.toggle_complete(created.id).await?;
        }

        for child in &org.children {
            import_task(handle, child, project_id, Some(created.id), summary).await?;
        }

        Ok(())
    })
}

/// Parse Org's `:EFFORT:` value into integer minutes. Supports
/// `H:MM` (`"1:30"` → 90) and the abbreviated forms `"30m"`,
/// `"1h"`, `"1h30m"`. Returns `None` for unparseable input.
fn parse_effort(value: &str) -> Option<i64> {
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
        assert_eq!(parse_effort("foo"), None);
        assert_eq!(parse_effort("1:60"), None); // minutes out of range
        assert_eq!(parse_effort("not:numeric"), None);
        assert_eq!(parse_effort("1x"), None);
    }

    #[test]
    fn dry_run_tally_counts_tasks_and_headings() {
        let input = "\
* Project sub-heading
** TODO Real task :work:
** DONE Done task
* TODO Top-level task
* Another sub-heading
** TODO Nested task
";
        let tasks = crate::sync::org::parse::parse_org_text(input);
        let mut summary = ImportSummary::default();
        for task in &tasks {
            tally_dry_run(task, &mut summary);
        }
        assert_eq!(summary.tasks_created, 4);
        assert_eq!(summary.headings_skipped, 2);
        assert_eq!(summary.tags_ensured, 1); // :work: on Real task
    }

    // End-to-end import tests live alongside the worker tests in
    // db/worker.rs because they need a spawned runtime; this
    // module covers the synchronous helpers (dry-run tally,
    // effort parser) only.
}
