// SPDX-License-Identifier: MIT
//! `AtriumWindow`: list-view + data-layer wiring, mode, project extras, accessors, resolvers.
//! Extracted from window/mod.rs in v0.22.0 split (Pass 3).

use super::*;

impl AtriumWindow {
    pub(super) fn init_list_view(&self) {
        let store = gio::ListStore::new::<crate::ui::task_object::AtriumTask>();
        self.imp().store.replace(Some(store.clone()));

        // Phase 7c — MultiSelection enables Ctrl+Click toggle,
        // Shift+Click range, and `Ctrl+A` Select All out of the box.
        // Single-row interactions (Space toggle, Delete) still work
        // because `selected_task_ids` returns the first item when
        // exactly one is selected.
        let selection = gtk::MultiSelection::new(Some(store.clone()));
        self.imp().task_list_view.set_model(Some(&selection));

        // Show / hide the bulk action bar as the selection size changes.
        // Phase 10 — also drives the Inspector side pane in Builder
        // Mode: a single-row selection populates the editor; zero or
        // multiple rows show the empty-state placeholder.
        let win_weak = self.downgrade();
        selection.connect_selection_changed(move |sel, _, _| {
            let Some(win) = win_weak.upgrade() else {
                return;
            };
            let n = sel.selection().size();
            win.update_selection_bar(n as i64);
            win.refresh_inspector_pane();
        });

        // Factory wires interactions back into the window via weak
        // refs so handlers don't extend the window's lifetime.
        let win_weak = self.downgrade();
        let on_toggle = move |id: i64, want_completed: bool| {
            let Some(win) = win_weak.upgrade() else {
                return;
            };
            win.handle_toggle(id, want_completed);
        };
        let win_weak2 = self.downgrade();
        let on_rename = move |id: i64, new_title: String| {
            let Some(win) = win_weak2.upgrade() else {
                return;
            };
            win.handle_rename(id, new_title);
        };
        let win_weak3 = self.downgrade();
        let on_reorder = move |src_id: i64, dest_id: i64| {
            let Some(win) = win_weak3.upgrade() else {
                return;
            };
            win.handle_reorder(src_id, dest_id);
        };
        // Subtasks (v0.23.0) — Shift+drop makes the dropped task a child
        // of the drop target; plain drop reorders (above).
        let win_weak_rp = self.downgrade();
        let on_reparent = move |src_id: i64, new_parent_id: i64| {
            let Some(win) = win_weak_rp.upgrade() else {
                return;
            };
            win.handle_reparent(src_id, new_parent_id);
        };
        let win_weak4 = self.downgrade();
        let pool_source = move || {
            win_weak4
                .upgrade()
                .and_then(|w| w.imp().read_pool.get().cloned())
        };
        let factory = build_factory(on_toggle, on_rename, on_reorder, on_reparent, pool_source);
        self.imp().task_list_view.set_factory(Some(&factory));

        // v0.1.15 — listen to GtkListView::activate as the canonical
        // double-click signal. The per-row Capture-phase gesture in
        // `build_factory` works for slow double-clicks (clicks
        // outside `gtk-double-click-time`), but for *fast* doubles
        // GtkListView's internal click gesture claims the event
        // sequence to fire its own `activate` signal, which prevents
        // our row-level gesture from seeing the second release.
        // Listening here covers exactly that case.
        //
        // The handler defers to an idle callback for the same reason
        // the row-level gesture does: GtkListView's selection focus
        // dance has to settle before we grab focus on the entry, or
        // our grab gets undone immediately.
        let win_weak_for_activate = self.downgrade();
        self.imp()
            .task_list_view
            .connect_activate(move |_lv, _pos| {
                tracing::debug!("list_view activate signal");
                let Some(win) = win_weak_for_activate.upgrade() else {
                    return;
                };
                glib::idle_add_local_once(move || {
                    let did_edit = win.start_edit_focused_row();
                    tracing::debug!(
                        did_edit,
                        "list_view activate: start_edit_focused_row (idle)"
                    );
                });
            });

        // (Phase 7j note: relying on `connect_activate` *alone* was
        // unreliable when the row's title was a `GtkEditableLabel`
        // that hijacked double-clicks. v0.0.37 replaced that with a
        // `GtkStack(Label/Entry)` setup, so `activate` is now safe
        // to listen to. Per-row gesture stays in place to handle
        // double-clicks slower than `gtk-double-click-time`.)

        // Phase 7h — list-scoped chords. `Space` (toggle complete),
        // `Delete` (delete focused task), and `Ctrl+A` (select all)
        // used to be window-global accels, which meant typing a
        // space in any GtkEntry on the surface (Quick Entry,
        // bottom-of-list new-task entry, search bar, sidebar
        // filter, tag editor, …) ran toggle-complete instead of
        // inserting the space character. Scoping the controller to
        // the task list with `ShortcutScope::Managed` fires the
        // shortcuts only when focus is on the list or one of its
        // descendant rows; entries elsewhere see the keys
        // unmodified and do their normal text input.
        let list_shortcuts = gtk::ShortcutController::new();
        list_shortcuts.set_scope(gtk::ShortcutScope::Managed);
        for (chord, action_name) in [
            ("space", "win.toggle-complete"),
            ("Delete", "win.delete-task"),
            ("<Primary>a", "win.select-all"),
            // v0.0.37 — Esc was a window-global accel for
            // `win.bulk-clear`, which meant typing in the
            // bottom-of-list new-task entry and hitting Esc
            // silently cleared the multi-selection. Scoping it to
            // the list lets entries (Quick Entry, search bar,
            // sidebar filter, tag editor, new-task) keep their own
            // Esc semantics.
            ("Escape", "win.bulk-clear"),
        ] {
            if let Some(trigger) = gtk::ShortcutTrigger::parse_string(chord) {
                let action = gtk::NamedAction::new(action_name);
                let shortcut = gtk::Shortcut::new(Some(trigger), Some(action));
                list_shortcuts.add_shortcut(shortcut);
            }
        }
        self.imp().task_list_view.add_controller(list_shortcuts);
    }

    /// Push the worker handle / read pool into the window after the
    /// data layer boots.
    /// v0.20.0 — Phase 19.5. Stash the reminder service handle
    /// so the TaskChanges bridge can wake it on every batch.
    pub fn attach_reminder_service(&self, service: crate::reminders::ReminderService) {
        *self.imp().reminder_service.borrow_mut() = Some(service);
    }

    /// v0.20.0 — Phase 19.5. Wake the reminder service so it
    /// re-queries `next_pending_reminder`. No-op when the
    /// service hasn't been attached yet (early-boot races).
    pub fn wake_reminder_service(&self) {
        if let Some(svc) = self.imp().reminder_service.borrow().as_ref() {
            svc.wake();
        }
    }

    pub fn attach_data_layer(&self, worker: WorkerHandle, read_pool: ReadPool) {
        let _ = self.imp().worker.set(worker.clone());
        let _ = self.imp().read_pool.set(read_pool.clone());
        // v0.13 Slice 3 — wire the inline-syntax tab-completion
        // popover now that the read pool exists; the popover
        // consults `read::list_tags` for `#tag` candidates.
        crate::ui::inline_complete::attach(&self.imp().new_task_entry.clone(), Some(read_pool));
        // Phase 10 — Inspector pane needs the worker; install once
        // the data layer is up. Mode is then applied so the pane
        // shows / hides correctly on first paint.
        self.install_inspector_pane(worker);
        self.install_mode_observer();
        self.install_calendar_width_watcher();
        // Append the Areas / Projects sections to the sidebar.
        self.rebuild_dynamic_sidebar();
        // Initial content-pane load now that the read pool exists.
        self.refresh_active_list();
        // Apply the persisted mode (calls into apply_mode which
        // updates overlay-split visibility, sidebar Builder rows,
        // project extras, etc.).
        let mode = self.settings().string("mode").to_string();
        self.apply_mode(&mode);
    }

    /// Phase 12.5 — when the window crosses
    /// `crate::ui::calendar::COMPACT_WIDTH_THRESHOLD`, refresh the
    /// calendar page if it's the active view. The notify::default-
    /// width signal fires on every pixel of resize, so we cache the
    /// last-observed compact-mode flag in a Cell and only rebuild
    /// when it actually flips.
    pub(super) fn install_calendar_width_watcher(&self) {
        let last_compact: std::rc::Rc<Cell<Option<bool>>> = std::rc::Rc::new(Cell::new(None));
        let win_weak = self.downgrade();
        self.connect_default_width_notify(move |w| {
            let Some(win) = win_weak.upgrade() else {
                return;
            };
            let now_compact = w.default_width() > 0
                && w.default_width() < crate::ui::calendar::COMPACT_WIDTH_THRESHOLD;
            if last_compact.get() == Some(now_compact) {
                return;
            }
            last_compact.set(Some(now_compact));
            if matches!(win.active_list(), ActiveList::Calendar) {
                win.refresh_calendar_page();
            }
        });
    }

    /// Mount the Inspector pane into the AdwBin host declared in
    /// `data/window.ui`. Edit Tags hand-off routes through the
    /// existing tag-editor open path.
    pub(super) fn install_inspector_pane(&self, worker: WorkerHandle) {
        let win_weak = self.downgrade();
        let win_weak_for_navigate = self.downgrade();
        let win_weak_for_pool = self.downgrade();
        let pane = crate::ui::inspector_pane::InspectorPane::install(
            &self.imp().inspector_pane_host,
            worker,
            move |task_id| {
                if let Some(win) = win_weak.upgrade() {
                    win.open_tag_editor_for(task_id);
                }
            },
            move |uuid| {
                // v0.19.0 — Phase 18.5 Tier-2 Org link click.
                // Resolve UUID → task id via the read pool; if
                // it lands, route to open_inspector_for so the
                // inspector swaps to the linked task. Stale
                // links (UUID points to a deleted task) silently
                // no-op — the user's click was a navigation
                // attempt, not a state mutation.
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
            },
            move || {
                // v0.19.0 — Link… picker source. Lazy-resolves
                // the read pool on every popover-show.
                win_weak_for_pool.upgrade().and_then(|win| win.read_pool())
            },
        );
        *self.imp().inspector_pane.borrow_mut() = Some(pane);
    }

    /// Subscribe to GSettings `mode` and route changes through
    /// `apply_mode`. Per spec §3 / CLAUDE.md commitment #1, this is
    /// pure UI rerender — no worker dispatch.
    pub(super) fn install_mode_observer(&self) {
        let settings = self.settings();
        settings.connect_changed(
            Some("mode"),
            clone!(
                #[weak(rename_to = win)]
                self,
                move |s, _key| {
                    let mode = s.string("mode").to_string();
                    win.apply_mode(&mode);
                }
            ),
        );
    }

    /// Toggle every Builder-only UI surface based on the GSettings
    /// `mode` value. Idempotent. **Pure UI** — never reaches the
    /// worker.
    ///
    /// **Phase 10 acceptance — mode-flip snapshot invariant.**
    ///
    /// The only side effect of a mode flip on the DB layer is the
    /// GSettings key write itself. `apply_mode` calls only:
    ///
    /// - `OverlaySplitView::set_show_sidebar` (GTK setter, no I/O)
    /// - `Revealer::set_reveal_child` (GTK setter, no I/O)
    /// - `rebuild_dynamic_sidebar` (read-pool SELECTs only)
    /// - `set_active_list` → `refresh_active_list` (read-pool only)
    /// - `select_sidebar_row_for` (GTK setter, no I/O)
    ///
    /// None of these reach `WorkerHandle`. The read pool is
    /// read-only by construction (`PRAGMA query_only = ON` —
    /// enforced engine-side, see
    /// `atrium_core::db::read_pool::tests::read_only_enforcement_blocks_writes`).
    /// Any accidental write attempt errors at SQLite, never lands.
    ///
    /// This is the spec §5.3 / CLAUDE.md commitment #1 contract:
    /// flipping mode is a GSetting write plus a UI re-render,
    /// never a migration, never a DB write.
    pub fn apply_mode(&self, mode: &str) {
        let builder = mode == "builder";
        debug!(mode, builder, "apply_mode");

        // v0.1.6 — write the synchronous mode tracker first so any
        // callbacks that fire during the rest of this method (e.g.,
        // a selection-changed signal racing through the event loop)
        // observe the new mode immediately.
        self.imp().current_mode_is_builder.set(builder);

        // Right-side Inspector pane. Three independent levers all
        // resolve the same way (`builder`) — belt-and-suspenders
        // because v0.1.4 user testing surfaced a case where the
        // OverlaySplitView's show-sidebar didn't fully hide the
        // pane on its own.
        self.imp().overlay_split.set_show_sidebar(builder);
        self.imp().inspector_pane_host.set_visible(builder);
        if !builder && let Some(pane) = self.imp().inspector_pane.borrow().clone() {
            // Don't keep a stale per-task editor around when
            // there's no pane to render it in. A future flip back
            // to Builder repopulates from the live selection.
            pane.clear();
        }

        // Builder-only sidebar entries (Forecast / Review / Perspectives).
        // The rebuild_dynamic_sidebar pass below appends them when
        // mode = builder; here we drop the entries that aren't valid.
        self.rebuild_dynamic_sidebar();

        // Project page extras revealer — visible when on a project
        // view AND in Builder mode.
        let on_project = matches!(self.active_list(), ActiveList::Project(_));
        self.imp()
            .project_extras_revealer
            .set_reveal_child(builder && on_project);

        // If the active list became invalid (a Builder-only view
        // is selected and we just flipped back to Simple), fall back
        // to Today so the Simple Mode user isn't stranded on a hidden
        // sidebar row.
        let active = self.active_list();
        let invalid_in_simple = !builder
            && matches!(
                active,
                ActiveList::Forecast | ActiveList::Review | ActiveList::Perspective(_)
            );
        if invalid_in_simple {
            self.set_active_list(ActiveList::Today);
            self.select_sidebar_row_for(ActiveList::Today);
        }
    }

    /// Phase 10 — Builder-mode-aware project metadata cache.
    /// `rebuild_dynamic_sidebar` calls this so the project_extras
    /// toolbar can populate correctly when the user selects a
    /// project row.
    pub(super) fn refresh_project_meta(&self, projects: &[Project]) {
        let mut meta = self.imp().project_meta.borrow_mut();
        meta.clear();
        for p in projects {
            meta.insert(p.id, p.clone());
        }
    }

    /// Wire the project extras toolbar (Sequential switch + Review
    /// interval SpinButton) to update_project. Called once during
    /// `constructed`; the extras-syncing flag suppresses echoes
    /// when we populate fields programmatically on selection change.
    pub(super) fn wire_project_extras(&self) {
        let switch = self.imp().project_sequential_switch.clone();
        let spin = self.imp().project_review_spin.clone();

        let win_weak = self.downgrade();
        switch.connect_active_notify(move |sw| {
            let Some(win) = win_weak.upgrade() else {
                return;
            };
            if win.imp().project_extras_syncing.get() {
                return;
            }
            let ActiveList::Project(id) = win.active_list() else {
                return;
            };
            let Some(worker) = win.worker() else { return };
            let value = sw.is_active();
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker
                    .update_project(ProjectUpdate::new(id).sequential(value))
                    .await
                {
                    error!(?e, id, "update_project(sequential) failed");
                }
            });
        });

        let win_weak = self.downgrade();
        spin.connect_value_changed(move |sb| {
            let Some(win) = win_weak.upgrade() else {
                return;
            };
            if win.imp().project_extras_syncing.get() {
                return;
            }
            let ActiveList::Project(id) = win.active_list() else {
                return;
            };
            let Some(worker) = win.worker() else { return };
            let raw = sb.value().round() as i64;
            let value = if raw <= 0 { None } else { Some(raw) };
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker
                    .update_project(ProjectUpdate::new(id).review_interval_days(value))
                    .await
                {
                    error!(?e, id, "update_project(review_interval_days) failed");
                }
            });
        });
    }

    /// Populate the project extras toolbar from the cached project
    /// metadata for the active project, suppressing the value-
    /// changed handlers so we don't echo back as a worker write.
    pub(super) fn populate_project_extras(&self, project_id: i64) {
        let Some(project) = self.imp().project_meta.borrow().get(&project_id).cloned() else {
            return;
        };
        self.imp().project_extras_syncing.set(true);
        self.imp()
            .project_sequential_switch
            .set_active(project.sequential);
        self.imp()
            .project_review_spin
            .set_value(project.review_interval_days.unwrap_or(0) as f64);
        self.imp().project_extras_syncing.set(false);
    }

    pub(super) fn worker(&self) -> Option<WorkerHandle> {
        self.imp().worker.get().cloned()
    }

    /// Public accessor for the worker handle so non-window
    /// surfaces (Quick Entry modal in Phase 6c) can dispatch
    /// commands without round-tripping through window methods.
    pub fn worker_handle_for_quickentry(&self) -> Option<WorkerHandle> {
        self.imp().worker.get().cloned()
    }

    pub(super) fn read_pool(&self) -> Option<ReadPool> {
        self.imp().read_pool.get().cloned()
    }

    /// Public read-pool accessor for the Quick Entry modal so its
    /// inline-completion popover can fetch tag candidates. Mirrors
    /// the existing `worker_handle_for_quickentry`.
    pub fn read_pool_for_quickentry(&self) -> Option<ReadPool> {
        self.imp().read_pool.get().cloned()
    }

    pub fn set_active_list(&self, active: ActiveList) {
        if self.imp().active_list.borrow().clone() == active {
            return;
        }
        self.imp().active_list.replace(active.clone());
        let view_title = self.title_for(active.clone());
        self.imp().content_page.set_title(&view_title);
        // v0.6.11 — surface the active view in the window title so
        // it reads "Atrium · Today" / "Atrium · Inbox" / etc. The
        // window-level title shows in window managers, alt-tab
        // overlays, and screencast picker UIs; "Atrium" alone read
        // as a brand sticker not a context cue.
        self.set_title(Some(&format!("Atrium · {view_title}")));
        // v0.7.0 — magazine-spread page title. Big label gets the
        // view name; subtitle gets a supporting line per view (e.g.
        // today's date for Today). Subtitle hidden when empty so
        // the strip collapses on views without a useful subhead.
        self.imp().page_title_label.set_text(&view_title);
        let subtitle = self.subtitle_for(&active);
        if subtitle.is_empty() {
            self.imp().page_subtitle_label.set_visible(false);
        } else {
            self.imp().page_subtitle_label.set_text(&subtitle);
            self.imp().page_subtitle_label.set_visible(true);
        }
        self.refresh_active_list();

        // Phase 10 — project extras revealer follows the selection.
        // Visible only on a Project view in Builder Mode; populated
        // from the cached project metadata.
        let builder = self.imp().current_mode_is_builder.get();
        match &active {
            ActiveList::Project(id) => {
                self.imp().project_extras_revealer.set_reveal_child(builder);
                if builder {
                    self.populate_project_extras(*id);
                }
            }
            _ => {
                self.imp().project_extras_revealer.set_reveal_child(false);
            }
        }
    }

    /// Resolve the human-readable title for a given active list.
    /// Canonical lists return their static label; `Project(id)` and
    /// `Area(id)` consult the title caches populated when the sidebar
    /// was built.
    pub(super) fn title_for(&self, active: ActiveList) -> String {
        match active {
            ActiveList::Project(id) => {
                // v0.3.0 — when a project lives under an area, render
                // "Area › Project" so the heading anchors the user
                // in the hierarchy. Falls back to bare project name
                // when the project has no area (Unfiled).
                let project_title = self
                    .imp()
                    .project_titles
                    .borrow()
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| "Project".into());
                let area_title = self
                    .imp()
                    .project_meta
                    .borrow()
                    .get(&id)
                    .and_then(|p| p.area_id)
                    .and_then(|aid| self.imp().area_titles.borrow().get(&aid).cloned());
                match area_title {
                    Some(area) if !area.is_empty() => format!("{area} › {project_title}"),
                    _ => project_title,
                }
            }
            ActiveList::Area(id) => self
                .imp()
                .area_titles
                .borrow()
                .get(&id)
                .cloned()
                .unwrap_or_else(|| "Area".into()),
            ActiveList::Tag(id) => self
                .imp()
                .tag_titles
                .borrow()
                .get(&id)
                .map_or_else(|| "Tag".into(), |n| format!("#{n}")),
            ActiveList::Perspective(id) => self
                .imp()
                .perspective_titles
                .borrow()
                .get(&id)
                .cloned()
                .unwrap_or_else(|| "Perspective".into()),
            ActiveList::SearchResults(_)
            | ActiveList::Inbox
            | ActiveList::Today
            | ActiveList::Upcoming
            | ActiveList::Anytime
            | ActiveList::Someday
            | ActiveList::Logbook
            | ActiveList::Forecast
            | ActiveList::Review
            | ActiveList::Agenda
            | ActiveList::Calendar => active.canonical_title().to_string(),
        }
    }

    /// v0.7.0 — supporting subtitle for the magazine-spread page
    /// title strip. Empty string means "no subtitle" and the row is
    /// hidden by `set_active_list`. We use these sparingly: only
    /// where the subtitle adds real context (today's date on
    /// Today, the date range on Upcoming / Forecast).
    pub(super) fn subtitle_for(&self, active: &ActiveList) -> String {
        match active {
            ActiveList::Today => chrono::Local::now()
                .date_naive()
                .format("%A, %B %-d")
                .to_string(),
            ActiveList::Upcoming => "Next 7 days".to_string(),
            ActiveList::Forecast => "Next 30 days".to_string(),
            ActiveList::Calendar => {
                let viewed = self.calendar_viewed_or_today();
                format!(
                    "{} {}",
                    crate::ui::calendar::month_name(chrono::Datelike::month(&viewed)),
                    chrono::Datelike::year(&viewed)
                )
            }
            _ => String::new(),
        }
    }

    pub fn active_list(&self) -> ActiveList {
        self.imp().active_list.borrow().clone()
    }

    /// Build a closure that maps a task to its "Area › Project"
    /// context chip. Returns the empty string for views where the
    /// chip would just echo what the user already sees:
    ///
    /// - `Project(_)`: the heading already names the project; no chip.
    /// - `Area(_)`: the area name is in the heading. Render only the
    ///   project name (drops the area part).
    ///
    /// Other views (Today / Inbox / Anytime / Someday / Logbook /
    /// Tag / Forecast / Perspective / SearchResults / Upcoming)
    /// render the full "Area › Project" form so users can place a
    /// task in their hierarchy at a glance.
    /// v0.4.0 — derive the project_id → area_id map from the cached
    /// `project_meta`. Used by the search evaluator's `area:` matcher
    /// and by `build_context_resolver` for the row-context chip.
    pub(super) fn project_areas_map(&self) -> HashMap<i64, Option<i64>> {
        self.imp()
            .project_meta
            .borrow()
            .iter()
            .map(|(id, p)| (*id, p.area_id))
            .collect()
    }

    /// v0.5.0 (Slice B2) — area-accent resolver. Returns a closure
    /// that takes a `Task` and yields the hex string of the area
    /// the task's project belongs to (or empty if unfiled / no
    /// area / no colour). The row factory mirrors the resulting
    /// hex to one of the `.atrium-area-accent-{color}` CSS classes
    /// for the row's left-border stripe.
    pub(super) fn build_area_color_resolver(&self) -> impl Fn(&Task) -> String + use<> {
        let project_areas: HashMap<i64, Option<i64>> = self
            .imp()
            .project_meta
            .borrow()
            .iter()
            .map(|(id, p)| (*id, p.area_id))
            .collect();
        let area_colors: HashMap<i64, Option<String>> = self.imp().area_colors.borrow().clone();
        move |task: &Task| -> String {
            let Some(pid) = task.project_id else {
                return String::new();
            };
            let Some(Some(aid)) = project_areas.get(&pid).copied() else {
                return String::new();
            };
            area_colors.get(&aid).cloned().flatten().unwrap_or_default()
        }
    }

    /// v0.15.0 — Phase 18.5 Tier-1 statistics-cookie resolver.
    /// Snapshots the per-parent `(done, total)` map from the read
    /// pool once and returns a closure that turns each task into
    /// its `[N/M]` cookie string. The cookie folds child TODO
    /// counts (from the snapshot) with body-checkbox counts
    /// (parsed from each task's note), mirroring Org's
    /// `org-checkbox-hierarchical-statistics` default. A task with
    /// zero subtasks but a body checklist still earns a cookie;
    /// a task with neither stays empty. Both modes.
    pub(super) fn build_cookie_resolver(&self) -> impl Fn(&Task) -> String + use<> {
        let child_counts: HashMap<i64, (u32, u32)> = self
            .read_pool()
            .and_then(|pool| {
                pool.with(atrium_core::db::read::count_done_total_per_parent)
                    .ok()
            })
            .unwrap_or_default();
        move |task: &Task| -> String {
            let (child_done, child_total) = child_counts.get(&task.id).copied().unwrap_or((0, 0));
            let (body_done, body_total) = atrium_core::count_body_checkboxes(&task.note);
            let total = child_total.saturating_add(body_total);
            if total == 0 {
                return String::new();
            }
            let done = child_done.saturating_add(body_done);
            format!("[{done}/{total}]")
        }
    }

    pub(super) fn build_context_resolver(
        &self,
        active: &ActiveList,
    ) -> impl Fn(&Task) -> String + use<> {
        let project_titles = self.imp().project_titles.borrow().clone();
        let area_titles = self.imp().area_titles.borrow().clone();
        let project_areas: HashMap<i64, Option<i64>> = self
            .imp()
            .project_meta
            .borrow()
            .iter()
            .map(|(id, p)| (*id, p.area_id))
            .collect();
        let mode = match active {
            ActiveList::Project(_) => ContextMode::Suppressed,
            ActiveList::Area(_) => ContextMode::ProjectOnly,
            _ => ContextMode::AreaAndProject,
        };
        // v0.6.11 — when the active list IS Inbox, suppress the
        // "Inbox" no-project chip. Every row on that view is
        // already in Inbox by definition; the chip just duplicates
        // what the page header says.
        let suppress_inbox_chip = matches!(active, ActiveList::Inbox);
        move |task: &Task| -> String {
            if matches!(mode, ContextMode::Suppressed) {
                return String::new();
            }
            let Some(pid) = task.project_id else {
                // v0.2.2 — when a task has no project (Inbox), the
                // chip would render blank in AreaAndProject mode.
                // Users unfamiliar with the data model don't know
                // a missing chip means "Inbox". Render it
                // explicitly. ProjectOnly views (Area pages) keep
                // the empty render — there's no project to name and
                // the heading already names the area. v0.6.11
                // adds: suppress on the Inbox view itself.
                if suppress_inbox_chip {
                    return String::new();
                }
                let inbox = match mode {
                    ContextMode::AreaAndProject => "Inbox".to_string(),
                    _ => String::new(),
                };
                return inbox;
            };
            let project = project_titles.get(&pid).cloned().unwrap_or_default();
            if matches!(mode, ContextMode::ProjectOnly) {
                return project;
            }
            let area = project_areas
                .get(&pid)
                .copied()
                .flatten()
                .and_then(|aid| area_titles.get(&aid).cloned());
            match area {
                Some(area) if !area.is_empty() && !project.is_empty() => {
                    format!("{area} › {project}")
                }
                _ => project,
            }
        }
    }
}
