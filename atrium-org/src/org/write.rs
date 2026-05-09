// SPDX-License-Identifier: MIT
//! Atrium DB → Org vault writer (Phase 16, v0.7.10).
//!
//! Inverse of [`super::import`]. Reads a project + its tasks +
//! tags through the read connection, builds an [`OrgTask`] tree
//! reflecting Atrium's domain, and writes the resulting Org text
//! atomically via [`super::emit::emit_org_file`].
//!
//! # Vault layout (spec §7.3.1)
//!
//! ```text
//! <vault_root>/
//! ├── inbox.org                ← unfiled projects (one file each)
//! ├── Personal/
//! │   ├── Errands.org
//! │   └── Reading.org
//! └── Work/
//!     ├── Q3.org
//!     └── Onboarding.org
//! ```
//!
//! For unfiled projects (no `area_id`) the file lands in the
//! vault root. For filed projects the file lands under
//! `<vault_root>/<area_title>/<project_title>.org`.
//!
//! # Field mapping (reverses spec §7.3.2)
//!
//! | Atrium Task | Org |
//! |---|---|
//! | `title` | headline text |
//! | `note` | body verbatim |
//! | `tags` | headline `:tag1:tag2:` |
//! | `completed_at` (Some) | DONE keyword + `CLOSED:` cookie |
//! | `completed_at` (None) | TODO keyword |
//! | `scheduled_for` | SCHEDULED cookie |
//! | `deadline` | DEADLINE cookie |
//! | `uuid` | `:ID:` property |
//! | `repeat_rule` | `:RRULE:` property |
//! | `estimated_minutes` | `:EFFORT:` property in `H:MM` |
//! | `defer_until` | `:DEFER_UNTIL:` property in `YYYY-MM-DD` |
//! | `parent_id` chain | nested headlines |
//!
//! # Known limits
//!
//! - Project sub-headings (the `heading` table) aren't emitted as
//!   non-TODO Org headlines on writeback. The importer counts
//!   them in `ImportSummary::headings_skipped`; round-tripping
//!   them through the `heading` table is roadmap.md §17 follow-up
//!   work. The structural impact is limited because the writer
//!   re-emits each project's tasks in `position` order — the file
//!   stays readable, just flatter than the source.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, Timelike, Utc};
use rusqlite::Connection;

use super::emit::emit_org_file_with_meta;
use super::parse::{OrgFile, OrgKeyword, OrgRepeater, OrgTask};
use atrium_core::domain::{Project, ScheduledFor, Task};
use atrium_core::error::DbError;

/// Result of writing one project to the vault.
#[derive(Debug, Clone)]
pub struct WriteSummary {
    pub project_id: i64,
    pub project_title: String,
    pub task_count: usize,
    pub file_path: PathBuf,
}

/// Errors specific to the vault-write flow.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error("project {0} not found")]
    ProjectNotFound(i64),
    #[error("io error writing {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("DB error: {0}")]
    Db(#[from] DbError),
}

/// Write one project's `.org` file under `vault_root`.
///
/// Reads the project metadata, every task in it (open + done),
/// and tag names per task through `conn`. Builds the OrgTask
/// tree, resolves the destination path, creates the parent
/// directory if absent, and emits via [`emit_org_file`] (atomic
/// write).
pub fn write_project_to_vault(
    conn: &Connection,
    vault_root: &Path,
    project_id: i64,
) -> Result<WriteSummary, WriteError> {
    let project = atrium_core::db::read::project_by_id(conn, project_id)?
        .ok_or(WriteError::ProjectNotFound(project_id))?;
    let area_title = match project.area_id {
        Some(aid) => atrium_core::db::read::area_by_id(conn, aid)?.map(|a| a.title),
        None => None,
    };
    let tasks = atrium_core::db::read::list_all_in_project(conn, project_id)?;
    let tag_names = atrium_core::db::read::tag_names_per_task(conn)?;

    let tree = build_org_tree(&tasks, &tag_names);
    // file-level preamble carries the project title +
    // project metadata so the importer can round-trip them
    // cleanly. The OrgFile struct bundles directives +
    // file_properties + headlines.
    let file = OrgFile {
        directives: build_file_directives(&project),
        file_properties: build_file_properties(&project),
        headlines: tree,
    };

    let path = build_project_vault_path(vault_root, area_title.as_deref(), &project.title);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| WriteError::Io {
            path: parent.display().to_string(),
            source: e,
        })?;
    }

    emit_org_file_with_meta(&path, &file).map_err(|e| WriteError::Io {
        path: path.display().to_string(),
        source: e,
    })?;

    Ok(WriteSummary {
        project_id,
        project_title: project.title,
        task_count: tasks.len(),
        file_path: path,
    })
}

/// Compute the destination path a project would land at without
/// performing the write. Used by the conflict-detection path in
/// [`crate::vault_writer::VaultWriter`] so it can stat the existing
/// file and back up external edits before the atomic-overwrite
/// runs. Cheap — one project + one area read.
pub fn project_vault_path(
    conn: &Connection,
    vault_root: &Path,
    project_id: i64,
) -> Result<PathBuf, WriteError> {
    let project = atrium_core::db::read::project_by_id(conn, project_id)?
        .ok_or(WriteError::ProjectNotFound(project_id))?;
    let area_title = match project.area_id {
        Some(aid) => atrium_core::db::read::area_by_id(conn, aid)?.map(|a| a.title),
        None => None,
    };
    Ok(build_project_vault_path(
        vault_root,
        area_title.as_deref(),
        &project.title,
    ))
}

fn build_project_vault_path(
    vault_root: &Path,
    area_title: Option<&str>,
    project_title: &str,
) -> PathBuf {
    let mut path = vault_root.to_path_buf();
    if let Some(area) = area_title {
        path.push(sanitize_filename(area));
    }
    path.push(format!("{}.org", sanitize_filename(project_title)));
    path
}

/// Write every project in the DB to `vault_root`. Used by the
/// `atrium-cli export org PATH` subcommand. Returns one
/// [`WriteSummary`] per project; failures abort the run with
/// the first error so the user sees a clear cause.
pub fn write_all_projects_to_vault(
    conn: &Connection,
    vault_root: &Path,
) -> Result<Vec<WriteSummary>, WriteError> {
    let projects = atrium_core::db::read::list_projects(conn)?;
    let mut out = Vec::with_capacity(projects.len());
    for project in projects {
        let summary = write_project_to_vault(conn, vault_root, project.id)?;
        out.push(summary);
    }
    Ok(out)
}

/// Convert a flat list of Tasks (in position order) into a
/// nested OrgTask tree by walking parent_id chains.
///
/// Tasks with a `parent_id` matching another task in the same
/// list become children of that task; the rest are top-level.
/// Children inherit `depth = parent.depth + 1`. Tasks whose
/// parent_id points to a task in a different project (shouldn't
/// happen, but defensive) fall back to top-level.
fn build_org_tree(tasks: &[Task], tag_names: &HashMap<i64, Vec<String>>) -> Vec<OrgTask> {
    // Index tasks by id so children can find their parents.
    let by_id: HashMap<i64, &Task> = tasks.iter().map(|t| (t.id, t)).collect();

    fn depth_for(by_id: &HashMap<i64, &Task>, task: &Task) -> usize {
        let mut depth = 1;
        let mut cursor = task.parent_id;
        while let Some(pid) = cursor {
            if let Some(parent) = by_id.get(&pid) {
                depth += 1;
                cursor = parent.parent_id;
            } else {
                break;
            }
        }
        depth
    }

    // First pass: build OrgTasks for every Task (in input order),
    // computing depth from the parent_id chain.
    let mut org_tasks: Vec<OrgTask> = tasks
        .iter()
        .map(|t| {
            let depth = depth_for(&by_id, t);
            task_to_org(t, depth, tag_names)
        })
        .collect();

    // Second pass: build the tree recursively by collecting
    // children as we go. For each top-level task (parent_id None
    // OR parent not in the map), recursively pull its descendants.
    // The `consumed` bit-vector marks tasks already attached so
    // sibling sweeps don't re-claim them.
    let mut top: Vec<OrgTask> = Vec::new();
    let mut consumed: Vec<bool> = vec![false; tasks.len()];

    fn pull_subtree(
        idx: usize,
        tasks: &[Task],
        org_tasks: &[OrgTask],
        consumed: &mut [bool],
    ) -> OrgTask {
        let mut node = org_tasks[idx].clone();
        consumed[idx] = true;
        let id = tasks[idx].id;
        for (j, t) in tasks.iter().enumerate() {
            if consumed[j] {
                continue;
            }
            if t.parent_id == Some(id) {
                let child = pull_subtree(j, tasks, org_tasks, consumed);
                node.children.push(child);
            }
        }
        node
    }

    for (i, task) in tasks.iter().enumerate() {
        if consumed[i] {
            continue;
        }
        let is_top = match task.parent_id {
            None => true,
            Some(pid) => !by_id.contains_key(&pid),
        };
        if is_top {
            let node = pull_subtree(i, tasks, &org_tasks, &mut consumed);
            top.push(node);
        }
    }

    // Any unconsumed tasks (e.g. orphaned subtasks whose parent
    // pointed to a task NOT in this project) get appended at top
    // level so we don't silently drop them.
    for (i, _) in tasks.iter().enumerate() {
        if !consumed[i] {
            top.push(org_tasks.swap_remove(i));
        }
    }

    top
}

/// file-level directives for a project's `.org` file.
/// Currently emits `#+TITLE:`. Other directives (`#+CATEGORY:`,
/// `#+FILETAGS:`, `#+STARTUP:` …) follow when Atrium grows
/// project-level analogues; v0.7.13 starts with the one
/// directive every Org tool reads.
fn build_file_directives(project: &Project) -> HashMap<String, String> {
    let mut out = HashMap::new();
    out.insert("TITLE".to_string(), project.title.clone());
    out
}

/// file-level :PROPERTIES: drawer for a project. The
/// keys mirror spec §7.3.2's project mapping:
///
/// | Atrium field | Org property |
/// |---|---|
/// | `Project.uuid` | `:ID:` |
/// | `Project.sequential` (true) | `:SEQUENTIAL: t` |
/// | `Project.review_interval_days` (Some) | `:REVIEW_INTERVAL:` |
/// | `Project.last_reviewed_at` (Some) | `:LAST_REVIEWED:` (inactive timestamp) |
/// | `Project.archived_at` (Some) | `:ARCHIVED:` (inactive timestamp) |
///
/// `Project.note` is currently dropped on write (no Org-side
/// home for project-level free-text yet); a future patch can
/// surface it as a body block above the first headline.
fn build_file_properties(project: &Project) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if !project.uuid.is_empty() {
        out.insert("ID".to_string(), project.uuid.clone());
    }
    if project.sequential {
        out.insert("SEQUENTIAL".to_string(), "t".to_string());
    }
    if let Some(days) = project.review_interval_days {
        out.insert("REVIEW_INTERVAL".to_string(), days.to_string());
    }
    if let Some(when) = project.last_reviewed_at {
        out.insert("LAST_REVIEWED".to_string(), inactive_timestamp(when));
    }
    if let Some(when) = project.archived_at {
        out.insert("ARCHIVED".to_string(), inactive_timestamp(when));
    }
    out
}

fn inactive_timestamp(when: chrono::DateTime<chrono::Utc>) -> String {
    let date = when.date_naive();
    let day = date.format("%a");
    let time = when.time();
    if time.hour() == 12 && time.minute() == 0 && time.second() == 0 {
        format!("[{} {}]", date.format("%Y-%m-%d"), day)
    } else {
        format!(
            "[{} {} {}]",
            date.format("%Y-%m-%d"),
            day,
            time.format("%H:%M")
        )
    }
}

fn task_to_org(task: &Task, depth: usize, tag_names: &HashMap<i64, Vec<String>>) -> OrgTask {
    // when the importer stashed a non-canonical Org
    // keyword (WAITING, BLOCKED, IN-PROGRESS, etc.) we restore
    // it on emit. The completed_at column still drives whether
    // the task counts as done in Atrium; the Org keyword is
    // purely a label round-trip.
    let keyword = if let Some(orig) = &task.orig_keyword {
        Some(OrgKeyword::Custom(orig.clone()))
    } else if task.completed_at.is_some() {
        Some(OrgKeyword::Done)
    } else {
        Some(OrgKeyword::Todo)
    };

    let scheduled = match task.scheduled_for {
        Some(ScheduledFor::Date(d)) => Some(d),
        _ => None,
    };

    let mut properties: HashMap<String, String> = HashMap::new();
    if !task.uuid.is_empty() {
        properties.insert("ID".into(), task.uuid.clone());
    }
    if let Some(rule) = &task.repeat_rule {
        properties.insert("RRULE".into(), rule.clone());
    }
    if let Some(minutes) = task.estimated_minutes {
        properties.insert("EFFORT".into(), format_effort(minutes));
    }
    if let Some(defer) = task.defer_until {
        properties.insert("DEFER_UNTIL".into(), defer.format("%Y-%m-%d").to_string());
    }

    let tags = tag_names.get(&task.id).cloned().unwrap_or_default();

    OrgTask {
        depth,
        keyword,
        title: task.title.clone(),
        tags,
        scheduled,
        scheduled_repeater: scheduled_repeater_from_task(task, scheduled),
        deadline: task.deadline,
        deadline_repeater: None,
        closed: task.completed_at,
        properties,
        body: task.note.clone(),
        unknown_lines: Vec::new(),
        children: Vec::new(),
    }
}

/// Render `estimated_minutes` as Org's `H:MM` form (matching
/// what the importer's effort parser accepts).
fn format_effort(minutes: i64) -> String {
    let h = minutes / 60;
    let m = minutes % 60;
    format!("{h}:{m:02}")
}

/// Scheduled-cookie repeater (Phase 17 / v0.10.3). When the task
/// has a `repeat_rule`, project the canonical RFC 5545 RRULE down
/// to a best-fit Org cookie (`+<N><unit>` with the mode prefix —
/// `+` / `++` / `.+`) so stock `org-agenda` shows a sensible
/// repeat. The full RRULE is still in the `:RRULE:` property
/// drawer; the cookie is the lossy projection. Spec §7.3.3 rule
/// 3 — `:RRULE:` is canonical; the cookie is best-fit. Multi-
/// weekday and BYMONTHDAY patterns degrade to nearest interval
/// per the rrule_cookie helper's contract.
fn scheduled_repeater_from_task(task: &Task, _scheduled: Option<NaiveDate>) -> Option<OrgRepeater> {
    let rule = task.repeat_rule.as_deref()?;
    let mode = atrium_core::repeat::RepeatMode::from_column(task.repeat_mode.as_deref());
    crate::rrule_cookie::rrule_to_org_repeater(rule, mode)
}

/// Replace filesystem-hostile characters in a project / area
/// title with underscores so the generated path is valid on
/// Linux / macOS. We're conservative: anything that's not
/// alphanumeric / space / dash / underscore / dot becomes `_`.
/// Multiple consecutive underscores collapse so a title with
/// many bad chars doesn't render as `_____`.
fn sanitize_filename(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_was_underscore = false;
    for ch in s.chars() {
        let valid = ch.is_alphanumeric() || matches!(ch, ' ' | '-' | '_' | '.');
        if valid {
            out.push(ch);
            prev_was_underscore = ch == '_';
        } else {
            if !prev_was_underscore {
                out.push('_');
                prev_was_underscore = true;
            }
        }
    }
    let trimmed = out.trim_matches(|c: char| c == ' ' || c == '_').to_string();
    if trimmed.is_empty() {
        "untitled".to_string()
    } else {
        trimmed
    }
}

// Suppress dead-code warnings for the chrono imports the
// scheduled_repeater stub will need once filled in.
const _: fn() -> Option<DateTime<Utc>> = || None;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_effort_renders_h_mm() {
        assert_eq!(format_effort(0), "0:00");
        assert_eq!(format_effort(30), "0:30");
        assert_eq!(format_effort(60), "1:00");
        assert_eq!(format_effort(90), "1:30");
        assert_eq!(format_effort(125), "2:05");
    }

    #[test]
    fn sanitize_filename_keeps_safe_chars() {
        assert_eq!(sanitize_filename("Errands"), "Errands");
        assert_eq!(sanitize_filename("Q3 2026"), "Q3 2026");
        assert_eq!(sanitize_filename("Read-me.org"), "Read-me.org");
    }

    #[test]
    fn sanitize_filename_replaces_path_seps() {
        assert_eq!(sanitize_filename("a/b"), "a_b");
        assert_eq!(sanitize_filename("a\\b"), "a_b");
        assert_eq!(sanitize_filename("a:b"), "a_b");
    }

    #[test]
    fn sanitize_filename_collapses_runs() {
        assert_eq!(sanitize_filename("a///b"), "a_b");
        assert_eq!(sanitize_filename("a\\\\\\b"), "a_b");
    }

    #[test]
    fn sanitize_filename_handles_empty_and_all_bad() {
        assert_eq!(sanitize_filename(""), "untitled");
        assert_eq!(sanitize_filename("///"), "untitled");
    }

    /// Build a project with a single TODO + DONE + nested
    /// subtask, write it, then re-parse the file and assert the
    /// expected fields landed.
    #[test]
    fn write_project_round_trips_through_disk() {
        use atrium_core::domain::{NewProject, NewTask};
        use atrium_core::spawn_worker;

        let dir = std::env::temp_dir().join(format!("atrium-write-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        atrium_core::db::configure_pragmas(&conn).unwrap();
        atrium_core::db::migrations::migrate(&mut conn).unwrap();

        // Spawn a worker on a fresh in-memory DB. We use a
        // separate read-conn for the writer, so we open a second
        // file-backed DB and spawn against that.
        let db_path = dir.join("atrium-test.db");
        let read_conn = rusqlite::Connection::open(&db_path).unwrap();
        atrium_core::db::configure_pragmas(&read_conn).unwrap();
        // Run migrations on the file-backed DB so the worker can
        // open it cleanly.
        let mut writer_conn = rusqlite::Connection::open(&db_path).unwrap();
        atrium_core::db::migrations::migrate(&mut writer_conn).unwrap();

        // Drive the worker on a tokio current-thread runtime
        // matching what atrium-cli uses.
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (handle, _changes_rx, _library_rx) =
            runtime.block_on(async move { spawn_worker(writer_conn) });

        // Suppress unused warning on the in-memory conn we created
        // earlier and discard it; the file-backed conn is the
        // canonical store.
        drop(conn);

        // Seed: a project with a TODO parent + DONE child.
        let project = runtime
            .block_on(async {
                handle
                    .create_project(NewProject {
                        title: "Errands".to_string(),
                        ..Default::default()
                    })
                    .await
            })
            .unwrap();
        let parent = runtime
            .block_on(async {
                handle
                    .create_task(NewTask {
                        title: "Buy milk".to_string(),
                        project_id: Some(project.id),
                        ..Default::default()
                    })
                    .await
            })
            .unwrap();
        let _child = runtime
            .block_on(async {
                handle
                    .create_task(NewTask {
                        title: "Pick brand".to_string(),
                        project_id: Some(project.id),
                        parent_id: Some(parent.id),
                        ..Default::default()
                    })
                    .await
            })
            .unwrap();

        let summary = write_project_to_vault(&read_conn, &dir, project.id).unwrap();
        assert_eq!(summary.project_title, "Errands");
        assert_eq!(summary.task_count, 2);

        let written = std::fs::read_to_string(&summary.file_path).unwrap();
        // Round-trip through the parser.
        let parsed = super::super::parse::parse_org_text(&written);
        assert_eq!(parsed.len(), 1, "one top-level headline expected");
        assert_eq!(parsed[0].title, "Buy milk");
        assert_eq!(parsed[0].keyword, Some(OrgKeyword::Todo));
        assert_eq!(parsed[0].children.len(), 1);
        assert_eq!(parsed[0].children[0].title, "Pick brand");
        assert_eq!(parsed[0].children[0].depth, 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// file-level project metadata round-trip end-to-end.
    /// Import an .org file with `#+TITLE:` + a top-level
    /// `:PROPERTIES:` block carrying `:SEQUENTIAL:` /
    /// `:REVIEW_INTERVAL:` / `:LAST_REVIEWED:` / `:ARCHIVED:`;
    /// export the resulting DB; the regenerated file's preamble
    /// matches the source's project-level fields.
    #[tokio::test]
    async fn project_metadata_round_trips_through_db() {
        use crate::org::{import_org_file, parse_org_file_with_meta};
        use atrium_core::spawn_worker;

        let dir =
            std::env::temp_dir().join(format!("atrium-project-meta-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src = dir.join("source.org");
        std::fs::write(
            &src,
            "\
#+TITLE: Q3 Plans
:PROPERTIES:
:ID: 99999999-aaaa-bbbb-cccc-dddddddddddd
:SEQUENTIAL: t
:REVIEW_INTERVAL: 14
:END:

* TODO First task
",
        )
        .unwrap();

        let db_path = dir.join("db.sqlite");
        let mut writer_conn = rusqlite::Connection::open(&db_path).unwrap();
        atrium_core::db::configure_pragmas(&writer_conn).unwrap();
        atrium_core::db::migrations::migrate(&mut writer_conn).unwrap();
        let read_conn = rusqlite::Connection::open(&db_path).unwrap();
        atrium_core::db::configure_pragmas(&read_conn).unwrap();

        let (handle, _changes_rx, _library_rx) = spawn_worker(writer_conn);
        let summary = import_org_file(&handle, &src, false).await.unwrap();
        let project_id = summary.project_id.unwrap();

        // Project should carry the imported metadata.
        let projects = atrium_core::db::read::list_all_projects(&read_conn).unwrap();
        let project = projects.iter().find(|p| p.id == project_id).unwrap();
        assert_eq!(project.title, "Q3 Plans");
        assert_eq!(project.uuid, "99999999-aaaa-bbbb-cccc-dddddddddddd");
        assert!(project.sequential);
        assert_eq!(project.review_interval_days, Some(14));

        let written = write_project_to_vault(&read_conn, &dir, project_id).unwrap();
        let parsed = parse_org_file_with_meta(&written.file_path).unwrap();

        assert_eq!(
            parsed.directives.get("TITLE").map(String::as_str),
            Some("Q3 Plans")
        );
        assert_eq!(
            parsed.file_properties.get("ID").map(String::as_str),
            Some("99999999-aaaa-bbbb-cccc-dddddddddddd")
        );
        assert_eq!(
            parsed.file_properties.get("SEQUENTIAL").map(String::as_str),
            Some("t")
        );
        assert_eq!(
            parsed
                .file_properties
                .get("REVIEW_INTERVAL")
                .map(String::as_str),
            Some("14")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// the custom-keyword round-trip end-to-end.
    /// Import an .org file with a `WAITING` headline; export the
    /// resulting DB; the regenerated file's headline carries
    /// `WAITING` again. orig_keyword is the only data path that
    /// makes this work — without it the writer would emit `TODO`.
    #[tokio::test]
    async fn custom_keyword_round_trips_through_db() {
        use crate::org::{import_org_file, parse_org_text};
        use atrium_core::spawn_worker;

        let dir = std::env::temp_dir().join(format!("atrium-orig-kw-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let src = dir.join("source.org");
        std::fs::write(
            &src,
            "* WAITING External signoff\n* IN-PROGRESS Refactor\n* TODO Plain task\n",
        )
        .unwrap();

        let db_path = dir.join("db.sqlite");
        let mut writer_conn = rusqlite::Connection::open(&db_path).unwrap();
        atrium_core::db::configure_pragmas(&writer_conn).unwrap();
        atrium_core::db::migrations::migrate(&mut writer_conn).unwrap();
        let read_conn = rusqlite::Connection::open(&db_path).unwrap();
        atrium_core::db::configure_pragmas(&read_conn).unwrap();

        let (handle, _changes_rx, _library_rx) = spawn_worker(writer_conn);
        let summary = import_org_file(&handle, &src, false).await.unwrap();
        assert_eq!(summary.tasks_created, 3);
        let project_id = summary.project_id.unwrap();

        let written = write_project_to_vault(&read_conn, &dir, project_id).unwrap();
        let text = std::fs::read_to_string(&written.file_path).unwrap();
        let parsed = parse_org_text(&text);

        // Parser orders headlines as written; we expect the same
        // three keywords back. Match on .keyword.as_str() so the
        // assertion message is readable on failure.
        let kws: Vec<String> = parsed
            .iter()
            .map(|t| {
                t.keyword
                    .as_ref()
                    .map(|k| k.as_str().to_string())
                    .unwrap_or_default()
            })
            .collect();
        assert_eq!(kws, vec!["WAITING", "IN-PROGRESS", "TODO"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_all_projects_writes_each_project() {
        use atrium_core::domain::{NewProject, NewTask};
        use atrium_core::spawn_worker;

        let dir =
            std::env::temp_dir().join(format!("atrium-write-all-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let db_path = dir.join("atrium-test.db");
        let mut writer_conn = rusqlite::Connection::open(&db_path).unwrap();
        atrium_core::db::configure_pragmas(&writer_conn).unwrap();
        atrium_core::db::migrations::migrate(&mut writer_conn).unwrap();

        let read_conn = rusqlite::Connection::open(&db_path).unwrap();
        atrium_core::db::configure_pragmas(&read_conn).unwrap();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (handle, _changes_rx, _library_rx) =
            runtime.block_on(async move { spawn_worker(writer_conn) });

        let p1 = runtime
            .block_on(async {
                handle
                    .create_project(NewProject {
                        title: "Alpha".to_string(),
                        ..Default::default()
                    })
                    .await
            })
            .unwrap();
        let p2 = runtime
            .block_on(async {
                handle
                    .create_project(NewProject {
                        title: "Beta".to_string(),
                        ..Default::default()
                    })
                    .await
            })
            .unwrap();
        let _ = runtime
            .block_on(async {
                handle
                    .create_task(NewTask {
                        title: "Task in alpha".to_string(),
                        project_id: Some(p1.id),
                        ..Default::default()
                    })
                    .await
            })
            .unwrap();
        let _ = runtime
            .block_on(async {
                handle
                    .create_task(NewTask {
                        title: "Task in beta".to_string(),
                        project_id: Some(p2.id),
                        ..Default::default()
                    })
                    .await
            })
            .unwrap();

        let summaries = write_all_projects_to_vault(&read_conn, &dir).unwrap();
        assert_eq!(summaries.len(), 2);
        let alpha = summaries
            .iter()
            .find(|s| s.project_title == "Alpha")
            .expect("alpha summary");
        let beta = summaries
            .iter()
            .find(|s| s.project_title == "Beta")
            .expect("beta summary");
        assert_eq!(alpha.task_count, 1);
        assert_eq!(beta.task_count, 1);
        assert!(alpha.file_path.exists());
        assert!(beta.file_path.exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Phase 17 RRULE canonicalisation: a repeating task emits
    /// BOTH the best-fit Org cookie on SCHEDULED *and* the full
    /// `:RRULE:` property in the drawer. Stock org-agenda renders
    /// the cookie; Atrium's read-back consults `:RRULE:` as
    /// canonical.
    #[test]
    fn write_emits_cookie_and_rrule_for_repeating_task() {
        use atrium_core::domain::{NewProject, NewTask};
        use atrium_core::spawn_worker;

        let dir = std::env::temp_dir().join(format!("atrium-write-rrule-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let db_path = dir.join("atrium-test.db");
        let read_conn = rusqlite::Connection::open(&db_path).unwrap();
        atrium_core::db::configure_pragmas(&read_conn).unwrap();
        let mut writer_conn = rusqlite::Connection::open(&db_path).unwrap();
        atrium_core::db::migrations::migrate(&mut writer_conn).unwrap();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (handle, _changes_rx, _library_rx) =
            runtime.block_on(async move { spawn_worker(writer_conn) });

        let project = runtime
            .block_on(async {
                handle
                    .create_project(NewProject {
                        title: "Repeats".to_string(),
                        ..Default::default()
                    })
                    .await
            })
            .unwrap();

        let scheduled = chrono::NaiveDate::from_ymd_opt(2026, 5, 11).unwrap(); // Mon
        let _ = runtime
            .block_on(async {
                handle
                    .create_task(NewTask {
                        title: "Multi-weekday".to_string(),
                        project_id: Some(project.id),
                        scheduled_for: Some(atrium_core::ScheduledFor::Date(scheduled)),
                        repeat_rule: Some("FREQ=WEEKLY;BYDAY=MO,WE".to_string()),
                        repeat_mode: Some("CUMULATIVE".to_string()),
                        ..Default::default()
                    })
                    .await
            })
            .unwrap();

        let summary = write_project_to_vault(&read_conn, &dir, project.id).unwrap();
        let written = std::fs::read_to_string(&summary.file_path).unwrap();

        // Cookie on SCHEDULED line — best-fit `++1w` (multi-weekday
        // degrades per spec §7.3.3 rule 3).
        assert!(
            written.contains("SCHEDULED: <2026-05-11 Mon ++1w>"),
            "expected SCHEDULED cookie with ++1w; got:\n{written}"
        );
        // Full RRULE in the property drawer — canonical source.
        assert!(
            written.contains(":RRULE: FREQ=WEEKLY;BYDAY=MO,WE"),
            "expected canonical :RRULE: in drawer; got:\n{written}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
