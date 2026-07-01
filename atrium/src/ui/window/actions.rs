// SPDX-License-Identifier: MIT
//! `AtriumWindow`: GAction wiring, create/rename/delete/perspective
//! dialogs, and canonical-list navigation. Extracted from
//! window/mod.rs in v0.22.0's structural split (Pass 3).

use super::*;

impl AtriumWindow {
    pub(super) fn install_window_actions(&self) {
        let delete = gio::SimpleAction::new("delete-task", None);
        delete.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.delete_focused_task()
        ));
        self.add_action(&delete);

        let toggle = gio::SimpleAction::new("toggle-complete", None);
        toggle.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.toggle_focused_task()
        ));
        self.add_action(&toggle);

        // Rename / delete operate on the active project or area in
        // the sidebar. No-op when the active list is canonical.
        let rename = gio::SimpleAction::new("rename-active", None);
        rename.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.prompt_rename_active()
        ));
        self.add_action(&rename);

        let del_active = gio::SimpleAction::new("delete-active", None);
        del_active.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.prompt_delete_active()
        ));
        self.add_action(&del_active);

        // v0.6.2 (Slice D1 GUI) — configure the active Perspective's
        // renderer (`list` ↔ `board`) and, when board, its column
        // list. No-op when the active list isn't a Perspective.
        let configure_renderer = gio::SimpleAction::new("configure-renderer", None);
        configure_renderer.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.prompt_configure_renderer()
        ));
        self.add_action(&configure_renderer);

        // v0.7.3 — full perspective editor (name + filter + renderer +
        // columns in one dialog). Triggered from the perspective row's
        // right-click "Edit…" menu item.
        let edit_persp = gio::SimpleAction::new("edit-perspective", None);
        edit_persp.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.prompt_edit_perspective()
        ));
        self.add_action(&edit_persp);

        // Phase 14 — save the current search bar query as a named
        // perspective. Only fires when the active list is
        // SearchResults; otherwise no-ops with a debug log.
        let save_persp = gio::SimpleAction::new("save-perspective", None);
        save_persp.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.prompt_save_perspective()
        ));
        self.add_action(&save_persp);

        let archive = gio::SimpleAction::new("archive-active-project", None);
        archive.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.archive_active_project()
        ));
        self.add_action(&archive);

        // Phase 7c — bulk action surfaces.
        let bulk_complete = gio::SimpleAction::new("bulk-complete", None);
        bulk_complete.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.bulk_complete_selection()
        ));
        self.add_action(&bulk_complete);

        let bulk_delete = gio::SimpleAction::new("bulk-delete", None);
        bulk_delete.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.bulk_delete_selection()
        ));
        self.add_action(&bulk_delete);

        let bulk_clear = gio::SimpleAction::new("bulk-clear", None);
        bulk_clear.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.clear_selection()
        ));
        self.add_action(&bulk_clear);

        // v0.42.0 — bulk edit surfaces beyond complete/delete. Each
        // reuses the same worker calls the single-task paths use, so
        // the capability stays CLI-testable (`atrium-cli edit ID…`).
        let bulk_move = gio::SimpleAction::new("bulk-move", None);
        bulk_move.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.bulk_move_selection()
        ));
        self.add_action(&bulk_move);

        let bulk_tag = gio::SimpleAction::new("bulk-tag", None);
        bulk_tag.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.bulk_tag_selection()
        ));
        self.add_action(&bulk_tag);

        let bulk_reschedule =
            gio::SimpleAction::new("bulk-reschedule", Some(&String::static_variant_type()));
        bulk_reschedule.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, param| {
                if let Some(when) = param.and_then(|v| v.get::<String>()) {
                    win.bulk_reschedule_selection(&when);
                }
            }
        ));
        self.add_action(&bulk_reschedule);

        // Build the selection bar's Schedule menu in code (same keyword
        // set as the per-row Schedule submenu), then hang it off the
        // `bulk_schedule_button` MenuButton.
        let schedule_menu = gio::Menu::new();
        for (label, key) in [
            ("Today", "today"),
            ("Tomorrow", "tomorrow"),
            ("This Weekend", "weekend"),
            ("Next Week", "nextweek"),
            ("Someday", "someday"),
            ("Clear Schedule", "clear"),
        ] {
            let item = gio::MenuItem::new(Some(label), None);
            item.set_action_and_target_value(Some("win.bulk-reschedule"), Some(&key.to_variant()));
            schedule_menu.append_item(&item);
        }
        self.imp()
            .bulk_schedule_button
            .set_menu_model(Some(&schedule_menu));

        let select_all = gio::SimpleAction::new("select-all", None);
        select_all.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.select_all_visible()
        ));
        self.add_action(&select_all);

        // Phase 8c — high-legibility font toggle. Stateful boolean
        // action backed by the `high-legibility-font` GSetting. Both
        // directions sync: clicking the menu item flips the GSetting,
        // and an external dconf write (or an initial preset) flows
        // back into the action state + CSS class.
        let settings = self.settings();
        let initial_hl = settings.boolean("high-legibility-font");
        self.apply_high_legibility(initial_hl);
        let hl_action =
            gio::SimpleAction::new_stateful("high-legibility-font", None, &initial_hl.to_variant());
        hl_action.connect_change_state(clone!(
            #[weak(rename_to = win)]
            self,
            move |action, value| {
                let Some(value) = value else { return };
                let on = value.get::<bool>().unwrap_or(false);
                let _ = win.settings().set_boolean("high-legibility-font", on);
                action.set_state(value);
                win.apply_high_legibility(on);
            }
        ));
        self.add_action(&hl_action);
        // Listen for external GSetting changes (dconf-editor, another
        // process) so the action state and CSS class stay coherent.
        settings.connect_changed(
            Some("high-legibility-font"),
            clone!(
                #[weak(rename_to = win)]
                self,
                #[strong]
                hl_action,
                move |s, _key| {
                    let on = s.boolean("high-legibility-font");
                    hl_action.set_state(&on.to_variant());
                    win.apply_high_legibility(on);
                }
            ),
        );

        // Phase 7i — Ctrl+I (or row right-click → Edit Details…)
        // opens the Inspector dialog for the focused / first-selected
        // task.
        let edit_details = gio::SimpleAction::new("edit-details-focused", None);
        edit_details.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.open_inspector_focused()
        ));
        self.add_action(&edit_details);
        let edit_details_for =
            gio::SimpleAction::new("edit-details-for", Some(&i64::static_variant_type()));
        edit_details_for.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, parameter| {
                let Some(target) = parameter else { return };
                let Some(id) = target.get::<i64>() else {
                    return;
                };
                win.open_inspector_for(id);
            }
        ));
        self.add_action(&edit_details_for);

        // Tier D (v0.39.3) — quick reschedule from the row context
        // menu's Schedule submenu. Target is `(task_id, keyword)` where
        // keyword is today / tomorrow / weekend / nextweek / someday /
        // clear. Collapses the open-editor-then-set-date round-trip to
        // a single right-click → pick.
        let reschedule =
            gio::SimpleAction::new("reschedule", Some(&<(i64, String)>::static_variant_type()));
        reschedule.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, parameter| {
                let Some((id, when)) = parameter.and_then(|p| p.get::<(i64, String)>()) else {
                    return;
                };
                win.quick_reschedule(id, &when);
            }
        ));
        self.add_action(&reschedule);

        // Tier D (v0.40.x) — Alt+Up / Alt+Down keyboard-reorder the
        // focused task (a keyboard alternative to drag-reorder). No-op
        // (with a toast) on date-sorted lists, same as a drag.
        let move_up = gio::SimpleAction::new("move-task-up", None);
        move_up.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.move_focused_task(true)
        ));
        self.add_action(&move_up);
        let move_down = gio::SimpleAction::new("move-task-down", None);
        move_down.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.move_focused_task(false)
        ));
        self.add_action(&move_down);

        // Phase 7g — Ctrl+T (or row right-click) opens the tag
        // editor for the focused / first-selected task.
        let edit_tags = gio::SimpleAction::new("edit-tags-focused", None);
        edit_tags.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.edit_tags_focused()
        ));
        self.add_action(&edit_tags);

        // Phase 7g — parameterized variant for the row context menu,
        // which knows the task id at popover-build time. Keeps the
        // menu working even when the right-click row isn't part of
        // the current selection.
        let edit_tags_for =
            gio::SimpleAction::new("edit-tags-for", Some(&i64::static_variant_type()));
        edit_tags_for.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, parameter| {
                let Some(target) = parameter else { return };
                let Some(id) = target.get::<i64>() else {
                    return;
                };
                win.open_tag_editor_for(id);
            }
        ));
        self.add_action(&edit_tags_for);

        // Phase 7f — Ctrl+Z invokes the most recent undo callback.
        let undo = gio::SimpleAction::new("undo", None);
        undo.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.invoke_last_undo()
        ));
        self.add_action(&undo);

        // Phase 7e — focus the sidebar filter (Ctrl+L).
        let focus_sidebar_filter = gio::SimpleAction::new("focus-sidebar-filter", None);
        focus_sidebar_filter.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| {
                let entry = win.imp().sidebar_filter.clone();
                entry.grab_focus();
                entry.select_region(0, -1);
            }
        ));
        self.add_action(&focus_sidebar_filter);
    }

    /// Apply a substring filter against area / project / tag rows.
    /// Canonical lists are always visible; a section header is visible
    /// iff at least one row in its section passes the filter. An empty
    /// query restores everything. Phase 7e.
    pub fn apply_sidebar_filter(&self, query: &str) {
        let targets = self.imp().sidebar_targets.borrow().clone();
        let titles = self.imp().sidebar_titles.borrow().clone();
        let list_box = self.imp().sidebar_list.clone();

        let visible = compute_sidebar_visibility(query, CANONICAL_LISTS.len(), &targets, &titles);

        for (idx, v) in visible.iter().enumerate() {
            if let Some(row) = list_box.row_at_index(idx as i32) {
                row.set_visible(*v);
            }
        }
    }

    /// Prompt for an area name and create it. Used by the
    /// `app.new-area` action. v0.5.0 (Slice B2) — the prompt grows
    /// the same six-swatch colour picker tags use, so an area can
    /// carry an optional accent that paints a 3 px stripe down the
    /// left of every task row filed under it.
    pub fn prompt_create_area(&self) {
        let win = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let Some((title, color, review)) =
                prompt_for_named_color(&win, "New Area", "Area name", "", None, "Create", Some(0))
                    .await
            else {
                return;
            };
            let Some(worker) = win.worker() else { return };
            let default_review_interval_days = review.filter(|&d| d > 0);
            if let Err(e) = worker
                .create_area(NewArea {
                    title,
                    color,
                    default_review_interval_days,
                })
                .await
            {
                error!(?e, "create_area failed");
            }
        });
    }

    /// Prompt for a project name and create it. If the sidebar's
    /// active list is an Area, the new project lands inside that area.
    pub fn prompt_create_project(&self) {
        let win = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let Some(title) =
                prompt_for_text(&win, "New Project", "Project name", "", "Create").await
            else {
                return;
            };
            let Some(worker) = win.worker() else { return };
            // We currently only track project→area lookup well
            // enough to default new projects when the user is on
            // an Area row. From a Project row the new project
            // lands unfiled — caching project→area would let us
            // inherit the parent area, but the project_titles map
            // doesn't carry that yet. Picked up when sidebar caches
            // grow to include area_id alongside title.
            let area_id = match win.active_list() {
                ActiveList::Area(id) => Some(id),
                _ => None,
            };
            let new = if let Some(aid) = area_id {
                NewProject::in_area(title, aid)
            } else {
                NewProject::unfiled(title)
            };
            if let Err(e) = worker.create_project(new).await {
                error!(?e, "create_project failed");
            }
        });
    }

    /// v0.33.0 — pick a saved task template and stamp it out as a fresh
    /// project. Authoring templates is CLI-side (`atrium-cli
    /// task-template create`); this is the GUI instantiate affordance.
    pub fn prompt_create_from_template(&self) {
        let Some(pool) = self.read_pool() else { return };
        let templates = pool
            .with(atrium_core::db::read::list_task_templates)
            .unwrap_or_default();
        if templates.is_empty() {
            self.show_toast(
                "No task templates yet. Create one with `atrium-cli task-template create`.",
            );
            return;
        }
        let names: Vec<&str> = templates.iter().map(|t| t.name.as_str()).collect();
        let model = gtk::StringList::new(&names);
        let dropdown = gtk::DropDown::builder().model(&model).build();
        let dialog = adw::AlertDialog::new(
            Some("Create from Template"),
            Some("Stamp the chosen template out as a new project."),
        );
        dialog.set_extra_child(Some(&dropdown));
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("ok", "Create");
        dialog.set_default_response(Some("ok"));
        dialog.set_close_response("cancel");
        dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);

        let ids: Vec<i64> = templates.iter().map(|t| t.id).collect();
        let win = self.clone();
        glib::MainContext::default().spawn_local(async move {
            if dialog.choose_future(&win).await.as_str() != "ok" {
                return;
            }
            let Some(&template_id) = ids.get(dropdown.selected() as usize) else {
                return;
            };
            let Some(worker) = win.worker() else { return };
            match worker.instantiate_template(template_id).await {
                Ok(project) => {
                    win.set_active_list(ActiveList::Project(project.id));
                    win.select_sidebar_row_for(ActiveList::Project(project.id));
                }
                Err(e) => {
                    error!(?e, "instantiate_template failed");
                    win.show_toast("Could not create project from template.");
                }
            }
        });
    }

    /// v0.34.0 — open the unified import dialog. No-op until the
    /// worker is attached.
    pub fn open_import_dialog(&self) {
        if let Some(worker) = self.worker() {
            crate::ui::import_dialog::open(self, worker);
        }
    }

    pub(super) fn prompt_rename_active(&self) {
        // Phase 7f — F2 prefers in-list inline editing when the task
        // list has focus. Falls through to the sidebar rename for
        // Area / Project / Tag when the focus lives elsewhere.
        if self.start_edit_focused_row() {
            return;
        }
        let active = self.active_list();
        let win = self.clone();
        match active {
            ActiveList::Area(id) => {
                let current_name = self
                    .imp()
                    .area_titles
                    .borrow()
                    .get(&id)
                    .cloned()
                    .unwrap_or_default();
                let current_color = self.imp().area_colors.borrow().get(&id).cloned().flatten();
                let current_review = self
                    .imp()
                    .area_review_intervals
                    .borrow()
                    .get(&id)
                    .cloned()
                    .flatten();
                glib::MainContext::default().spawn_local(async move {
                    let Some((title, color, review)) = prompt_for_named_color(
                        &win,
                        "Edit Area",
                        "Area name",
                        &current_name,
                        current_color.as_deref(),
                        "Save",
                        Some(current_review.unwrap_or(0)),
                    )
                    .await
                    else {
                        return;
                    };
                    let Some(worker) = win.worker() else { return };
                    // 0 from the spin means "no default": `interval`
                    // becomes None, and the builder's Some(None) clears
                    // any existing default rather than just changing it.
                    let interval = review.filter(|&d| d > 0);
                    if let Err(e) = worker
                        .update_area(
                            AreaUpdate::new(id)
                                .title(title)
                                .color(color)
                                .default_review_interval_days(interval),
                        )
                        .await
                    {
                        error!(?e, id, "update_area failed");
                    }
                });
            }
            ActiveList::Project(id) => {
                let current = self
                    .imp()
                    .project_titles
                    .borrow()
                    .get(&id)
                    .cloned()
                    .unwrap_or_default();
                glib::MainContext::default().spawn_local(async move {
                    let Some(title) =
                        prompt_for_text(&win, "Rename Project", "Project name", &current, "Rename")
                            .await
                    else {
                        return;
                    };
                    let Some(worker) = win.worker() else { return };
                    if let Err(e) = worker
                        .update_project(ProjectUpdate::new(id).title(title))
                        .await
                    {
                        error!(?e, id, "update_project failed");
                    }
                });
            }
            ActiveList::Tag(id) => {
                let current_name = self
                    .imp()
                    .tag_titles
                    .borrow()
                    .get(&id)
                    .cloned()
                    .unwrap_or_default();
                let current_color = self.imp().tag_colors.borrow().get(&id).cloned().flatten();
                glib::MainContext::default().spawn_local(async move {
                    let Some((name, color, _)) = prompt_for_named_color(
                        &win,
                        "Edit Tag",
                        "Tag name",
                        &current_name,
                        current_color.as_deref(),
                        "Save",
                        None,
                    )
                    .await
                    else {
                        return;
                    };
                    let Some(worker) = win.worker() else { return };
                    if let Err(e) = worker
                        .update_tag(TagUpdate::new(id).name(name).color(color))
                        .await
                    {
                        error!(?e, id, "update_tag failed");
                    }
                });
            }
            ActiveList::Perspective(id) => {
                let current = self
                    .imp()
                    .perspective_titles
                    .borrow()
                    .get(&id)
                    .cloned()
                    .unwrap_or_default();
                glib::MainContext::default().spawn_local(async move {
                    let Some(name) = prompt_for_text(
                        &win,
                        "Rename Perspective",
                        "Perspective name",
                        &current,
                        "Rename",
                    )
                    .await
                    else {
                        return;
                    };
                    let Some(worker) = win.worker() else { return };
                    if let Err(e) = worker
                        .update_perspective(PerspectiveUpdate::new(id).name(name))
                        .await
                    {
                        error!(?e, id, "update_perspective failed");
                    }
                });
            }
            _ => {
                debug!("rename-active: nothing to rename in canonical list");
            }
        }
    }

    pub(super) fn prompt_delete_active(&self) {
        let active = self.active_list();
        let win = self.clone();
        match active {
            ActiveList::Area(id) => {
                let title = self
                    .imp()
                    .area_titles
                    .borrow()
                    .get(&id)
                    .cloned()
                    .unwrap_or_default();
                glib::MainContext::default().spawn_local(async move {
                    let confirmed = prompt_confirm_destructive(
                        &win,
                        "Delete Area?",
                        &format!(
                            "“{}” will be removed. Projects inside it become unfiled — their tasks aren't deleted.",
                            title
                        ),
                        "Delete",
                    )
                    .await;
                    if !confirmed {
                        return;
                    }
                    let Some(worker) = win.worker() else { return };
                    if let Err(e) = worker.delete_area(id).await {
                        error!(?e, id, "delete_area failed");
                    }
                });
            }
            ActiveList::Project(id) => {
                let title = self
                    .imp()
                    .project_titles
                    .borrow()
                    .get(&id)
                    .cloned()
                    .unwrap_or_default();
                glib::MainContext::default().spawn_local(async move {
                    let confirmed = prompt_confirm_destructive(
                        &win,
                        "Delete Project?",
                        &format!(
                            "“{}” and every task inside it will be removed. This cannot be undone.",
                            title
                        ),
                        "Delete",
                    )
                    .await;
                    if !confirmed {
                        return;
                    }
                    let Some(worker) = win.worker() else { return };
                    if let Err(e) = worker.delete_project(id).await {
                        error!(?e, id, "delete_project failed");
                    }
                });
            }
            ActiveList::Tag(id) => {
                let title = self
                    .imp()
                    .tag_titles
                    .borrow()
                    .get(&id)
                    .cloned()
                    .unwrap_or_default();
                glib::MainContext::default().spawn_local(async move {
                    let confirmed = prompt_confirm_destructive(
                        &win,
                        "Delete Tag?",
                        &format!(
                            "“{}” will be removed. Tasks bearing this tag stay; the tag association is dropped.",
                            title
                        ),
                        "Delete",
                    )
                    .await;
                    if !confirmed {
                        return;
                    }
                    let Some(worker) = win.worker() else { return };
                    if let Err(e) = worker.delete_tag(id).await {
                        error!(?e, id, "delete_tag failed");
                    }
                });
            }
            ActiveList::Perspective(id) => {
                let title = self
                    .imp()
                    .perspective_titles
                    .borrow()
                    .get(&id)
                    .cloned()
                    .unwrap_or_default();
                glib::MainContext::default().spawn_local(async move {
                    let confirmed = prompt_confirm_destructive(
                        &win,
                        "Delete Perspective?",
                        &format!(
                            "“{}” will be removed. Tasks the perspective surfaces are not affected — only the saved view is deleted.",
                            title
                        ),
                        "Delete",
                    )
                    .await;
                    if !confirmed {
                        return;
                    }
                    let Some(worker) = win.worker() else { return };
                    if let Err(e) = worker.delete_perspective(id).await {
                        error!(?e, id, "delete_perspective failed");
                    }
                });
            }
            _ => {
                debug!("delete-active: nothing to delete in canonical list");
            }
        }
    }

    pub fn prompt_create_tag(&self) {
        let win = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let Some((name, color, _)) =
                prompt_for_named_color(&win, "New Tag", "Tag name", "", None, "Create", None).await
            else {
                return;
            };
            let Some(worker) = win.worker() else { return };
            if let Err(e) = worker.create_tag(NewTag { name, color }).await {
                error!(?e, "create_tag failed");
            }
        });
    }

    /// Phase 14 — capture the current search bar query as a named
    /// perspective. Only valid on SearchResults views; the menu item
    /// surfaces the action but no-ops elsewhere with a debug log so
    /// keyboard / accelerator dispatch doesn't crash.
    pub(super) fn prompt_save_perspective(&self) {
        let ActiveList::SearchResults(query) = self.active_list() else {
            debug!("save-perspective: not on a SearchResults view; ignoring");
            return;
        };
        let trimmed = query.trim().to_string();
        if trimmed.is_empty() {
            debug!("save-perspective: empty query; ignoring");
            return;
        }
        let win = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let Some(name) =
                prompt_for_text(&win, "Save Perspective", "Perspective name", "", "Save").await
            else {
                return;
            };
            let Some(worker) = win.worker() else { return };
            match worker
                .create_perspective(NewPerspective {
                    name: name.clone(),
                    icon: None,
                    filter_expr: trimmed,
                    ..Default::default()
                })
                .await
            {
                Ok(p) => {
                    // Switch to the new perspective so the user sees
                    // the saved view immediately.
                    win.set_active_list(ActiveList::Perspective(p.id));
                }
                Err(e) => error!(?e, "create_perspective failed"),
            }
        });
    }

    /// v0.6.2 (Slice D1 GUI) — configure the active Perspective's
    /// renderer (`list` ↔ `board`) and, when board, its column list.
    /// Surfaces from the perspective row's right-click context menu.
    /// No-op when the active list isn't a Perspective.
    pub(super) fn prompt_configure_renderer(&self) {
        let ActiveList::Perspective(id) = self.active_list() else {
            debug!("configure-renderer: not on a Perspective");
            return;
        };
        let perspective = self.imp().perspective_meta.borrow().get(&id).cloned();
        let Some(perspective) = perspective else {
            return;
        };
        let win = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let Some((renderer, config)) =
                prompt_configure_renderer_dialog(&win, &perspective).await
            else {
                return;
            };
            let Some(worker) = win.worker() else { return };
            let mut update = atrium_core::PerspectiveUpdate::new(id).renderer(renderer);
            update = update.renderer_config(config);
            if let Err(e) = worker.update_perspective(update).await {
                error!(?e, id, "update_perspective (renderer) failed");
            }
        });
    }

    /// v0.7.3 — open the full perspective editor for the active
    /// Perspective, mapping the captured fields onto a
    /// `PerspectiveUpdate` and dispatching it. Wired to
    /// `win.edit-perspective` (right-click → Edit\u{2026} on a
    /// perspective sidebar row).
    pub(super) fn prompt_edit_perspective(&self) {
        let ActiveList::Perspective(id) = self.active_list() else {
            debug!("edit-perspective: not on a Perspective");
            return;
        };
        let perspective = self.imp().perspective_meta.borrow().get(&id).cloned();
        let Some(perspective) = perspective else {
            return;
        };
        let win = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let parent: gtk::Widget = win.clone().upcast();
            let Some(fields) = prompt_edit_perspective(&parent, Some(&perspective)).await else {
                return;
            };
            let Some(worker) = win.worker() else { return };
            let mut update = atrium_core::PerspectiveUpdate::new(id)
                .name(fields.name)
                .filter_expr(fields.filter_expr)
                .renderer(fields.renderer);
            update = update.renderer_config(fields.renderer_config);
            if let Err(e) = worker.update_perspective(update).await {
                error!(?e, id, "update_perspective (full edit) failed");
            }
        });
    }

    pub(super) fn archive_active_project(&self) {
        let ActiveList::Project(id) = self.active_list() else {
            debug!("archive-active-project: not on a project view");
            return;
        };
        let title = self
            .imp()
            .project_titles
            .borrow()
            .get(&id)
            .cloned()
            .unwrap_or_default();
        let win = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let confirmed = prompt_confirm_destructive(
                &win,
                "Archive Project?",
                &format!(
                    "“{}” will be archived and every open task inside it will be marked complete. They'll appear in Logbook.",
                    title
                ),
                "Archive",
            )
            .await;
            if !confirmed {
                return;
            }
            let Some(worker) = win.worker() else { return };
            if let Err(e) = worker.archive_project(id).await {
                error!(?e, id, "archive_project failed");
            }
        });
    }

    /// Activate from a sidebar shortcut (Ctrl+1..6) — jumps to the
    /// canonical list at `idx`. Project / area shortcuts are reserved
    /// for Phase 5b's CRUD pass.
    pub fn show_list_at(&self, idx: usize) {
        if let Some(active) = CANONICAL_LISTS.get(idx) {
            self.set_active_list(active.clone());
            if let Some(row) = self.imp().sidebar_list.row_at_index(idx as i32) {
                self.imp().sidebar_list.select_row(Some(&row));
            }
        }
    }
}
