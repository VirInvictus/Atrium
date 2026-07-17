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
//! # Project sub-heading emission (Phase 18, v0.12.0)
//!
//! Heading rows from the `heading` table emit as depth-1 headlines
//! with no TODO keyword (per spec §7.3.1's "project sub-heading"
//! shape). Top-level tasks and headings interleave by `position`
//! across both tables; a task whose position falls after a heading
//! becomes a depth-2 child of that heading. Headings before any
//! task and tasks before any heading both behave intuitively. The
//! Todoist importer (v0.12.0) drives this: a CSV section becomes a
//! heading row, and the tasks under it inherit a position that
//! sorts between the section's heading and the next section's.
//!
//! Org tasks without coordinated positions still emit cleanly —
//! a project with no headings produces the same flat top-level
//! list it did pre-v0.12.0.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, NaiveDate, Timelike, Utc};
use rusqlite::Connection;

use super::emit::{emit_org_file_with_meta, emit_org_text_with_meta};
use super::parse::{OrgFile, OrgKeyword, OrgRepeater, OrgTask};
use atrium_core::domain::{Heading, Project, ScheduledFor, Task};
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
    let built = build_project_org_file(conn, vault_root, project_id)?;

    if let Some(parent) = built.path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| WriteError::Io {
            path: parent.display().to_string(),
            source: e,
        })?;
    }

    emit_org_file_with_meta(&built.path, &built.file).map_err(|e| WriteError::Io {
        path: built.path.display().to_string(),
        source: e,
    })?;

    Ok(WriteSummary {
        project_id,
        project_title: built.project_title,
        task_count: built.task_count,
        file_path: built.path,
    })
}

/// Render one project to its canonical Org text without touching the
/// filesystem, alongside the path it would land at. Because
/// [`emit_org_file_with_meta`] writes exactly
/// [`emit_org_text_with_meta`]'s bytes, the returned string is
/// byte-identical to what [`write_project_to_vault`] would produce —
/// so the vault writer's startup seed can compare it against the
/// on-disk file to decide whether that file is already in sync (ours)
/// or carries an external edit (needs a conflict backup).
pub fn render_project_to_string(
    conn: &Connection,
    vault_root: &Path,
    project_id: i64,
) -> Result<(PathBuf, String), WriteError> {
    let built = build_project_org_file(conn, vault_root, project_id)?;
    Ok((built.path, emit_org_text_with_meta(&built.file)))
}

/// The materials needed to write (or render) one project's `.org`
/// file: the assembled [`OrgFile`], its destination path, and the
/// bits [`WriteSummary`] carries back to callers.
struct BuiltProject {
    file: OrgFile,
    path: PathBuf,
    project_title: String,
    task_count: usize,
}

/// Read a project + its tasks / headings / tags / clock entries and
/// assemble the [`OrgFile`] tree, without any filesystem side effects.
/// Shared by [`write_project_to_vault`] (which then writes it) and
/// [`render_project_to_string`] (which emits it to a string).
fn build_project_org_file(
    conn: &Connection,
    vault_root: &Path,
    project_id: i64,
) -> Result<BuiltProject, WriteError> {
    let project = atrium_core::db::read::project_by_id(conn, project_id)?
        .ok_or(WriteError::ProjectNotFound(project_id))?;
    let area_title = match project.area_id {
        Some(aid) => atrium_core::db::read::area_by_id(conn, aid)?.map(|a| a.title),
        None => None,
    };
    let tasks = atrium_core::db::read::list_all_in_project(conn, project_id)?;
    let headings = atrium_core::db::read::list_headings_in_project(conn, project_id)?;
    let tag_names = atrium_core::db::read::tag_names_per_task(conn)?;

    // v0.17.0 — Phase 18.5 Tier-1 CLOCK time tracking. Pre-load
    // clock entries per task in one query; `task_to_org` reads
    // from this map when building each headline. Tasks without
    // entries pay nothing (the lookup misses; OrgTask keeps its
    // empty default Vec).
    let clock_by_task = atrium_core::db::read::clock_entries_per_project(conn, project_id)?;

    let mut tree = build_project_tree(&tasks, &headings, &tag_names, &clock_by_task);
    // v0.15.0 — stamp statistics cookies on every parent.
    for node in &mut tree {
        stamp_statistics_cookies(node);
    }
    // v0.16.0 — Phase 18.5 Tier-1 custom TODO sequences. Read
    // the sidecar's configured sequence (if any) so the writer
    // can project a `#+TODO:` preamble. Single-sequence-per-vault
    // is the typical Org pattern; multi-sequence support would
    // need a different directives shape (the HashMap is keyed by
    // name, so two #+TODO: keys would collide). Defer that until
    // a real user asks. NotFound silently → no preamble, which
    // is the correct behaviour for vaults that haven't configured
    // sequences.
    let todo_sequence = crate::sidecar::read_sidecar(vault_root)
        .ok()
        .and_then(|s| s.todo_sequences.into_iter().next());
    // file-level preamble carries the project title +
    // project metadata so the importer can round-trip them
    // cleanly. The OrgFile struct bundles directives +
    // file_properties + headlines.
    let file = OrgFile {
        directives: build_file_directives(&project, todo_sequence.as_ref()),
        file_properties: build_file_properties(&project),
        headlines: tree,
    };

    let path = build_project_vault_path(vault_root, area_title.as_deref(), &project.title);

    Ok(BuiltProject {
        file,
        path,
        project_title: project.title,
        task_count: tasks.len(),
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

/// Build the Org headline tree for one project — interleaves
/// heading rows and top-level tasks by `position`, then nests
/// each task's subtree of subtasks underneath.
///
/// Layout rules:
///
/// - Top-level items (heading rows + tasks with `parent_id =
///   NULL`) sort by their `position` field. On a position tie,
///   headings precede tasks (a section break that splits two tasks
///   should appear above the second one in the source order).
/// - When the cursor crosses a heading, subsequent top-level
///   tasks attach as children of that heading at depth 2; their
///   subtask subtrees recurse from depth 3. Top-level tasks
///   before the first heading stay at depth 1.
/// - Subtasks (tasks with a `parent_id` that points into the
///   same project) collect under their parent task in
///   position order.
/// - Orphaned subtasks (parent_id pointing outside the project,
///   shouldn't happen, but defensive) fall back to top-level.
fn build_project_tree(
    tasks: &[Task],
    headings: &[Heading],
    tag_names: &HashMap<i64, Vec<String>>,
    clock_by_task: &HashMap<i64, Vec<atrium_core::TaskClockEntry>>,
) -> Vec<OrgTask> {
    let by_id: HashMap<i64, &Task> = tasks.iter().map(|t| (t.id, t)).collect();
    let mut consumed: Vec<bool> = vec![false; tasks.len()];

    /// Position-ordered top-level item — either a heading row or
    /// a top-level task. Carries the index back into the source
    /// slice so we can read out the original row when we visit it.
    enum Item {
        Heading(usize),
        Task(usize),
    }

    let mut items: Vec<(f64, Item)> = Vec::with_capacity(headings.len() + tasks.len());
    for (i, h) in headings.iter().enumerate() {
        items.push((h.position, Item::Heading(i)));
    }
    for (i, t) in tasks.iter().enumerate() {
        let is_top = match t.parent_id {
            None => true,
            Some(pid) => !by_id.contains_key(&pid),
        };
        if is_top {
            items.push((t.position, Item::Task(i)));
        }
    }
    // Sort by position; on tie, headings precede tasks so a section
    // header introduces the run of tasks at its position. NaN
    // shouldn't appear (REAL NOT NULL with sane writer-side values),
    // but partial_cmp's None falls through to Ordering::Equal so we
    // don't panic on it.
    items.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| match (&a.1, &b.1) {
                (Item::Heading(_), Item::Task(_)) => std::cmp::Ordering::Less,
                (Item::Task(_), Item::Heading(_)) => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Equal,
            })
    });

    let mut top: Vec<OrgTask> = Vec::new();
    let mut current_heading_idx: Option<usize> = None;

    for (_pos, item) in items {
        match item {
            Item::Heading(hi) => {
                top.push(heading_to_org(&headings[hi]));
                current_heading_idx = Some(top.len() - 1);
            }
            Item::Task(ti) => {
                let depth = if current_heading_idx.is_some() { 2 } else { 1 };
                let node =
                    build_task_subtree(ti, tasks, tag_names, clock_by_task, depth, &mut consumed);
                match current_heading_idx {
                    Some(hi) => top[hi].children.push(node),
                    None => top.push(node),
                }
            }
        }
    }

    // Any unconsumed task is an orphaned subtask (its parent_id
    // pointed outside this project's slice). Append it at top
    // level so we never silently drop a row on writeback.
    for i in 0..tasks.len() {
        if !consumed[i] {
            let node = build_task_subtree(i, tasks, tag_names, clock_by_task, 1, &mut consumed);
            top.push(node);
        }
    }

    top
}

/// Recursively build a task's OrgTask, attaching its subtasks (in
/// position order) as children at `depth + 1`.
fn build_task_subtree(
    idx: usize,
    tasks: &[Task],
    tag_names: &HashMap<i64, Vec<String>>,
    clock_by_task: &HashMap<i64, Vec<atrium_core::TaskClockEntry>>,
    depth: usize,
    consumed: &mut [bool],
) -> OrgTask {
    consumed[idx] = true;
    let mut node = task_to_org(&tasks[idx], depth, tag_names, clock_by_task);
    let parent_id = tasks[idx].id;

    // Children sorted by position so the emitted file matches
    // the project's UI order. Index pairs avoid cloning Tasks.
    let mut child_indices: Vec<usize> = (0..tasks.len())
        .filter(|&j| !consumed[j] && tasks[j].parent_id == Some(parent_id))
        .collect();
    child_indices.sort_by(|&a, &b| {
        tasks[a]
            .position
            .partial_cmp(&tasks[b].position)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for j in child_indices {
        if consumed[j] {
            continue;
        }
        let child = build_task_subtree(j, tasks, tag_names, clock_by_task, depth + 1, consumed);
        node.children.push(child);
    }
    node
}

/// v0.15.0 — Phase 18.5 Tier-1 statistics-cookie projection.
/// Walks an OrgTask subtree post-order; any node with children
/// gets a `Counter { done, total }` cookie counting *immediate*
/// children only (Org's `org-hierarchical-todo-statistics`
/// default — recursive variants exist but aren't the convention).
///
/// "Done" means the child's keyword is Done or Cancelled. TODO,
/// custom workflow keywords (WAITING / IN-PROGRESS / etc.), and
/// keyword-less section sub-headings count as not-done. The done
/// criterion is keyword-based on purpose: the Org file is the
/// surface, and Org's own statistics counter only sees the
/// keyword. Custom keyword sequences (Phase 18.5 follow-up,
/// v0.16.0) will let the user map workflow keywords to "done"
/// per-vault — until then, Done|Cancelled is the canonical set.
///
/// Preserves an existing `statistics_cookie` *shape* (Counter
/// vs Percent) when present; only the values get overwritten.
/// This is what gives the user control: if their source file
/// has `[40%]`, the writer keeps emitting `[N%]` after Atrium
/// recomputes from DB state.
fn stamp_statistics_cookies(node: &mut OrgTask) {
    // Recurse first so children get their own cookies.
    for child in &mut node.children {
        stamp_statistics_cookies(child);
    }
    // v0.15.0 — child TODOs + body checkboxes both contribute to
    // the cookie. Mirrors Org's `org-checkbox-hierarchical-statistics`
    // (default on). A task with zero child headlines but a body
    // checklist still earns a cookie.
    let (body_done, body_total) = atrium_core::count_body_checkboxes(&node.body);
    let mut child_done = 0u32;
    let child_total = u32::try_from(node.children.len()).unwrap_or(u32::MAX);
    for child in &node.children {
        if matches!(
            child.keyword,
            Some(super::parse::OrgKeyword::Done | super::parse::OrgKeyword::Cancelled)
        ) {
            child_done = child_done.saturating_add(1);
        }
    }
    let total = child_total.saturating_add(body_total);
    let done = child_done.saturating_add(body_done);
    if total == 0 {
        // Leaf with no body checkboxes — no cookie. Clear any
        // stale shape captured on read.
        node.statistics_cookie = None;
        return;
    }
    use super::parse::StatisticsCookie;
    let new_cookie = match node.statistics_cookie {
        Some(StatisticsCookie::Percent { .. }) => {
            // Preserve percent shape.
            let value = ((u64::from(done) * 100) / u64::from(total)) as u8;
            StatisticsCookie::Percent { value }
        }
        // Default + Counter both land on the fraction form.
        _ => StatisticsCookie::Counter { done, total },
    };
    node.statistics_cookie = Some(new_cookie);
}

/// Convert a Heading row into a depth-1 OrgTask carrying the
/// section's title and `:ID:` (uuid) so a future Org importer
/// can match heading rows back by id rather than by title.
fn heading_to_org(heading: &Heading) -> OrgTask {
    let mut properties: HashMap<String, String> = HashMap::new();
    if !heading.uuid.is_empty() {
        properties.insert("ID".into(), heading.uuid.clone());
    }
    OrgTask {
        depth: 1,
        keyword: None,
        title: heading.title.clone(),
        tags: Vec::new(),
        scheduled: None,
        scheduled_time: None,
        scheduled_repeater: None,
        scheduled_warning: None,
        deadline: None,
        deadline_repeater: None,
        deadline_warning: None,
        // v0.15.0 — sub-headings get cookies set later by the
        // emit-time projection from DB state, not here.
        statistics_cookie: None,
        // v0.17.0 — sub-headings don't carry clock entries (entries
        // attach to TODO headlines, not section dividers).
        clock_entries: Vec::new(),
        logbook_unknown_lines: Vec::new(),
        closed: None,
        properties,
        body: String::new(),
        unknown_lines: Vec::new(),
        children: Vec::new(),
    }
}

/// file-level directives for a project's `.org` file.
/// Currently emits `#+TITLE:` and (v0.16.0, optional) `#+TODO:`
/// when the vault sidecar configures a custom keyword sequence.
/// Other directives (`#+CATEGORY:`, `#+FILETAGS:`, `#+STARTUP:`
/// …) follow when Atrium grows project-level analogues.
fn build_file_directives(
    project: &Project,
    todo_sequence: Option<&crate::sidecar::TodoSequenceEntry>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    out.insert("TITLE".to_string(), project.title.clone());
    // v0.16.0 — emit `#+TODO: STATE1 STATE2 | DONE1 DONE2` when
    // the vault has a configured sequence. Skipping when the
    // workflow + done sets are both empty avoids emitting an
    // empty `#+TODO: |` line that would just confuse readers.
    if let Some(seq) = todo_sequence
        && (!seq.workflow.is_empty() || !seq.done.is_empty())
    {
        let workflow = seq.workflow.join(" ");
        let done = seq.done.join(" ");
        // Org's pipe-with-spaces convention is what every
        // tutorial uses; sticking to it keeps the file readable
        // alongside the rest of an Emacs user's Org corpus.
        let value = format!("{workflow} | {done}");
        out.insert("TODO".to_string(), value);
    }
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

fn task_to_org(
    task: &Task,
    depth: usize,
    tag_names: &HashMap<i64, Vec<String>>,
    clock_by_task: &HashMap<i64, Vec<atrium_core::TaskClockEntry>>,
) -> OrgTask {
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
    // v0.24.0 — merge custom property-drawer extras back into
    // the emitted drawer so spec §7.3.3 rule 1 holds. The
    // importer / watcher stripped modeled keys before stashing
    // here so the typed-column value always wins; `entry().or_insert`
    // defends against a hand-crafted DB row that puts a
    // modeled-name key in `extra_properties`.
    for (key, value) in &task.extra_properties {
        properties
            .entry(key.clone())
            .or_insert_with(|| value.clone());
    }

    let tags = tag_names.get(&task.id).cloned().unwrap_or_default();

    OrgTask {
        depth,
        keyword,
        title: task.title.clone(),
        tags,
        scheduled,
        // v0.19.0 — Phase 18.5 Tier-2 time-of-day on schedule.
        // Only meaningful when `scheduled_for` is a Date; the
        // `scheduled` local was already None for Someday/None,
        // so threading the column directly is safe.
        scheduled_time: task.scheduled_time,
        scheduled_repeater: scheduled_repeater_from_task(task, scheduled),
        scheduled_warning: None,
        deadline: task.deadline,
        deadline_repeater: None,
        // v0.14.0 — project the per-task warning window onto the
        // DEADLINE cookie. NULL → no suffix (org-agenda falls back
        // to its global default); Some(n) → `-Nd` after the date.
        // Stored as `u32` in OrgTask but the DB column is `i64`;
        // negative values shouldn't reach here (the GUI clamps to
        // 0 and the parser only produces unsigned), but we clamp
        // defensively before the cast.
        deadline_warning: task
            .deadline_warn_days
            .filter(|n| *n >= 0)
            .and_then(|n| u32::try_from(n).ok()),
        // v0.15.0 — projected from DB at emit time, not here.
        // The build_project_tree pass that walks tasks + headings
        // recomputes counters for parents after the children are
        // attached, since this scope can't see the children yet.
        statistics_cookie: None,
        // v0.17.0 — Phase 18.5 Tier-1 CLOCK time tracking.
        // Map DB clock entries to the OrgTask shape. The
        // emitter suppresses in-progress entries on its own;
        // we pass them all through here.
        clock_entries: clock_by_task
            .get(&task.id)
            .map(|entries| {
                entries
                    .iter()
                    .map(|e| super::parse::OrgClockEntry {
                        started: e.started_at,
                        ended: e.ended_at,
                        note: e.note.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        logbook_unknown_lines: Vec::new(),
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

    // v0.15.0 — Phase 18.5 statistics cookies. The projection
    // walks an OrgTask tree post-order; every parent gets a
    // `Counter` cookie counting child Done|Cancelled vs total
    // children, plus body-checkbox done/total folded in.
    #[test]
    fn stamp_cookie_counts_only_immediate_children() {
        use super::super::parse::{OrgKeyword, OrgTask, StatisticsCookie};
        let mut grandchild = OrgTask::default_test_node(3);
        grandchild.keyword = Some(OrgKeyword::Done);
        let mut child = OrgTask::default_test_node(2);
        child.keyword = Some(OrgKeyword::Todo);
        child.children.push(grandchild);
        let mut parent = OrgTask::default_test_node(1);
        parent.keyword = Some(OrgKeyword::Todo);
        parent.children.push(child);
        stamp_statistics_cookies(&mut parent);
        // Parent has 1 immediate child (the TODO middle one).
        // Done count is 0 (the middle child is TODO; the
        // grandchild's DONE doesn't bubble up — Org's default
        // `org-hierarchical-todo-statistics` is non-recursive).
        assert_eq!(
            parent.statistics_cookie,
            Some(StatisticsCookie::Counter { done: 0, total: 1 })
        );
        // The middle child has 1 DONE grandchild.
        assert_eq!(
            parent.children[0].statistics_cookie,
            Some(StatisticsCookie::Counter { done: 1, total: 1 })
        );
    }

    #[test]
    fn stamp_cookie_folds_body_checkboxes() {
        use super::super::parse::{OrgKeyword, OrgTask, StatisticsCookie};
        let mut child = OrgTask::default_test_node(2);
        child.keyword = Some(OrgKeyword::Todo);
        let mut parent = OrgTask::default_test_node(1);
        parent.keyword = Some(OrgKeyword::Todo);
        parent.body = "- [X] body done\n- [ ] body open\n- [-] partial".to_string();
        parent.children.push(child);
        stamp_statistics_cookies(&mut parent);
        // 1 child + 3 body checkboxes = 4 total; 0 child done +
        // 1 body done = 1 done.
        assert_eq!(
            parent.statistics_cookie,
            Some(StatisticsCookie::Counter { done: 1, total: 4 })
        );
    }

    #[test]
    fn stamp_cookie_preserves_percent_shape() {
        use super::super::parse::{OrgKeyword, OrgTask, StatisticsCookie};
        let mut child_done = OrgTask::default_test_node(2);
        child_done.keyword = Some(OrgKeyword::Done);
        let mut child_open = OrgTask::default_test_node(2);
        child_open.keyword = Some(OrgKeyword::Todo);
        let mut parent = OrgTask::default_test_node(1);
        parent.keyword = Some(OrgKeyword::Todo);
        parent.statistics_cookie = Some(StatisticsCookie::Percent { value: 0 });
        parent.children.push(child_done);
        parent.children.push(child_open);
        stamp_statistics_cookies(&mut parent);
        // Source had a percent cookie; projection preserves the
        // shape and recomputes the value (1 of 2 = 50%).
        assert_eq!(
            parent.statistics_cookie,
            Some(StatisticsCookie::Percent { value: 50 })
        );
    }

    #[test]
    fn stamp_cookie_clears_on_leaf_with_no_body_checkboxes() {
        use super::super::parse::{OrgKeyword, OrgTask, StatisticsCookie};
        let mut leaf = OrgTask::default_test_node(1);
        leaf.keyword = Some(OrgKeyword::Todo);
        // Stale cookie captured on read — should be cleared since
        // the task has neither children nor body checkboxes.
        leaf.statistics_cookie = Some(StatisticsCookie::Counter { done: 0, total: 0 });
        stamp_statistics_cookies(&mut leaf);
        assert_eq!(leaf.statistics_cookie, None);
    }

    #[test]
    fn stamp_cookie_appears_on_leaf_with_only_body_checkboxes() {
        use super::super::parse::{OrgKeyword, OrgTask, StatisticsCookie};
        let mut leaf = OrgTask::default_test_node(1);
        leaf.keyword = Some(OrgKeyword::Todo);
        leaf.body = "- [X] one\n- [ ] two".to_string();
        stamp_statistics_cookies(&mut leaf);
        assert_eq!(
            leaf.statistics_cookie,
            Some(StatisticsCookie::Counter { done: 1, total: 2 })
        );
    }

    // v0.16.0 — Phase 18.5 Tier-1 #+TODO: preamble emission.
    // No sidecar configured → no preamble. Sidecar with an
    // empty workflow + done → no preamble. Sidecar with values →
    // preamble lands in the directive map.
    #[test]
    fn build_file_directives_omits_todo_when_no_sequence() {
        use atrium_core::test_support::dummy_task;
        let _ = dummy_task(0); // touch test_support so it stays in the linked set
        let project = atrium_core::Project {
            id: 1,
            uuid: "u".into(),
            title: "Errands".into(),
            note: String::new(),
            area_id: None,
            sequential: false,
            review_interval_days: None,
            last_reviewed_at: None,
            archived_at: None,
            position: 1.0,
            created_at: chrono::Utc::now(),
            modified_at: chrono::Utc::now(),
        };
        let directives = build_file_directives(&project, None);
        assert_eq!(directives.get("TITLE").map(String::as_str), Some("Errands"));
        assert!(!directives.contains_key("TODO"));
    }

    #[test]
    fn build_file_directives_emits_todo_with_sequence() {
        let project = atrium_core::Project {
            id: 1,
            uuid: "u".into(),
            title: "Errands".into(),
            note: String::new(),
            area_id: None,
            sequential: false,
            review_interval_days: None,
            last_reviewed_at: None,
            archived_at: None,
            position: 1.0,
            created_at: chrono::Utc::now(),
            modified_at: chrono::Utc::now(),
        };
        let seq = crate::sidecar::TodoSequenceEntry {
            name: "default".into(),
            workflow: vec!["TODO".into(), "NEXT".into(), "WAITING".into()],
            done: vec!["DONE".into(), "CANCELLED".into()],
        };
        let directives = build_file_directives(&project, Some(&seq));
        assert_eq!(
            directives.get("TODO").map(String::as_str),
            Some("TODO NEXT WAITING | DONE CANCELLED")
        );
    }

    #[test]
    fn build_file_directives_omits_todo_when_sequence_is_empty() {
        let project = atrium_core::Project {
            id: 1,
            uuid: "u".into(),
            title: "Errands".into(),
            note: String::new(),
            area_id: None,
            sequential: false,
            review_interval_days: None,
            last_reviewed_at: None,
            archived_at: None,
            position: 1.0,
            created_at: chrono::Utc::now(),
            modified_at: chrono::Utc::now(),
        };
        let seq = crate::sidecar::TodoSequenceEntry::default();
        let directives = build_file_directives(&project, Some(&seq));
        assert!(!directives.contains_key("TODO"));
    }

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

    /// Phase 18 v0.12.0 — project sub-headings emit. A project with
    /// two heading rows interleaved with tasks by `position` writes
    /// to disk as a section-bearing Org file: each heading becomes
    /// a depth-1 keyword-less headline and tasks whose position
    /// falls between two headings nest under the preceding one at
    /// depth 2.
    #[test]
    fn write_emits_headings_as_depth1_sections() {
        use atrium_core::domain::{NewHeading, NewProject, NewTask};
        use atrium_core::spawn_worker;

        let dir = std::env::temp_dir().join(format!("atrium-headings-test-{}", std::process::id()));
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
                        title: "Weekly chores".to_string(),
                        ..Default::default()
                    })
                    .await
            })
            .unwrap();

        // Position layout (Todoist mapper will lay it out this way):
        //   heading "Kitchen" → 1.0
        //   task    "Wipe counters" → 1.5  (under Kitchen)
        //   task    "Empty compost" → 1.75 (under Kitchen)
        //   heading "Laundry" → 2.0
        //   task    "Wash darks"    → 2.5  (under Laundry)
        //
        // We can't set positions through the public worker API, so
        // we drive them by inserting in the right order: the worker
        // assigns next_*_position(project_id) as max+1 per table.
        // Headings get 1.0 then 2.0; tasks get 1.0, 2.0, 3.0. To
        // realise the layout above we patch positions via a write
        // connection at the end so the writer reads the intended
        // shape — this is test-only mechanics, not production code.
        let kitchen = runtime
            .block_on(async {
                handle
                    .ensure_heading(project.id, "Kitchen".to_string())
                    .await
            })
            .unwrap();
        let counters = runtime
            .block_on(async {
                handle
                    .create_task(NewTask {
                        title: "Wipe counters".to_string(),
                        project_id: Some(project.id),
                        ..Default::default()
                    })
                    .await
            })
            .unwrap();
        let compost = runtime
            .block_on(async {
                handle
                    .create_task(NewTask {
                        title: "Empty compost".to_string(),
                        project_id: Some(project.id),
                        ..Default::default()
                    })
                    .await
            })
            .unwrap();
        let laundry = runtime
            .block_on(async {
                handle
                    .ensure_heading(project.id, "Laundry".to_string())
                    .await
            })
            .unwrap();
        let darks = runtime
            .block_on(async {
                handle
                    .create_task(NewTask {
                        title: "Wash darks".to_string(),
                        project_id: Some(project.id),
                        ..Default::default()
                    })
                    .await
            })
            .unwrap();

        // Patch positions to realise the interleaved layout.
        let patch = rusqlite::Connection::open(&db_path).unwrap();
        patch
            .execute(
                "UPDATE heading SET position = 1.0 WHERE id = ?1",
                rusqlite::params![kitchen.id],
            )
            .unwrap();
        patch
            .execute(
                "UPDATE heading SET position = 2.0 WHERE id = ?1",
                rusqlite::params![laundry.id],
            )
            .unwrap();
        patch
            .execute(
                "UPDATE task SET position = 1.5 WHERE id = ?1",
                rusqlite::params![counters.id],
            )
            .unwrap();
        patch
            .execute(
                "UPDATE task SET position = 1.75 WHERE id = ?1",
                rusqlite::params![compost.id],
            )
            .unwrap();
        patch
            .execute(
                "UPDATE task SET position = 2.5 WHERE id = ?1",
                rusqlite::params![darks.id],
            )
            .unwrap();

        let summary = write_project_to_vault(&read_conn, &dir, project.id).unwrap();
        let written = std::fs::read_to_string(&summary.file_path).unwrap();
        let parsed = super::super::parse::parse_org_text(&written);

        assert_eq!(parsed.len(), 2, "two top-level sub-headings expected");
        assert_eq!(parsed[0].title, "Kitchen");
        assert_eq!(parsed[0].keyword, None);
        assert_eq!(
            parsed[0].properties.get("ID").map(String::as_str),
            Some(kitchen.uuid.as_str()),
        );
        assert_eq!(parsed[0].children.len(), 2);
        assert_eq!(parsed[0].children[0].title, "Wipe counters");
        assert_eq!(parsed[0].children[0].depth, 2);
        assert_eq!(parsed[0].children[0].keyword, Some(OrgKeyword::Todo));
        assert_eq!(parsed[0].children[1].title, "Empty compost");

        assert_eq!(parsed[1].title, "Laundry");
        assert_eq!(parsed[1].keyword, None);
        assert_eq!(parsed[1].children.len(), 1);
        assert_eq!(parsed[1].children[0].title, "Wash darks");
        assert_eq!(parsed[1].children[0].depth, 2);

        let _ = std::fs::remove_dir_all(&dir);
        // NewHeading import: keep the binding live so the unused-import
        // lint doesn't fire on a future stub-out of this test.
        let _ = NewHeading::default();
    }

    /// Tasks before any heading still emit at depth 1; the writer
    /// only reaches depth 2 once the cursor crosses a heading row.
    #[test]
    fn write_keeps_pre_heading_tasks_at_top_level() {
        use atrium_core::domain::{NewProject, NewTask};
        use atrium_core::spawn_worker;

        let dir =
            std::env::temp_dir().join(format!("atrium-pre-heading-test-{}", std::process::id()));
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
                        title: "Mixed".to_string(),
                        ..Default::default()
                    })
                    .await
            })
            .unwrap();
        let pre_task = runtime
            .block_on(async {
                handle
                    .create_task(NewTask {
                        title: "Standalone".to_string(),
                        project_id: Some(project.id),
                        ..Default::default()
                    })
                    .await
            })
            .unwrap();
        let heading = runtime
            .block_on(async {
                handle
                    .ensure_heading(project.id, "Section".to_string())
                    .await
            })
            .unwrap();
        let under_task = runtime
            .block_on(async {
                handle
                    .create_task(NewTask {
                        title: "Under section".to_string(),
                        project_id: Some(project.id),
                        ..Default::default()
                    })
                    .await
            })
            .unwrap();

        // pre_task @ 0.5, heading @ 1.0, under_task @ 1.5.
        let patch = rusqlite::Connection::open(&db_path).unwrap();
        patch
            .execute(
                "UPDATE task SET position = 0.5 WHERE id = ?1",
                rusqlite::params![pre_task.id],
            )
            .unwrap();
        patch
            .execute(
                "UPDATE heading SET position = 1.0 WHERE id = ?1",
                rusqlite::params![heading.id],
            )
            .unwrap();
        patch
            .execute(
                "UPDATE task SET position = 1.5 WHERE id = ?1",
                rusqlite::params![under_task.id],
            )
            .unwrap();

        let summary = write_project_to_vault(&read_conn, &dir, project.id).unwrap();
        let parsed = super::super::parse::parse_org_text(
            &std::fs::read_to_string(&summary.file_path).unwrap(),
        );

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].title, "Standalone");
        assert_eq!(parsed[0].keyword, Some(OrgKeyword::Todo));
        assert_eq!(parsed[0].depth, 1);
        assert_eq!(parsed[0].children.len(), 0);

        assert_eq!(parsed[1].title, "Section");
        assert_eq!(parsed[1].keyword, None);
        assert_eq!(parsed[1].children.len(), 1);
        assert_eq!(parsed[1].children[0].title, "Under section");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
