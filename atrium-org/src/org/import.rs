// SPDX-License-Identifier: MIT
//! Org-mode → Atrium DB importer (Phase 16, v0.7.9).
//!
//! [`import_org_file`] reads a single `.org` file, parses it via
//! [`super::parse::parse_org_file`], and inserts the resulting
//! tasks through the worker. The file is treated as one Project
//! (the Atrium analogue of an Org project file per spec §7.3.1);
//! every TODO-keyworded headline becomes a Task; subtask nesting
//! (deeper headlines) maps onto Atrium's `parent_id` column.
//! [`import_org_directory`] walks a vault root and routes each
//! `.org` file through the single-file path, mapping subdirectories
//! onto Atrium areas via `WorkerHandle::ensure_area`.
//!
//! # Field mapping (spec §7.3.2)
//!
//! | Org | Atrium |
//! |---|---|
//! | Filename / `#+TITLE:` | Project.title |
//! | TODO keyword | (open task) |
//! | DONE / CANCELLED | `completed_at` from CLOSED cookie when present, else `now()` |
//! | Custom keyword | open task; original stashed in `task.orig_keyword` |
//! | Headline | Task.title |
//! | Headline `:tags:` | Atrium tags via ensure_tag |
//! | Body | Task.note |
//! | SCHEDULED | Task.scheduled_for |
//! | DEADLINE | Task.deadline |
//! | CLOSED | Task.completed_at (threaded through `NewTask.completed_at`) |
//! | `:ID:` | Task.uuid |
//! | `:RRULE:` | Task.repeat_rule (verbatim) |
//! | `:EFFORT:` | Task.estimated_minutes (`H:MM` or `Mm` / `Hh` / `HhMm`) |
//! | `:DEFER_UNTIL:` | Task.defer_until |
//!
//! # Known limits
//!
//! - Project sub-headings (headlines without a TODO keyword)
//!   pass through transparently — children import at the parent
//!   level and the heading itself is counted in
//!   `ImportSummary::headings_skipped`. Writing them back through
//!   the `heading` table is roadmap.md §17 follow-up work.
//! - Repeater suffixes on SCHEDULED / DEADLINE round-trip via
//!   `OrgRepeater` but are not converted to RFC 5545 RRULE. Set
//!   `:RRULE:` in the source file for canonical round-trips;
//!   spec §7.3.3 rule 3 makes `:RRULE:` canonical.
//! - `:CREATED:` / `:MODIFIED:` properties don't override the
//!   schema-auto-set timestamps. The `WHEN old=new` triggers in
//!   `0001_initial.sql` would honour an explicit write, but the
//!   importer doesn't supply one.
//!
//! Re-imports always create new rows. Upsert-by-`:ID:` is the
//! [`crate::vault_watcher`] path (Phase 17), not the importer.

use std::path::Path;

use crate::org::parse::{OrgKeyword, OrgTask, parse_org_file_with_meta};
use atrium_core::WorkerHandle;
use atrium_core::domain::{NewProject, NewTask, ScheduledFor};
use atrium_core::error::DbError;

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
/// Convenience wrapper for the common single-file path. The
/// resulting project has no `area_id` (Atrium calls this
/// "unfiled"). Use [`import_org_file_with_area`] when the file
/// belongs to a vault subdirectory that should map onto an
/// Atrium area.
///
/// On success, creates one project (named after the file stem
/// or `#+TITLE:` if present) and inserts every TODO-keyworded
/// headline as a task. Returns an [`ImportSummary`] with counts
/// and a lossy-fields list.
///
/// `dry_run = true` walks the parse tree and tallies what *would*
/// be created without touching the DB.
pub async fn import_org_file(
    handle: &WorkerHandle,
    path: &Path,
    dry_run: bool,
) -> Result<ImportSummary, ImportError> {
    import_org_file_with_area(handle, path, None, dry_run).await
}

/// Full single-file importer that accepts an optional
/// `area_id` to file the resulting project under. Used by
/// [`import_org_directory`] to map vault subdirectories onto
/// Atrium areas.
pub async fn import_org_file_with_area(
    handle: &WorkerHandle,
    path: &Path,
    area_id: Option<i64>,
    dry_run: bool,
) -> Result<ImportSummary, ImportError> {
    let path_display = path.display().to_string();
    let file = parse_org_file_with_meta(path).map_err(|source| ImportError::Io {
        path: path_display.clone(),
        source,
    })?;
    let tasks = file.headlines;

    // file-level metadata threading. #+TITLE: wins over
    // the file stem when present (matches Org's own convention).
    // The file-level :PROPERTIES: drawer carries project-level
    // fields (:SEQUENTIAL: / :REVIEW_INTERVAL: / :LAST_REVIEWED:
    // / :ARCHIVED: / :ID:) per spec §7.3.2.
    let project_title = file
        .directives
        .get("TITLE")
        .cloned()
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|os| os.to_str())
                .map_or_else(|| "Imported".to_string(), std::string::ToString::to_string)
        });

    let project_uuid = file.file_properties.get("ID").cloned();
    let project_sequential = file
        .file_properties
        .get("SEQUENTIAL")
        .is_some_and(|v| matches!(v.as_str(), "t" | "T" | "true" | "TRUE" | "1"));
    let project_review_interval = file
        .file_properties
        .get("REVIEW_INTERVAL")
        .and_then(|v| v.parse::<i64>().ok());
    let project_last_reviewed = file
        .file_properties
        .get("LAST_REVIEWED")
        .and_then(|v| parse_inactive_or_active_datetime(v));
    let project_archived = file
        .file_properties
        .get("ARCHIVED")
        .and_then(|v| parse_inactive_or_active_datetime(v));

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
            uuid: project_uuid,
            sequential: project_sequential,
            review_interval_days: project_review_interval,
            last_reviewed_at: project_last_reviewed,
            archived_at: project_archived,
            // caller's area_id wins. None for the
            // single-file path; Some for the directory walker.
            area_id,
            ..Default::default()
        })
        .await?;
    summary.project_id = Some(project.id);

    for task in &tasks {
        import_task(handle, task, project.id, None, &mut summary).await?;
    }

    Ok(summary)
}

/// Multi-file vault import. Walks `vault_root` for `.org`
/// files and routes each through `import_org_file_with_area`:
///
/// - Files at `<vault_root>/<project>.org` → unfiled Project.
/// - Files at `<vault_root>/<area>/<project>.org` → Project filed
///   under Area `<area>` (created via `ensure_area` if absent;
///   case-insensitive match against existing areas).
///
/// Skips dot-prefixed entries (`.atrium/`, `.git/`, hidden temp
/// files) for safety. Skips non-`.org` files silently. Sub-
/// directories nested deeper than one level (an "area's area")
/// are flagged and skipped — spec §7.3.1 has exactly one level
/// of areas.
///
/// Returns one [`ImportSummary`] per imported file. On dry-run,
/// no DB changes are made and each summary's `project_id` is
/// `None`.
pub async fn import_org_directory(
    handle: &WorkerHandle,
    vault_root: &Path,
    dry_run: bool,
) -> Result<Vec<ImportSummary>, ImportError> {
    let mut summaries: Vec<ImportSummary> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Top-level pass: every entry of vault_root.
    let entries = std::fs::read_dir(vault_root).map_err(|source| ImportError::Io {
        path: vault_root.display().to_string(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| ImportError::Io {
            path: vault_root.display().to_string(),
            source,
        })?;
        let entry_path = entry.path();
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }
        let metadata = entry.metadata().map_err(|source| ImportError::Io {
            path: entry_path.display().to_string(),
            source,
        })?;
        if metadata.is_file() {
            if entry_path.extension().and_then(|e| e.to_str()) != Some("org") {
                continue;
            }
            // Top-level file → unfiled project.
            let mut summary = import_org_file_with_area(handle, &entry_path, None, dry_run).await?;
            // Surface accumulated warnings on the first summary
            // so the caller can pass them through. Subsequent
            // summaries leave them empty (avoids n-way duplication).
            if !warnings.is_empty() {
                summary.lossy.append(&mut warnings);
            }
            summaries.push(summary);
        } else if metadata.is_dir() {
            // Subdirectory → Area name. Walk one level deeper.
            let area_name = entry.file_name().to_string_lossy().to_string();
            let area_id = if dry_run {
                None
            } else {
                let area = handle.ensure_area(area_name.clone()).await?;
                Some(area.id)
            };

            let inner = std::fs::read_dir(&entry_path).map_err(|source| ImportError::Io {
                path: entry_path.display().to_string(),
                source,
            })?;
            for inner_entry in inner {
                let inner_entry = inner_entry.map_err(|source| ImportError::Io {
                    path: entry_path.display().to_string(),
                    source,
                })?;
                let inner_path = inner_entry.path();
                let inner_name = inner_entry.file_name();
                let inner_name_str = inner_name.to_string_lossy();
                if inner_name_str.starts_with('.') {
                    continue;
                }
                let inner_md = inner_entry.metadata().map_err(|source| ImportError::Io {
                    path: inner_path.display().to_string(),
                    source,
                })?;
                if inner_md.is_dir() {
                    warnings.push(format!(
                        "skipped sub-area directory {} (spec §7.3.1 has only one level of areas)",
                        inner_path.display()
                    ));
                    continue;
                }
                if !inner_md.is_file() {
                    continue;
                }
                if inner_path.extension().and_then(|e| e.to_str()) != Some("org") {
                    continue;
                }
                let mut summary =
                    import_org_file_with_area(handle, &inner_path, area_id, dry_run).await?;
                if !warnings.is_empty() {
                    summary.lossy.append(&mut warnings);
                }
                summaries.push(summary);
            }
        }
    }

    // Stragglers in `warnings` (if no summary was emitted to
    // attach them to — e.g. a vault where the only finds were
    // sub-area dirs we skipped) get hung off a synthetic
    // summary so the caller still sees them.
    if !warnings.is_empty() {
        summaries.push(ImportSummary {
            lossy: warnings,
            ..Default::default()
        });
    }

    Ok(summaries)
}

/// Permissive Org timestamp parser. Accepts `[YYYY-MM-DD ...]`
/// (inactive — what `:LAST_REVIEWED:` and `:ARCHIVED:` use) and
/// `<YYYY-MM-DD ...>` (active — rare for these properties but
/// not invalid). Falls back to plain `YYYY-MM-DD`. Drops
/// time-of-day if absent (defaults to noon UTC).
fn parse_inactive_or_active_datetime(text: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let trimmed = text.trim();
    let inner = if let Some(s) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        s
    } else if let Some(s) = trimmed.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
        s
    } else {
        trimmed
    };
    let mut parts = inner.split_whitespace();
    let date_part = parts.next()?;
    let date = chrono::NaiveDate::parse_from_str(date_part, "%Y-%m-%d").ok()?;
    let time = parts.find_map(|p| {
        let mut split = p.split(':');
        let h: u32 = split.next()?.parse().ok()?;
        let m: u32 = split.next()?.parse().ok()?;
        if split.next().is_some() {
            return None;
        }
        chrono::NaiveTime::from_hms_opt(h, m, 0)
    });
    let dt = match time {
        Some(t) => date.and_time(t),
        None => date.and_hms_opt(12, 0, 0)?,
    };
    Some(dt.and_utc())
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
        // unchanged. The keyword itself sits at TODO/DONE
        // sentinel because Atrium's domain model only knows two
        // canonical states; the orig_keyword column carries the
        // original text.
        //
        // CANCELLED is also stashed since Atrium's
        // completed_at column doesn't distinguish "done" from
        // "cancelled"; without orig_keyword preservation a
        // CANCELLED task round-trips back as DONE. The writer
        // checks orig_keyword first when picking the headline
        // keyword, so this lands cleanly.
        let is_done = matches!(keyword, OrgKeyword::Done | OrgKeyword::Cancelled);
        let orig_keyword = match keyword {
            OrgKeyword::Custom(name) => Some(name.clone()),
            OrgKeyword::Cancelled => Some("CANCELLED".to_string()),
            _ => None,
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

        // DONE / CANCELLED tasks now thread the source
        // CLOSED cookie through to NewTask.completed_at so the
        // round-trip preserves the exact completion timestamp.
        // The earlier toggle_complete-after-create path stamped
        // `now()`, which broke the round-trip on completed
        // tasks; now the worker inserts with completed_at set
        // directly when the source vault file specifies a CLOSED
        // cookie. No lossy note needed for the common case.
        let completed_at_for_insert = if is_done { org.closed } else { None };

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
            completed_at: completed_at_for_insert,
            // v0.14.0 — round-trip the DEADLINE warning suffix
            // (`-Nd` / `--Nd`) into the per-task override column.
            // Both prefix shapes parse to the same `u32` days; the
            // emitter normalises onto `-`.
            deadline_warn_days: org.deadline_warning.map(i64::from),
            // v0.19.0 — Phase 18.5 Tier-2 time-of-day on
            // schedule. Parser captures the time portion of the
            // SCHEDULED active timestamp into `org.scheduled_time`;
            // thread it into the new task's column.
            scheduled_time: org.scheduled_time,
            // v0.20.0 — Phase 19.5 reminders. Org-mode has no
            // standard reminder cookie; importer leaves this
            // None and users set reminders in Atrium.
            reminder_at: None,
            // v0.24.0 — Post-v0.22.0 Tier 1 custom-property
            // drawer passthrough. Stash every drawer key
            // outside the modeled set so spec §7.3.3 rule 1
            // holds for property drawers.
            extra_properties: super::extras_from_properties(&org.properties),
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

        // DONE / CANCELLED tasks where the source had no CLOSED
        // cookie still need to be marked complete. NewTask
        // already inserted with completed_at set when the source
        // had a CLOSED cookie (v0.7.17 path); only toggle when
        // we need the worker to fill in `now()`.
        if is_done && completed_at_for_insert.is_none() {
            handle.toggle_complete(created.id).await?;
        }

        // v0.17.0 — Phase 18.5 Tier-1 CLOCK time tracking. Thread
        // every parsed :LOGBOOK: entry into task_clock_entry via
        // the worker's import path (caller-provided timestamps,
        // skips the single-active-clock invariant since the source
        // file is trusted).
        for entry in &org.clock_entries {
            handle
                .import_clock_entry(created.id, entry.started, entry.ended, entry.note.clone())
                .await?;
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
        let tasks = crate::org::parse::parse_org_text(input);
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
