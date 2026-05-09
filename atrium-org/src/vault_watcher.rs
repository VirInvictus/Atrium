// SPDX-License-Identifier: MIT
//! Vault → DB sync watcher (Phase 17, v0.10.0).
//!
//! Pairs with [`crate::vault_writer::VaultWriter`]. Watches the
//! configured Org vault for `.org` file changes, debounces them ~200
//! ms, and merges the file's parsed state back into the SQLite store
//! through atrium-core's `WorkerHandle`. The companion
//! [`crate::self_write::RecentWrites`] set suppresses inotify events
//! the writer just generated so the loop doesn't echo.
//!
//! v0.10.0 ships the working slice — external add / edit / delete on
//! tasks that already have `:ID:` properties, plus `:ID:` allocation
//! for headlines added in Emacs without one. Conflict detection (mtime
//! race), malformed-file pause/resume, RRULE divergence detection, and
//! the agenda-parity acceptance test land across the v0.10.x patch
//! arc per the Phase 17 roadmap entry.
//!
//! Threading model:
//!
//! 1. `notify::recommended_watcher` spawns its own OS thread for the
//!    inotify callback.
//! 2. The callback `try_send`s raw events into an `UnboundedSender`.
//! 3. The [`VaultWatcher`] task runs on the existing tokio runtime,
//!    receives raw events, and applies a 200 ms debounce keyed on
//!    file path (last-deadline-wins, matching the writer's pattern).
//! 4. After debounce, the watcher consults `recent_writes` and
//!    drops self-writes; remaining events parse the file and submit
//!    diff results through `WorkerHandle`.
//!
//! Diff strategy: match parsed tasks to DB tasks by `:ID:` property.
//! Tasks present in parsed but missing in DB → `create_task`. Tasks
//! present in DB but missing in parsed → `delete_task`. Tasks present
//! in both with differences → `update_task` + `set_task_tags`.
//! Headlines parsed without `:ID:` get a fresh UUIDv4 and the file is
//! rewritten by the vault writer (suppressed by the self-write filter).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use atrium_core::db::read_pool::ReadPool;
use atrium_core::domain::{NewProject, NewTask, ProjectUpdate, ScheduledFor, Task, TaskUpdate};
use atrium_core::{DbError, WorkerHandle};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{trace, warn};
use uuid::Uuid;

use crate::VaultEvent;
use crate::org::{OrgFile, OrgKeyword, OrgTask, parse_org_file_with_meta};
use crate::self_write::RecentWrites;

/// 50 ms tick keeps detection latency below the human-perceptible
/// threshold; combined with the 200 ms debounce, total round-trip
/// from an Emacs save to a DB write is ≤ 250 ms.
const TICK: Duration = Duration::from_millis(50);
const DEBOUNCE: Duration = Duration::from_millis(200);

/// File-level property key carrying the project's `:ID:`. The
/// writer emits this on every project file (spec §7.3.3 rule 2).
const PROJECT_ID_PROPERTY: &str = "ID";

/// Background task. Owns the `WorkerHandle`, read pool, debounce
/// state, and the inotify watcher (held to keep the notify thread
/// alive). Drop the watcher to stop the inotify backend.
///
/// The optional `events_tx` ferries `VaultEvent::ParseFailed` +
/// `VaultEvent::ParseRecovered` notices up to the GUI for toast
/// surfacing. `None` keeps the pre-event log-only behaviour.
///
/// `paused` is the malformed-file pause set. A file lands in
/// `paused` when its last parse failed; while present, the
/// watcher emits no further `ParseFailed` events (one per
/// transition is enough — repeated bad saves don't spam toasts).
/// When a paused file parses cleanly the entry comes out and a
/// `ParseRecovered` event surfaces.
pub struct VaultWatcher {
    root: PathBuf,
    handle: WorkerHandle,
    pool: ReadPool,
    recent_writes: Arc<RwLock<RecentWrites>>,
    events_tx: Option<mpsc::UnboundedSender<VaultEvent>>,
    rx: mpsc::UnboundedReceiver<Event>,
    pending: HashMap<PathBuf, Instant>,
    paused: Arc<Mutex<HashSet<PathBuf>>>,
    _watcher: RecommendedWatcher,
}

impl VaultWatcher {
    /// Run the watcher to completion. Returns when the event channel
    /// closes (i.e., the inotify watcher is dropped).
    pub async fn run(mut self) {
        let mut ticker = tokio::time::interval(TICK);
        loop {
            tokio::select! {
                event = self.rx.recv() => {
                    match event {
                        Some(event) => self.handle_event(event),
                        None => break,
                    }
                }
                _ = ticker.tick() => {
                    self.flush_due().await;
                }
            }
        }
    }

    fn handle_event(&mut self, event: Event) {
        let interesting = matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
        );
        if !interesting {
            return;
        }
        for path in event.paths {
            if path.extension().is_some_and(|e| e == "org") {
                let deadline = Instant::now() + DEBOUNCE;
                self.pending.insert(path, deadline);
            }
        }
    }

    async fn flush_due(&mut self) {
        let now = Instant::now();
        let due: Vec<PathBuf> = self
            .pending
            .iter()
            .filter(|(_, dl)| **dl <= now)
            .map(|(p, _)| p.clone())
            .collect();
        for path in due {
            self.pending.remove(&path);
            // Get the file's current mtime so the self-write
            // filter can match exactly. A missing file (deleted)
            // skips this branch and goes through process_file's
            // delete-handling.
            let mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
            let is_self = match mtime {
                Some(m) => self
                    .recent_writes
                    .read()
                    .map(|rw| rw.is_self_write(&path, m))
                    .unwrap_or(false),
                None => false,
            };
            if is_self {
                trace!(path = %path.display(), "vault watcher: self-write, skipping");
                continue;
            }
            if let Err(e) = self.process_file(&path).await {
                warn!(path = %path.display(), error = %e, "vault watcher: process failed");
            }
        }
    }

    async fn process_file(&self, path: &Path) -> Result<(), DbError> {
        if !path.exists() {
            // The user `rm`ed a vault file or moved it out of the
            // vault. Per spec §3.5 (DB canonical, vault projected)
            // we *don't* auto-delete tasks — that would let an
            // accidental `rm` destroy a hundred rows. Instead we
            // surface a toast so the user knows the projection is
            // stale on disk; the next project flush recreates the
            // file. Per-headline deletion (a TODO removed from a
            // file that still exists) already round-trips via
            // `diff_and_apply`'s "in DB but not in parsed → delete"
            // branch and is unaffected.
            trace!(
                path = %path.display(),
                "vault watcher: file removed; tasks retained, toast surfaced"
            );
            if let Some(tx) = &self.events_tx {
                let _ = tx.send(VaultEvent::FileRemoved {
                    source: path.to_path_buf(),
                });
            }
            return Ok(());
        }
        let parsed = match parse_org_file_with_meta(path) {
            Ok(f) => f,
            Err(e) => {
                // Per spec §7.3.3 rule 5: parse failure pauses
                // sync for this file, DB version preserved. Toast
                // once per pause transition; repeated bad saves
                // stay silent until the file parses again.
                let already_paused = self.mark_paused(path);
                if !already_paused {
                    warn!(
                        path = %path.display(),
                        error = %e,
                        "vault watcher: parse failed; pausing sync for this file"
                    );
                    if let Some(tx) = &self.events_tx {
                        let _ = tx.send(VaultEvent::ParseFailed {
                            source: path.to_path_buf(),
                            error: e.to_string(),
                        });
                    }
                } else {
                    trace!(
                        path = %path.display(),
                        "vault watcher: still paused (parse still failing)"
                    );
                }
                return Ok(());
            }
        };
        // Clean parse: if this file was paused, it's back. Surface
        // the recovery before applying the diff so the user sees
        // the toast pair (Failed then Recovered).
        if self.clear_paused(path)
            && let Some(tx) = &self.events_tx
        {
            let _ = tx.send(VaultEvent::ParseRecovered {
                source: path.to_path_buf(),
            });
        }
        // RRULE divergence detection (spec §7.3.3 rule 3): a
        // headline whose SCHEDULED cookie doesn't match its own
        // `:RRULE:` property means the user edited the cookie
        // alone in Emacs. `:RRULE:` is canonical; we surface the
        // divergence and rewrite the file so the cookie matches.
        // The check runs before diff_and_apply so the caller can
        // post-flush the rewrite.
        let divergences = collect_rrule_divergences(&parsed.headlines);
        for d in &divergences {
            warn!(
                path = %path.display(),
                title = %d.title,
                cookie = %d.cookie,
                rrule = %d.rrule,
                "vault watcher: SCHEDULED cookie disagrees with :RRULE:; rewriting file"
            );
            if let Some(tx) = &self.events_tx {
                let _ = tx.send(VaultEvent::RruleDiverged {
                    source: path.to_path_buf(),
                    title: d.title.clone(),
                    cookie: d.cookie.clone(),
                    rrule: d.rrule.clone(),
                });
            }
        }
        let project_id_opt = self.diff_and_apply(path, parsed).await?;
        // If we found divergences, rewrite the file. The writer's
        // scheduled_repeater_from_task projects the canonical
        // `:RRULE:` back to the right cookie, so the file becomes
        // self-consistent. RecentWrites swallows the resulting
        // inotify echo.
        if !divergences.is_empty()
            && let Some(project_id) = project_id_opt
        {
            self.rewrite_project_file(project_id);
        }
        Ok(())
    }

    /// Synchronously rewrite a project's vault file from DB state.
    /// Bypasses the writer's debounce — divergence detection wants
    /// the file fixed immediately, not at the next 100 ms tick.
    /// Failures log but don't propagate; the next normal flush
    /// will recover.
    fn rewrite_project_file(&self, project_id: i64) {
        let result = self.pool.with(|conn| {
            crate::org::write_project_to_vault(conn, &self.root, project_id)
                .map_err(|e| DbError::Sync(e.to_string()))
        });
        match result {
            Ok(summary) => {
                if let Ok(mut rw) = self.recent_writes.write() {
                    let _ = rw.record(summary.file_path);
                }
                trace!(
                    project_id,
                    "vault watcher: rewrote file to fix RRULE divergence"
                );
            }
            Err(e) => {
                warn!(
                    project_id,
                    error = %e,
                    "vault watcher: divergence-fix rewrite failed"
                );
            }
        }
    }

    /// Mark `path` as paused; returns `true` if it was already in
    /// the set (so the caller knows whether to suppress the
    /// `ParseFailed` toast).
    fn mark_paused(&self, path: &Path) -> bool {
        let mut paused = self.paused.lock().unwrap();
        !paused.insert(path.to_path_buf())
    }

    /// Drop `path` from the paused set. Returns `true` when it was
    /// previously paused (so the caller emits `ParseRecovered`).
    fn clear_paused(&self, path: &Path) -> bool {
        let mut paused = self.paused.lock().unwrap();
        paused.remove(path)
    }

    async fn diff_and_apply(&self, path: &Path, parsed: OrgFile) -> Result<Option<i64>, DbError> {
        // Resolve or create the project this file maps to.
        let project_id = self.resolve_or_create_project(path, &parsed).await?;

        // Snapshot current DB state for this project + the global
        // tag map so we can diff tag sets per task.
        let (db_tasks, db_tag_names) = self.pool.with(|conn| {
            let tasks = atrium_core::db::read::list_all_in_project(conn, project_id)?;
            let tag_names = atrium_core::db::read::tag_names_per_task(conn)?;
            Ok::<_, DbError>((tasks, tag_names))
        })?;

        let db_by_uuid: HashMap<String, &Task> =
            db_tasks.iter().map(|t| (t.uuid.clone(), t)).collect();

        // Flatten the parsed headline tree into a list of
        // (uuid, parent_uuid, depth, OrgTask). Headlines without
        // `:ID:` get a freshly-minted UUIDv4 here so the create
        // path can pass it through `NewTask.uuid`. The writer
        // (triggered by the worker after the create commits) will
        // rewrite the file with the now-stable :ID: property.
        let flat = flatten_with_uuids(&parsed.headlines);
        let parsed_uuids: HashSet<String> = flat.iter().map(|p| p.uuid.clone()).collect();

        // Deletes: DB tasks not in parsed.
        for task in &db_tasks {
            if !parsed_uuids.contains(&task.uuid) {
                self.handle.delete_task(task.id).await?;
            }
        }

        // Creates and updates. Process top-level tasks first so
        // children can reference their parent's freshly-created id.
        let mut uuid_to_task_id: HashMap<String, i64> =
            db_by_uuid.iter().map(|(u, t)| (u.clone(), t.id)).collect();
        for parsed_task in &flat {
            let parent_id = parsed_task
                .parent_uuid
                .as_ref()
                .and_then(|u| uuid_to_task_id.get(u).copied());
            let new_id = match db_by_uuid.get(&parsed_task.uuid) {
                None => {
                    let new = self
                        .handle
                        .create_task(parsed_task.to_new_task(project_id, parent_id))
                        .await?;
                    new.id
                }
                Some(existing) => {
                    let existing_tags = db_tag_names.get(&existing.id).cloned().unwrap_or_default();
                    if let Some(update) = parsed_task.diff_from(existing) {
                        self.handle.update_task(update).await?;
                    }
                    if !same_tag_set(&parsed_task.org.tags, &existing_tags) {
                        self.apply_tag_set(existing.id, &parsed_task.org.tags)
                            .await?;
                    }
                    existing.id
                }
            };
            uuid_to_task_id.insert(parsed_task.uuid.clone(), new_id);
        }

        Ok(Some(project_id))
    }

    async fn resolve_or_create_project(
        &self,
        path: &Path,
        parsed: &OrgFile,
    ) -> Result<i64, DbError> {
        // Look up the project by the file-level :ID: property.
        let id_from_file = parsed.file_properties.get(PROJECT_ID_PROPERTY).cloned();

        if let Some(uuid) = id_from_file.clone() {
            let found = self.pool.with(|conn| project_id_for_uuid(conn, &uuid))?;
            if let Some(project_id) = found {
                self.maybe_update_project_metadata(project_id, parsed)
                    .await?;
                return Ok(project_id);
            }
        }

        // No matching project. Create one. Title comes from
        // #+TITLE: directive or, failing that, the filename stem.
        let title = parsed
            .directives
            .get("TITLE")
            .cloned()
            .or_else(|| {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| "Untitled".to_string());

        // Resolve area from the directory the file lives in
        // (`<vault>/<area>/<project>.org`). Files at the vault
        // root → unfiled.
        let area_id = self.resolve_area_for_path(path).await?;

        let new = NewProject {
            uuid: id_from_file,
            title,
            area_id,
            ..Default::default()
        };
        let project = self.handle.create_project(new).await?;
        Ok(project.id)
    }

    async fn maybe_update_project_metadata(
        &self,
        project_id: i64,
        parsed: &OrgFile,
    ) -> Result<(), DbError> {
        // Title-only sync today — `#+TITLE:` flows back when it
        // changes. The other file-level fields (`:SEQUENTIAL:`,
        // `:REVIEW_INTERVAL:`, `:LAST_REVIEWED:`, `:ARCHIVED:`)
        // round-trip on import, but we don't pick up their
        // mutations from external edits yet. roadmap.md §17
        // follow-up.
        let parsed_title = match parsed.directives.get("TITLE") {
            Some(t) => t.clone(),
            None => return Ok(()),
        };
        let existing = self
            .pool
            .with(|conn| atrium_core::db::read::project_by_id(conn, project_id))?;
        if let Some(p) = existing
            && p.title != parsed_title
        {
            let update = ProjectUpdate::new(project_id).title(parsed_title);
            self.handle.update_project(update).await?;
        }
        Ok(())
    }

    async fn resolve_area_for_path(&self, path: &Path) -> Result<Option<i64>, DbError> {
        let parent = match path.parent() {
            Some(p) => p,
            None => return Ok(None),
        };
        if parent == self.root {
            return Ok(None);
        }
        let area_name = match parent.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => return Ok(None),
        };
        // EnsureArea is idempotent (case-insensitive match-or-create).
        let area = self.handle.ensure_area(area_name).await?;
        Ok(Some(area.id))
    }

    async fn apply_tag_set(&self, task_id: i64, tag_names: &[String]) -> Result<(), DbError> {
        let mut tag_ids = Vec::with_capacity(tag_names.len());
        for name in tag_names {
            let tag = self.handle.ensure_tag(name.clone()).await?;
            tag_ids.push(tag.id);
        }
        self.handle.set_task_tags(task_id, tag_ids).await?;
        Ok(())
    }
}

fn project_id_for_uuid(conn: &rusqlite::Connection, uuid: &str) -> Result<Option<i64>, DbError> {
    let mut stmt = conn.prepare("SELECT id FROM project WHERE uuid = ?1 LIMIT 1")?;
    let mut rows = stmt.query([uuid])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}

fn same_tag_set(a: &[String], b: &[String]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let a_set: HashSet<&str> = a.iter().map(String::as_str).collect();
    b.iter().all(|x| a_set.contains(x.as_str()))
}

/// Flat representation of a parsed headline with the uuid we'll
/// use in the DB. Headlines without an `:ID:` property get a
/// freshly-minted UUIDv4 here; the worker's `notify_project_dirty`
/// after `create_task` triggers the writer to stamp the property
/// onto disk, and the self-write filter swallows the resulting
/// inotify event.
struct ParsedTask<'a> {
    uuid: String,
    parent_uuid: Option<String>,
    org: &'a OrgTask,
}

fn flatten_with_uuids(headlines: &[OrgTask]) -> Vec<ParsedTask<'_>> {
    let mut out = Vec::new();
    for h in headlines {
        flatten_one(h, None, &mut out);
    }
    out
}

fn flatten_one<'a>(task: &'a OrgTask, parent_uuid: Option<String>, out: &mut Vec<ParsedTask<'a>>) {
    // Headlines without a TODO keyword are project sub-headings:
    // organisational, not structural. They aren't tasks themselves,
    // but TODOs nested under them are real tasks and attach at the
    // same parent level as the heading. Mirrors the import path's
    // treatment in `org::import::import_task`.
    let Some(_) = task.keyword else {
        for child in &task.children {
            flatten_one(child, parent_uuid.clone(), out);
        }
        return;
    };
    let uuid = match task.properties.get("ID") {
        Some(id) if !id.is_empty() => id.clone(),
        _ => Uuid::new_v4().to_string(),
    };
    out.push(ParsedTask {
        uuid: uuid.clone(),
        parent_uuid,
        org: task,
    });
    for child in &task.children {
        flatten_one(child, Some(uuid.clone()), out);
    }
}

impl<'a> ParsedTask<'a> {
    fn to_new_task(&self, project_id: i64, parent_id: Option<i64>) -> NewTask {
        let scheduled_for = self.org.scheduled.map(ScheduledFor::Date);
        let completed_at = match self.org.keyword {
            Some(OrgKeyword::Done) | Some(OrgKeyword::Cancelled) => {
                self.org.closed.or_else(|| Some(chrono::Utc::now()))
            }
            _ => None,
        };
        // `:RRULE:` in the properties drawer is canonical per spec
        // §7.3.3 rule 3. The cookie is best-fit projection only;
        // we ignore it here and trust the property drawer.
        let repeat_rule = self.org.properties.get("RRULE").cloned();
        NewTask {
            uuid: Some(self.uuid.clone()),
            title: self.org.title.clone(),
            project_id: Some(project_id),
            parent_id,
            scheduled_for,
            deadline: self.org.deadline,
            completed_at,
            orig_keyword: org_keyword_to_orig(self.org.keyword.as_ref()),
            repeat_rule,
            note: self.org.body.clone(),
            ..Default::default()
        }
    }

    /// Returns `Some(TaskUpdate)` if any field in the parsed task
    /// disagrees with `existing`. Returns `None` when no field
    /// differs (saves a worker round-trip).
    fn diff_from(&self, existing: &Task) -> Option<TaskUpdate> {
        let mut update = TaskUpdate::new(existing.id);
        let mut dirty = false;

        if self.org.title != existing.title {
            update = update.title(self.org.title.clone());
            dirty = true;
        }

        let parsed_scheduled = self.org.scheduled.map(ScheduledFor::Date);
        if parsed_scheduled != existing.scheduled_for {
            update = update.schedule(parsed_scheduled);
            dirty = true;
        }

        if self.org.deadline != existing.deadline {
            update = update.deadline_value(self.org.deadline);
            dirty = true;
        }

        // Completion: TODO/DONE/CANCELLED → completed_at. Diff the
        // scalar (Option<DateTime<Utc>>) so we don't round-trip on
        // identical values.
        let parsed_completed = match self.org.keyword {
            Some(OrgKeyword::Done) | Some(OrgKeyword::Cancelled) => self.org.closed,
            _ => None,
        };
        if parsed_completed != existing.completed_at {
            update = update.completed_at(parsed_completed);
            dirty = true;
        }

        // Custom keyword (WAITING / IN-PROGRESS / etc.). Spec
        // §7.3.3 rule 1 — the original keyword survives the
        // round-trip via `task.orig_keyword`. The watcher used
        // to drop OrgKeyword::Custom on its create path entirely
        // and never sync it on existing rows; v0.10.2 fixes both.
        let parsed_orig = org_keyword_to_orig(self.org.keyword.as_ref());
        if parsed_orig != existing.orig_keyword {
            update = update.orig_keyword(parsed_orig);
            dirty = true;
        }

        // `:RRULE:` (canonical RRULE per spec §7.3.3 rule 3). The
        // SCHEDULED cookie is best-fit projection — divergence
        // detection at the file level is a separate concern (see
        // detect_rrule_divergences in the watcher).
        let parsed_rrule = self.org.properties.get("RRULE").cloned();
        if parsed_rrule != existing.repeat_rule {
            update = update.repeat_rule_value(parsed_rrule);
            dirty = true;
        }

        if dirty { Some(update) } else { None }
    }
}

/// Map a parsed Org keyword to the value `task.orig_keyword`
/// should hold. Custom keywords (anything outside TODO / DONE)
/// stash their name verbatim; CANCELLED is pinned to the literal
/// "CANCELLED" so the writer can recover the keyword on emit
/// (Atrium's domain only knows two completion states; the
/// orig_keyword column carries the original label). Plain TODO /
/// DONE map to `None` — the column's default.
fn org_keyword_to_orig(keyword: Option<&OrgKeyword>) -> Option<String> {
    match keyword {
        Some(OrgKeyword::Custom(name)) => Some(name.clone()),
        Some(OrgKeyword::Cancelled) => Some("CANCELLED".to_string()),
        _ => None,
    }
}

/// Spawn a vault watcher task on the current tokio runtime. The
/// returned `JoinHandle` is detached unless the caller holds it —
/// drop the handle to let the watcher run for the runtime's
/// lifetime, or `await` it for clean shutdown in tests.
pub fn spawn_vault_watcher(
    root: PathBuf,
    handle: WorkerHandle,
    pool: ReadPool,
    recent_writes: Arc<RwLock<RecentWrites>>,
) -> Result<tokio::task::JoinHandle<()>, notify::Error> {
    spawn_vault_watcher_with_events(root, handle, pool, recent_writes, None)
}

/// Variant that wires an optional [`VaultEvent`] sender. `None`
/// keeps the watcher's prior log-only behaviour; `Some(tx)` lets
/// the watcher surface [`VaultEvent::ParseFailed`] notices to the
/// caller for toast surfacing.
pub fn spawn_vault_watcher_with_events(
    root: PathBuf,
    handle: WorkerHandle,
    pool: ReadPool,
    recent_writes: Arc<RwLock<RecentWrites>>,
    events_tx: Option<mpsc::UnboundedSender<VaultEvent>>,
) -> Result<tokio::task::JoinHandle<()>, notify::Error> {
    let (tx, rx) = mpsc::unbounded_channel();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| match res {
        Ok(event) => {
            // Unbounded; can't fail unless the receiver dropped.
            let _ = tx.send(event);
        }
        Err(e) => {
            warn!(error = %e, "vault watcher: notify error");
        }
    })?;
    watcher.watch(&root, RecursiveMode::Recursive)?;

    let task = VaultWatcher {
        root,
        handle,
        pool,
        recent_writes,
        events_tx,
        rx,
        pending: HashMap::new(),
        paused: Arc::new(Mutex::new(HashSet::new())),
        _watcher: watcher,
    };
    Ok(tokio::spawn(task.run()))
}

/// One headline's cookie-vs-RRULE disagreement, captured for the
/// `RruleDiverged` event.
struct RruleDivergence {
    title: String,
    cookie: String,
    rrule: String,
}

/// Walk a parsed headline tree and find every task whose
/// SCHEDULED cookie disagrees with its `:RRULE:` property on the
/// FREQ + INTERVAL axis. BY-clauses in `:RRULE:` don't count as
/// divergence — the cookie can't express them by design (spec
/// §7.3.3 rule 3 explicitly allows the lossy projection).
fn collect_rrule_divergences(headlines: &[OrgTask]) -> Vec<RruleDivergence> {
    let mut out = Vec::new();
    for h in headlines {
        collect_rrule_divergences_one(h, &mut out);
    }
    out
}

fn collect_rrule_divergences_one(task: &OrgTask, out: &mut Vec<RruleDivergence>) {
    if let (Some(repeater), Some(rrule)) = (
        task.scheduled_repeater.as_ref(),
        task.properties.get("RRULE"),
    ) && !crate::rrule_cookie::cookie_matches_rrule(repeater, rrule)
    {
        out.push(RruleDivergence {
            title: task.title.clone(),
            cookie: format!("{}{}{}", repeater.mode, repeater.interval, repeater.unit),
            rrule: rrule.clone(),
        });
    }
    for child in &task.children {
        collect_rrule_divergences_one(child, out);
    }
}
