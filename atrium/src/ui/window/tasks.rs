// SPDX-License-Identifier: MIT
//! `AtriumWindow`: task mutations (toggle / rename / reorder / create / delete), selection, inspector open.
//! Extracted from window/mod.rs in v0.22.0 split (Pass 3).

use super::*;

impl AtriumWindow {
    /// Toggle handler — fires the worker call. The worker emits a
    /// `TaskChanges` delta which the bridge applies; we don't update
    /// the model here.
    pub(super) fn handle_toggle(&self, id: i64, want_completed: bool) {
        let Some(worker) = self.worker() else {
            warn!("worker not attached; toggle ignored");
            return;
        };
        let win_weak = self.downgrade();
        glib::MainContext::default().spawn_local(async move {
            match worker.toggle_complete(id).await {
                Ok(task) => {
                    let Some(win) = win_weak.upgrade() else {
                        return;
                    };
                    let message = if task.is_completed() {
                        format!("“{}” completed", truncate(&task.title, 40))
                    } else {
                        format!("“{}” reopened", truncate(&task.title, 40))
                    };
                    let worker_for_undo = worker.clone();
                    win.show_undo_toast(&message, move || {
                        let worker = worker_for_undo;
                        glib::MainContext::default().spawn_local(async move {
                            if let Err(e) = worker.toggle_complete(id).await {
                                error!(?e, id, "undo toggle_complete failed");
                            }
                        });
                    });
                    let _ = want_completed;
                }
                Err(e) => error!(?e, id, "toggle_complete failed"),
            }
        });
    }

    /// Rename handler — fires `update_task` with the new title.
    ///
    /// v0.13 (Slice 1) routes the new title through
    /// [`atrium_inline::parse`] so an inline rename can
    /// pick up the same `#tag` / `@today` / `@deadline` syntax the
    /// bottom-of-list entry and Quick Entry modal already speak.
    /// Plain-text titles take a fast path identical to the
    /// pre-v0.13 single-update behaviour.
    ///
    /// Semantics for the extended path:
    ///
    /// - `parsed.title` becomes the new title (with the inline
    ///   tokens stripped out).
    /// - `parsed.tag_names` are *added* to the existing tag set
    ///   (rename never removes a tag — the user can't see the
    ///   existing tags from the rename surface, so a destructive
    ///   merge would surprise them; the tag editor and Inspector
    ///   are the channels for tag removal).
    /// - `parsed.scheduled_for` and `parsed.deadline` *set* the
    ///   corresponding fields when present. They never clear.
    /// - An empty title after parsing (the user typed only
    ///   `#tag`) is rejected so the row doesn't lose its name.
    pub(super) fn handle_rename(&self, id: i64, new_title: String) {
        let Some(worker) = self.worker() else {
            warn!("worker not attached; rename ignored");
            return;
        };

        let parsed = atrium_inline::parse(&new_title);

        // Fast path — no inline syntax. Behaves identically to the
        // pre-v0.13 single-update flow so renames of plain text
        // can't regress.
        if parsed.is_plain_title() {
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker
                    .update_task(TaskUpdate::new(id).title(new_title))
                    .await
                {
                    error!(?e, id, "update_task failed");
                }
            });
            return;
        }

        // Extended path — apply the parsed scalars + merge tags.
        if parsed.title.trim().is_empty() {
            warn!(
                id,
                "inline rename produced an empty title; ignored to keep the row named"
            );
            return;
        }

        let Some(pool) = self.read_pool() else {
            warn!("read pool not attached; inline rename ignored");
            return;
        };
        let existing_tag_ids = pool
            .with(|c| atrium_core::db::read::tag_ids_for_task(c, id))
            .unwrap_or_default();
        // When the user typed `!N`, we need to swap any stale
        // `priority-*` tag for the new one (single-valued field
        // pretending to be a tag). Pull the names of the existing
        // tags now so the async block can filter by them without a
        // second read.
        let priority_override = parsed.priority;
        let stale_priority_ids: Vec<i64> = if priority_override.is_some() {
            pool.with(atrium_core::db::read::list_tags)
                .unwrap_or_default()
                .into_iter()
                .filter(|t| atrium_inline::is_priority_tag_name(&t.name))
                .map(|t| t.id)
                .collect()
        } else {
            Vec::new()
        };

        glib::MainContext::default().spawn_local(async move {
            // Single update for title + scheduled + deadline so the
            // listener side sees one notify event per scalar field
            // rather than three sequential updates.
            let mut update = TaskUpdate::new(id).title(parsed.title.clone());
            if let Some(sched) = parsed.scheduled_for {
                update = update.schedule(Some(sched));
            }
            if let Some(due) = parsed.deadline {
                update = update.deadline_value(Some(due));
            }
            if let Err(e) = worker.update_task(update).await {
                error!(?e, id, "update_task (inline rename) failed");
                return;
            }

            // Tag merge — start from the existing set, drop any
            // stale `priority-*` when the user typed `!N`, then
            // append the parsed free-form tags + the priority
            // projection. Free-form tags are never removed by a
            // rename (the rename surface doesn't show them).
            let mut merged: Vec<i64> = existing_tag_ids
                .into_iter()
                .filter(|tid| !stale_priority_ids.contains(tid))
                .collect();
            for name in &parsed.tag_names {
                match worker.ensure_tag(name.clone()).await {
                    Ok(tag) => {
                        if !merged.contains(&tag.id) {
                            merged.push(tag.id);
                        }
                    }
                    Err(e) => {
                        error!(?e, ?name, id, "ensure_tag failed during inline rename");
                        return;
                    }
                }
            }
            if let Some(level) = priority_override {
                let proj = format!("priority-{level}");
                match worker.ensure_tag(proj.clone()).await {
                    Ok(tag) => {
                        if !merged.contains(&tag.id) {
                            merged.push(tag.id);
                        }
                    }
                    Err(e) => {
                        error!(?e, ?proj, id, "ensure_tag (priority) failed");
                        return;
                    }
                }
            }
            if let Err(e) = worker.set_task_tags(id, merged).await {
                error!(?e, id, "set_task_tags (inline rename) failed");
            }
        });
    }

    /// Reorder handler — drag-and-drop drops `src_id` onto `dest_id`.
    /// Computes a midpoint position so `src` ends up adjacent to
    /// `dest`, then fires a single `update_task` with the new
    /// position. Active store re-sorts via `apply_changes` after the
    /// worker round-trip.
    pub(super) fn handle_reorder(
        &self,
        src_id: i64,
        dest_id: i64,
        bias: crate::ui::task_list::DropBias,
    ) {
        tracing::debug!(src_id, dest_id, ?bias, active = ?self.active_list(), "reorder entry");
        if src_id == dest_id {
            tracing::debug!(src_id, "reorder: same id, ignoring");
            return;
        }
        // Drag-reorder is meaningful on every view whose ordering
        // derives from `task.position` — Inbox, project pages, area
        // pages, and the "manual" canonical lists (Anytime, Someday).
        // The original Phase-4 cut narrowed this to Inbox; v0.23.1
        // widens it because pick-it-up.md's "fails safe to reorder"
        // assumption requires reorder to actually work on project
        // pages (which is where the subtasks Shift-drag tests run).
        // Time-sorted views (Today, Upcoming, Forecast, Logbook) and
        // the read-only debug views stay out — reorder there has no
        // persistent meaning.
        if !matches!(
            self.active_list(),
            ActiveList::Inbox
                | ActiveList::Anytime
                | ActiveList::Someday
                | ActiveList::Project(_)
                | ActiveList::Area(_)
        ) {
            tracing::debug!(active = ?self.active_list(), "reorder: not a position-ordered view, ignoring");
            // Tier D — don't fail silently. On a date-sorted list the
            // order follows the dates, so a drag-to-reorder has no
            // persistent meaning; tell the user instead of swallowing
            // the drop. Only when it's a genuine reorder attempt (not a
            // task dropped back onto itself).
            if src_id != dest_id {
                self.show_toast(
                    "This list is sorted by date — drag to reorder isn't available here.",
                );
            }
            return;
        }

        let Some(store) = self.imp().store.borrow().clone() else {
            tracing::debug!("reorder: no store, ignoring");
            return;
        };

        // Snapshot positions for the math.
        let n = store.n_items();
        let mut entries: Vec<(u32, i64, f64)> = Vec::with_capacity(n as usize);
        for i in 0..n {
            if let Some(obj) = store
                .item(i)
                .and_downcast::<crate::ui::task_object::AtriumTask>()
            {
                entries.push((i, obj.id(), obj.position()));
            }
        }
        let src = entries.iter().find(|(_, id, _)| *id == src_id);
        let dest = entries.iter().find(|(_, id, _)| *id == dest_id);
        let (Some(&(_, _, _src_pos)), Some(&(dest_idx, _, dest_pos))) = (src, dest) else {
            tracing::debug!(
                src_id,
                dest_id,
                src_found = src.is_some(),
                dest_found = dest.is_some(),
                "reorder: src/dest not in store"
            );
            return;
        };

        // v0.23.1 — cursor's vertical position on the dest row decides
        // before / after. The bias was computed at drop time from the
        // row's `y` coordinate vs its height. Skip src itself when
        // looking up the neighbour so dragging onto an adjacent row's
        // far half lands the right way rather than snapping back.
        let neighbour_pos = |idx: u32| -> Option<f64> {
            entries
                .iter()
                .find(|(i, id, _)| *i == idx && *id != src_id)
                .map(|(_, _, p)| *p)
        };
        let new_position = match bias {
            crate::ui::task_list::DropBias::Above => {
                let prev_pos = if dest_idx == 0 {
                    dest_pos - 1.0
                } else {
                    neighbour_pos(dest_idx - 1).unwrap_or(dest_pos - 1.0)
                };
                (prev_pos + dest_pos) / 2.0
            }
            crate::ui::task_list::DropBias::Below => {
                let next_pos = neighbour_pos(dest_idx + 1).unwrap_or(dest_pos + 1.0);
                (dest_pos + next_pos) / 2.0
            }
        };

        let Some(worker) = self.worker() else {
            return;
        };
        glib::MainContext::default().spawn_local(async move {
            if let Err(e) = worker
                .update_task(TaskUpdate::new(src_id).position(new_position))
                .await
            {
                error!(?e, src_id, dest_id, "reorder update_task failed");
            }
        });
    }

    /// Subtasks (v0.23.0) — Shift+drop reparents: make `src_id` a child
    /// of `new_parent_id`. The worker enforces the same-project rule and
    /// rejects cycles; on rejection we surface a toast rather than fail
    /// silently. The resulting TaskChanges delta re-nests the list via
    /// `apply_nesting`.
    pub(super) fn handle_reparent(&self, src_id: i64, new_parent_id: i64) {
        if src_id == new_parent_id {
            return;
        }
        let Some(worker) = self.worker() else {
            return;
        };
        tracing::debug!(src_id, new_parent_id, "reparent dispatch");
        let win = self.downgrade();
        glib::MainContext::default().spawn_local(async move {
            if let Err(e) = worker
                .update_task(TaskUpdate::new(src_id).reparent(Some(new_parent_id)))
                .await
            {
                warn!(?e, src_id, new_parent_id, "reparent failed");
                if let Some(win) = win.upgrade() {
                    win.show_toast("Can't nest there (a different project or a cycle).");
                }
            }
        });
    }

    /// Create with the given title — fired by the bottom-of-list entry.
    /// Phase 6b: parses inline `#tag` / `@today` / `@yyyy-mm-dd` /
    /// `@deadline yyyy-mm-dd` syntax via `quickentry::parser` and
    /// applies the metadata to the new task.
    pub(super) fn create_task_with_title(&self, raw_input: String) {
        let Some(worker) = self.worker() else {
            warn!("worker not attached; new-task ignored");
            return;
        };
        let active = self.active_list();
        let parsed = atrium_inline::parse(&raw_input);
        let projected_tags = parsed.projected_tag_names();
        if parsed.title.is_empty() && projected_tags.is_empty() {
            return;
        }
        glib::MainContext::default().spawn_local(async move {
            let scheduled = parsed.scheduled_for.or({
                if matches!(active, ActiveList::Today) {
                    Some(atrium_core::ScheduledFor::Date(Local::now().date_naive()))
                } else {
                    None
                }
            });
            let project_id = match active {
                ActiveList::Project(id) => Some(id),
                _ => None,
            };
            let new = NewTask {
                title: parsed.title,
                project_id,
                scheduled_for: scheduled,
                deadline: parsed.deadline,
                ..NewTask::default()
            };
            match worker.create_task(new).await {
                Ok(task) => {
                    debug!(id = task.id, "task created");
                    if !projected_tags.is_empty() {
                        // Resolve tag names → ids, creating any new
                        // tags via `ensure_tag`. Run sequentially so
                        // we collect ids before SetTaskTags fires.
                        // `projected_tags` includes the v0.13 `!N`
                        // priority projection appended after the
                        // user's free-form `#tag` set.
                        let mut tag_ids = Vec::with_capacity(projected_tags.len());
                        for name in projected_tags {
                            match worker.ensure_tag(name).await {
                                Ok(t) => tag_ids.push(t.id),
                                Err(e) => warn!(?e, "ensure_tag failed; skipping"),
                            }
                        }
                        if !tag_ids.is_empty()
                            && let Err(e) = worker.set_task_tags(task.id, tag_ids).await
                        {
                            error!(?e, task_id = task.id, "set_task_tags failed");
                        }
                    }
                }
                Err(e) => error!(?e, "create_task failed"),
            }
        });
    }

    /// Delete handler — operates on the focused list row. Captures
    /// the full task state + tag attachments before delete so the
    /// undo toast can recreate the row. Cascade-deleted subtasks are
    /// lost (parent_id chains aren't recovered) — accepting that for
    /// v0.1; Phase 8 polish could capture the full subtree.
    pub fn delete_focused_task(&self) {
        let Some(id) = self.focused_task_id() else {
            return;
        };
        let Some(worker) = self.worker() else { return };
        let Some(pool) = self.read_pool() else { return };

        let task = match pool.with(|c| atrium_core::db::read::task_by_id(c, id)) {
            Ok(Some(t)) => t,
            _ => return,
        };
        let tag_ids = pool
            .with(|c| atrium_core::db::read::tag_ids_for_task(c, id))
            .unwrap_or_default();

        let win_weak = self.downgrade();
        glib::MainContext::default().spawn_local(async move {
            if let Err(e) = worker.delete_task(id).await {
                error!(?e, id, "delete_task failed");
                return;
            }
            let Some(win) = win_weak.upgrade() else {
                return;
            };
            let title = task.title.clone();
            let worker_for_undo = worker.clone();
            win.show_undo_toast(
                &format!("Deleted “{}”", truncate(&title, 40)),
                move || {
                    let worker = worker_for_undo;
                    let task = task.clone();
                    let tag_ids = tag_ids.clone();
                    glib::MainContext::default().spawn_local(async move {
                        let new = atrium_core::NewTask {
                            title: task.title,
                            note: task.note,
                            project_id: task.project_id,
                            parent_id: task.parent_id,
                            scheduled_for: task.scheduled_for,
                            deadline: task.deadline,
                            defer_until: task.defer_until,
                            estimated_minutes: task.estimated_minutes,
                            repeat_rule: task.repeat_rule,
                            repeat_mode: task.repeat_mode,
                            // Undo-restore creates a fresh row; let
                            // the worker generate a fresh UUID rather
                            // than reusing the deleted task's ID.
                            uuid: None,
                            // Preserve the orig_keyword from the
                            // pre-deletion row so an Org-imported
                            // custom-keyword task survives an Atrium
                            // delete/undo cycle without losing its
                            // round-trip anchor.
                            orig_keyword: task.orig_keyword,
                            // Preserve completion state on undo —
                            // restoring a deleted DONE task should
                            // come back DONE with its original
                            // completion timestamp, not flip to TODO.
                            completed_at: task.completed_at,
                            // Preserve the per-task warning window so
                            // a sensitive deadline keeps its early
                            // surfacing across the delete/undo cycle.
                            deadline_warn_days: task.deadline_warn_days,
                            // Preserve the time-of-day on schedule
                            // across the undo cycle.
                            scheduled_time: task.scheduled_time,
                            // Preserve any pending reminder so undo
                            // restores the full task state.
                            reminder_at: task.reminder_at,
                            // v0.24.0 — preserve custom property-
                            // drawer extras across the delete/undo
                            // cycle so an Org-imported task with
                            // `:CLIENT: Acme` keeps its drawer.
                            extra_properties: task.extra_properties,
                        };
                        match worker.create_task(new).await {
                            Ok(restored) => {
                                if !tag_ids.is_empty()
                                    && let Err(e) = worker.set_task_tags(restored.id, tag_ids).await
                                {
                                    error!(?e, "undo set_task_tags failed");
                                }
                            }
                            Err(e) => error!(?e, "undo create_task failed"),
                        }
                    });
                },
            );
        });
    }

    /// Toggle complete on the focused row (Space keybinding).
    pub fn toggle_focused_task(&self) {
        let Some(id) = self.focused_task_id() else {
            return;
        };
        let Some(worker) = self.worker() else { return };
        glib::MainContext::default().spawn_local(async move {
            if let Err(e) = worker.toggle_complete(id).await {
                error!(?e, id, "toggle_complete failed");
            }
        });
    }

    pub(super) fn focused_task_id(&self) -> Option<i64> {
        self.selected_task_ids().first().copied()
    }

    /// All task ids currently selected in the active list. Order
    /// matches the model (low index → high index).
    pub(super) fn selected_task_ids(&self) -> Vec<i64> {
        let Some(model) = self.imp().task_list_view.model() else {
            return Vec::new();
        };
        let Some(selection) = model.downcast_ref::<gtk::MultiSelection>() else {
            return Vec::new();
        };
        let bitset = selection.selection();
        let mut out = Vec::new();
        if let Some((mut iter, first)) = gtk::BitsetIter::init_first(&bitset) {
            let mut pos = first;
            loop {
                if let Some(obj) = selection.item(pos)
                    && let Some(t) = obj.downcast_ref::<crate::ui::task_object::AtriumTask>()
                {
                    out.push(t.id());
                }
                match iter.next() {
                    Some(next_pos) => pos = next_pos,
                    None => break,
                }
            }
        }
        out
    }

    /// v0.1.8 — bulk-action toolbar reveals only when ≥ 2 rows
    /// are selected. Single-row selection has the row's own
    /// highlight as feedback, the per-row checkbox for completion,
    /// the Delete key for deletion, and Ctrl+I for the editor —
    /// the toolbar buttons would just be redundant copies of those.
    /// The toolbar earns its keep when bulk ops are actually
    /// available, i.e. when there's something to bulk-act on.
    pub(super) fn update_selection_bar(&self, n: i64) {
        let revealer = self.imp().selection_revealer.clone();
        let label = self.imp().selection_label.clone();
        if n < 2 {
            revealer.set_reveal_child(false);
        } else {
            label.set_label(&format!("{n} selected"));
            revealer.set_reveal_child(true);
        }
    }

    /// Bulk handlers — fire individual worker calls in a loop. We
    /// suppress per-item undo toasts to avoid spamming the overlay
    /// with N toasts; bulk-undo as a single coalesced operation is a
    /// Phase 8 polish item.
    pub fn bulk_complete_selection(&self) {
        let ids = self.selected_task_ids();
        if ids.is_empty() {
            return;
        }
        let Some(worker) = self.worker() else {
            return;
        };
        glib::MainContext::default().spawn_local(async move {
            for id in ids {
                if let Err(e) = worker.toggle_complete(id).await {
                    error!(?e, id, "bulk toggle_complete failed");
                }
            }
        });
        self.clear_selection();
    }

    pub fn bulk_delete_selection(&self) {
        let ids = self.selected_task_ids();
        if ids.is_empty() {
            return;
        }
        let Some(worker) = self.worker() else {
            return;
        };
        let count = ids.len();
        let win_weak = self.downgrade();
        glib::MainContext::default().spawn_local(async move {
            let mut deleted = 0;
            for id in ids {
                if let Err(e) = worker.delete_task(id).await {
                    error!(?e, id, "bulk delete_task failed");
                } else {
                    deleted += 1;
                }
            }
            if let Some(win) = win_weak.upgrade() {
                let toast = adw::Toast::new(&format!(
                    "{deleted} of {count} task{} deleted",
                    if count == 1 { "" } else { "s" }
                ));
                toast.set_timeout(4);
                win.imp().toast_overlay.add_toast(toast);
            }
        });
        self.clear_selection();
    }

    pub fn clear_selection(&self) {
        let Some(model) = self.imp().task_list_view.model() else {
            return;
        };
        if let Some(sel) = model.downcast_ref::<gtk::MultiSelection>() {
            sel.unselect_all();
        }
    }

    pub fn select_all_visible(&self) {
        let Some(model) = self.imp().task_list_view.model() else {
            return;
        };
        if let Some(sel) = model.downcast_ref::<gtk::MultiSelection>() {
            sel.select_all();
        }
    }

    /// Open the per-task editor for `task_id`. Mode-aware: Simple
    /// Mode opens the Phase 7i modal dialog; Builder Mode routes
    /// through the always-visible side pane (re-populating it if
    /// the requested task isn't the one currently shown) and
    /// hands keyboard focus to the title row.
    ///
    /// All three editor entry points fan in here:
    /// - `Ctrl+I` (`win.edit-details-focused` → `open_inspector_focused` →
    ///   `open_inspector_for(focused_id)`),
    /// - per-row double-click gesture (`task_list.rs` →
    ///   `win.edit-details-for(i64)` → `open_inspector_for(id)`),
    /// - right-click → *Edit Details…* (same `win.edit-details-for`
    ///   action target).
    ///
    /// The v0.1.1 design call had `Ctrl+I` no-op in Builder Mode
    /// on the rationale "the side pane already shows the editor."
    /// That was wrong: the user's mental model of Ctrl+I is *get
    /// me into the editor for this task*; doing nothing makes the
    /// chord feel broken. v0.1.4 retracts the no-op.
    pub fn open_inspector_for(&self, task_id: i64) {
        let Some(pool) = self.read_pool() else {
            return;
        };
        let Some(worker) = self.worker() else {
            return;
        };
        let task = match pool.with(|conn| atrium_core::db::read::task_by_id(conn, task_id)) {
            Ok(Some(t)) => t,
            Ok(None) => {
                error!(task_id, "inspector: task not found");
                return;
            }
            Err(e) => {
                error!(?e, task_id, "inspector: task_by_id failed");
                return;
            }
        };
        let projects = pool
            .with(atrium_core::db::read::list_projects)
            .unwrap_or_default();
        let tag_count = pool
            .with(|conn| atrium_core::db::read::tag_ids_for_task(conn, task_id))
            .unwrap_or_default()
            .len();
        // v0.17.0 — pre-load clock entries for the Inspector
        // Time group. Newest-first by started_at; the inspector
        // computes the running state + total + log directly from
        // this Vec.
        let clock_entries = pool
            .with(|conn| atrium_core::db::read::list_clock_entries(conn, task_id))
            .unwrap_or_default();

        // Builder Mode — route through the side pane. Repopulate
        // if the pane isn't already showing this task (e.g., the
        // user right-clicked a row that wasn't selected; the
        // selection-changed signal hasn't fired yet so the pane
        // still shows the previously-selected row). Either way,
        // grab keyboard focus on the title.
        let builder = self.imp().current_mode_is_builder.get();
        if builder && let Some(pane) = self.imp().inspector_pane.borrow().clone() {
            if pane.current_task_id() != Some(task_id) {
                pane.set_task(task, projects, tag_count, clock_entries);
            }
            pane.focus_title();
            return;
        }

        // Simple Mode (and any path where the pane isn't up yet)
        // — open the modal dialog.
        let win_weak = self.downgrade();
        let on_edit_tags = move |id: i64| {
            if let Some(win) = win_weak.upgrade() {
                win.open_tag_editor_for(id);
            }
        };
        let win_weak_for_navigate = self.downgrade();
        let on_navigate_uuid = move |uuid: String| {
            // v0.19.0 — Phase 18.5 Tier-2 Org-link click in Simple
            // Mode. The dialog closes itself before this fires;
            // we just resolve UUID → id and re-open the inspector
            // for the linked task.
            let Some(win) = win_weak_for_navigate.upgrade() else {
                return;
            };
            let Some(pool) = win.read_pool() else {
                return;
            };
            let task_id = pool
                .with(|conn| atrium_core::db::read::task_id_for_uuid(conn, &uuid))
                .ok()
                .flatten();
            if let Some(id) = task_id {
                win.open_inspector_for(id);
            }
        };
        crate::ui::inspector::open(
            self,
            worker,
            task,
            projects,
            tag_count,
            on_edit_tags,
            on_navigate_uuid,
        );
    }

    /// `Ctrl+I` shortcut entry point — operates on the focused /
    /// first-selected task. The mode-specific routing lives in
    /// `open_inspector_for`; this is just the focus-resolver wrapper.
    pub fn open_inspector_focused(&self) {
        if let Some(id) = self.focused_task_id() {
            self.open_inspector_for(id);
        }
    }
}
