// SPDX-License-Identifier: MIT
//! `AtriumWindow`: primary menu, sidebar build, context menus, count badges.
//! Extracted from window/mod.rs in v0.22.0 split (Pass 3).

use super::*;

impl AtriumWindow {
    pub(super) fn install_menu(&self) {
        let menu = build_primary_menu(self.imp().debug_enabled.get());
        self.imp().menu_button.set_menu_model(Some(&menu));
    }

    /// Attach a right-click context menu to a project row. The menu
    /// targets `win.*` actions which consult `active_list()`, so we
    /// set the row's project as active before popping the menu —
    /// otherwise Rename / Delete / Archive would operate on whatever
    /// list was selected before the right-click.
    pub(super) fn install_project_context_menu(&self, row: &gtk::ListBoxRow, project_id: i64) {
        let menu = gio::Menu::new();
        menu.append(Some("Rename"), Some("win.rename-active"));
        menu.append(Some("Archive"), Some("win.archive-active-project"));
        menu.append(Some("Delete"), Some("win.delete-active"));
        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.set_has_arrow(false);
        popover.set_parent(row);
        // Phase 8h — stash the popover so we can `unparent()` it
        // before the row finalizes; otherwise GTK warns about a
        // ListBoxRow being torn down with a still-attached child.
        unsafe {
            row.set_data("atrium-context-popover", popover.clone());
        }

        let gesture = gtk::GestureClick::new();
        gesture.set_button(gtk::gdk::BUTTON_SECONDARY);
        let win_weak = self.downgrade();
        gesture.connect_pressed(move |_, _, x, y| {
            let Some(win) = win_weak.upgrade() else {
                return;
            };
            win.set_active_list(ActiveList::Project(project_id));
            popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });
        row.add_controller(gesture);
    }

    /// Right-click context menu on a tag row — Rename / Delete.
    pub(super) fn install_tag_context_menu(&self, row: &gtk::ListBoxRow, tag_id: i64) {
        let menu = gio::Menu::new();
        menu.append(Some("Rename"), Some("win.rename-active"));
        menu.append(Some("Delete"), Some("win.delete-active"));
        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.set_has_arrow(false);
        popover.set_parent(row);
        // Phase 8h — stash the popover so we can `unparent()` it
        // before the row finalizes; otherwise GTK warns about a
        // ListBoxRow being torn down with a still-attached child.
        unsafe {
            row.set_data("atrium-context-popover", popover.clone());
        }

        let gesture = gtk::GestureClick::new();
        gesture.set_button(gtk::gdk::BUTTON_SECONDARY);
        let win_weak = self.downgrade();
        gesture.connect_pressed(move |_, _, x, y| {
            let Some(win) = win_weak.upgrade() else {
                return;
            };
            win.set_active_list(ActiveList::Tag(tag_id));
            popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });
        row.add_controller(gesture);
    }

    /// Same idea for areas — Rename / Delete only (areas don't archive).
    pub(super) fn install_area_context_menu(&self, row: &gtk::ListBoxRow, area_id: i64) {
        let menu = gio::Menu::new();
        menu.append(Some("Rename"), Some("win.rename-active"));
        menu.append(Some("Delete"), Some("win.delete-active"));
        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.set_has_arrow(false);
        popover.set_parent(row);
        // Phase 8h — stash the popover so we can `unparent()` it
        // before the row finalizes; otherwise GTK warns about a
        // ListBoxRow being torn down with a still-attached child.
        unsafe {
            row.set_data("atrium-context-popover", popover.clone());
        }

        let gesture = gtk::GestureClick::new();
        gesture.set_button(gtk::gdk::BUTTON_SECONDARY);
        let win_weak = self.downgrade();
        gesture.connect_pressed(move |_, _, x, y| {
            let Some(win) = win_weak.upgrade() else {
                return;
            };
            win.set_active_list(ActiveList::Area(area_id));
            popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });
        row.add_controller(gesture);
    }

    /// v0.7.3 — Perspectives section header with a trailing "+"
    /// affordance. Clicking + opens `prompt_edit_perspective` in
    /// create mode and dispatches `worker.create_perspective` on
    /// Save. The header label keeps the same `.atrium-sidebar-section`
    /// styling as other section headers; the button uses `.flat`
    /// plus `.circular` so it reads as an inline affordance not a
    /// primary action.
    pub(super) fn build_perspectives_section_header(&self) -> gtk::ListBoxRow {
        let label = gtk::Label::builder()
            .label("Perspectives")
            .halign(gtk::Align::Start)
            .hexpand(true)
            .build();
        label.add_css_class("dim-label");
        label.add_css_class("caption-heading");
        label.add_css_class("atrium-sidebar-section");

        let add_button = gtk::Button::builder()
            .icon_name("list-add-symbolic")
            .tooltip_text("New Perspective")
            .css_classes(["flat", "circular"])
            .valign(gtk::Align::Center)
            .build();
        add_button.update_property(&[gtk::accessible::Property::Label("New Perspective")]);
        let win_weak = self.downgrade();
        add_button.connect_clicked(move |_| {
            let Some(win) = win_weak.upgrade() else {
                return;
            };
            let win_for_dispatch = win.clone();
            glib::MainContext::default().spawn_local(async move {
                let parent: gtk::Widget = win_for_dispatch.clone().upcast();
                let Some(fields) = prompt_edit_perspective(&parent, None).await else {
                    return;
                };
                let Some(worker) = win_for_dispatch.worker() else {
                    return;
                };
                let renderer_field = if fields.renderer == "list" {
                    Some("list".to_string())
                } else {
                    Some("board".to_string())
                };
                let new = atrium_core::NewPerspective {
                    name: fields.name,
                    icon: None,
                    filter_expr: fields.filter_expr,
                    renderer: renderer_field,
                    renderer_config: fields.renderer_config,
                };
                if let Err(e) = worker.create_perspective(new).await {
                    error!(?e, "create_perspective failed");
                }
            });
        });

        let row_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(4)
            .margin_start(8)
            .margin_end(4)
            .margin_top(14)
            .margin_bottom(4)
            .build();
        row_box.append(&label);
        row_box.append(&add_button);

        gtk::ListBoxRow::builder()
            .child(&row_box)
            .selectable(false)
            .activatable(false)
            .build()
    }

    /// Phase 14 → v0.7.3 — saved perspective row context menu.
    ///
    /// v0.7.3 collapses the previous three menu items (Rename /
    /// Configure renderer / Delete) into two: **Edit…** (one
    /// dialog covering name + filter + renderer + columns) and
    /// **Delete**. The Edit dialog is `prompt_edit_perspective`,
    /// the same dialog the sidebar's "+" button uses for create
    /// mode. Delete remains on the shared `win.delete-active`
    /// action so the confirmation flow stays uniform across
    /// areas / projects / tags / perspectives.
    pub(super) fn install_perspective_context_menu(
        &self,
        row: &gtk::ListBoxRow,
        perspective_id: i64,
    ) {
        let menu = gio::Menu::new();
        menu.append(Some("Edit\u{2026}"), Some("win.edit-perspective"));
        menu.append(Some("Delete"), Some("win.delete-active"));
        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.set_has_arrow(false);
        popover.set_parent(row);
        unsafe {
            row.set_data("atrium-context-popover", popover.clone());
        }

        let gesture = gtk::GestureClick::new();
        gesture.set_button(gtk::gdk::BUTTON_SECONDARY);
        let win_weak = self.downgrade();
        gesture.connect_pressed(move |_, _, x, y| {
            let Some(win) = win_weak.upgrade() else {
                return;
            };
            win.set_active_list(ActiveList::Perspective(perspective_id));
            popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover.popup();
        });
        row.add_controller(gesture);
    }

    pub(super) fn install_drop_target_for_project(
        &self,
        row: &gtk::ListBoxRow,
        project_id: Option<i64>,
    ) {
        // Drop target accepts a task id; on drop, fires update_task
        // to move the task into this project (or to Inbox when
        // project_id is None — used for the Inbox row).
        let drop_target = gtk::DropTarget::new(i64::static_type(), gtk::gdk::DragAction::MOVE);
        let win_weak = self.downgrade();
        drop_target.connect_drop(move |_, value, _, _| {
            let Some(win) = win_weak.upgrade() else {
                return false;
            };
            if let Ok(task_id) = value.get::<i64>() {
                let Some(worker) = win.worker() else {
                    return false;
                };
                let target_project = project_id;
                glib::MainContext::default().spawn_local(async move {
                    if let Err(e) = worker
                        .update_task(TaskUpdate::new(task_id).project(target_project))
                        .await
                    {
                        error!(?e, task_id, ?target_project, "move-to-project failed");
                    }
                });
                return true;
            }
            false
        });
        row.add_controller(drop_target);
    }

    pub(super) fn build_sidebar(&self) {
        let list_box = self.imp().sidebar_list.clone();

        // Phase 4 baseline — canonical rows. `attach_data_layer`
        // appends area/project rows once the read pool is available.
        let mut targets: Vec<Option<ActiveList>> = Vec::new();
        let mut titles: Vec<Option<String>> = Vec::new();
        let mut badges: Vec<gtk::Label> = Vec::new();
        for active in CANONICAL_LISTS {
            let (row, badge) = build_canonical_row(active);
            // Inbox is special — accept dropped tasks to unfile them.
            if matches!(active, ActiveList::Inbox) {
                self.install_drop_target_for_project(&row, None);
            }
            list_box.append(&row);
            targets.push(Some(active.clone()));
            // Canonical rows are always visible regardless of filter —
            // tracked as None so `apply_sidebar_filter` skips them.
            titles.push(None);
            badges.push(badge);
        }
        self.imp().sidebar_targets.replace(targets);
        self.imp().sidebar_titles.replace(titles);
        self.imp().canonical_badges.replace(badges);

        // Phase 7e: filter entry above the list. Emits `search-changed`
        // with the native `search-delay` (100 ms) so we can debounce
        // for free.
        self.imp().sidebar_filter.connect_search_changed(clone!(
            #[weak(rename_to = win)]
            self,
            move |entry| {
                win.apply_sidebar_filter(&entry.text());
            }
        ));
        // Esc inside the entry clears the filter.
        self.imp().sidebar_filter.connect_stop_search(clone!(
            #[weak(rename_to = win)]
            self,
            move |entry| {
                entry.set_text("");
                win.apply_sidebar_filter("");
            }
        ));

        // Pre-select Today (index 1).
        if let Some(today_row) = list_box.row_at_index(1) {
            list_box.select_row(Some(&today_row));
        }

        list_box.connect_row_activated(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, row| {
                let idx = row.index() as usize;
                if let Some(Some(active)) = win.imp().sidebar_targets.borrow().get(idx).cloned() {
                    win.set_active_list(active);
                }
            }
        ));

        list_box.connect_selected_rows_changed(clone!(
            #[weak(rename_to = win)]
            self,
            move |list| {
                if let Some(row) = list.selected_row() {
                    let idx = row.index() as usize;
                    if let Some(Some(active)) = win.imp().sidebar_targets.borrow().get(idx).cloned()
                    {
                        win.set_active_list(active);
                    }
                }
            }
        ));
    }

    /// Re-read the count caches from the read pool. Cheap (six small
    /// SELECTs); called whenever a `TaskChanges` or `LibraryChanges`
    /// could have moved a count.
    pub(super) fn refresh_counts(&self) {
        let Some(pool) = self.read_pool() else {
            return;
        };
        let today = Local::now().date_naive();
        if let Ok(c) = pool.with(|conn| atrium_core::db::read::count_open_canonical(conn, today)) {
            *self.imp().canonical_counts.borrow_mut() = c;
        }
        if let Ok(c) = pool.with(atrium_core::db::read::count_open_per_project) {
            *self.imp().project_counts.borrow_mut() = c;
        }
        if let Ok(c) = pool.with(atrium_core::db::read::count_open_per_area) {
            *self.imp().area_counts.borrow_mut() = c;
        }
        if let Ok(c) = pool.with(atrium_core::db::read::count_open_per_tag) {
            *self.imp().tag_counts.borrow_mut() = c;
        }
    }

    /// Update canonical-row badges from `canonical_counts`. v0.6.16
    /// split out the Logbook badge — it lives in the trailing slot
    /// of `top_tier_extras` now and is tracked in `logbook_badge`
    /// rather than `canonical_badges`. Both still refresh here so
    /// callers don't need to remember the split.
    pub(super) fn refresh_canonical_badges(&self) {
        let counts = self.imp().canonical_counts.borrow().clone();
        let badges = self.imp().canonical_badges.borrow();
        let values = [
            counts.inbox,
            counts.today,
            counts.upcoming,
            counts.anytime,
            counts.someday,
        ];
        for (badge, n) in badges.iter().zip(values.iter()) {
            apply_badge_label(badge, *n);
        }
        if let Some(badge) = self.imp().logbook_badge.borrow().as_ref() {
            apply_badge_label(badge, counts.logbook);
        }
    }

    /// Update project / area / tag badges from the count caches.
    /// Phase 11 — in Builder Mode, sequential project badges show
    /// the *available* count instead of the open count: a sequential
    /// project with N open tasks has 1 available (the head row);
    /// a parallel project still shows N. Simple Mode shows open
    /// count regardless (Simple Mode hides the sequential toggle).
    pub(super) fn refresh_dynamic_badges(&self) {
        let builder = self.imp().current_mode_is_builder.get();
        let project_counts = self.imp().project_counts.borrow().clone();
        let project_meta = self.imp().project_meta.borrow().clone();
        for (pid, badge) in self.imp().project_badges.borrow().iter() {
            let open = project_counts.get(pid).copied().unwrap_or(0);
            let display = if builder {
                let sequential = project_meta.get(pid).is_some_and(|p| p.sequential);
                available_count(open, sequential)
            } else {
                open
            };
            apply_badge_label(badge, display);
        }
        let area_counts = self.imp().area_counts.borrow().clone();
        for (aid, badge) in self.imp().area_badges.borrow().iter() {
            let n = area_counts.get(aid).copied().unwrap_or(0);
            apply_badge_label(badge, n);
        }
        let tag_counts = self.imp().tag_counts.borrow().clone();
        for (tid, badge) in self.imp().tag_badges.borrow().iter() {
            let n = tag_counts.get(tid).copied().unwrap_or(0);
            apply_badge_label(badge, n);
        }
    }

    /// Append the Areas / Projects sections to the sidebar after the
    /// read pool is attached. Idempotent — clears any previously-added
    /// non-canonical rows first.
    /// Rebuild the dynamic sidebar (areas / projects / tags /
    /// perspectives + the top-tier extras) from the read pool.
    /// Public so the debug fixture-generation action in `main.rs`
    /// can poke the window to re-read the database after a
    /// fresh fixture insert.
    pub fn rebuild_dynamic_sidebar(&self) {
        // Refresh counts first so the canonical rows we rebuild use
        // current values.
        self.refresh_counts();
        self.refresh_canonical_badges();

        let Some(pool) = self.read_pool() else {
            return;
        };
        let list_box = self.imp().sidebar_list.clone();

        // Phase 8h — unparent any context-menu popovers stashed on
        // dynamic rows before we drop them. `set_parent(row)` makes
        // the popover a phantom child of the row outside the normal
        // child slot; if the row finalizes still parented, GTK warns
        // ~"Finalizing GtkListBoxRow … but it still has children
        // left: GtkPopoverMenu".
        self.unparent_sidebar_context_menus();

        // Trim back to just the canonical rows. CANONICAL_LISTS.len()
        // is the cutoff — anything past that is from a previous build.
        while list_box
            .row_at_index(CANONICAL_LISTS.len() as i32)
            .is_some()
        {
            if let Some(row) = list_box.row_at_index(CANONICAL_LISTS.len() as i32) {
                list_box.remove(&row);
            }
        }

        // Reset targets to just the canonical Some(...) entries.
        let mut targets: Vec<Option<ActiveList>> =
            CANONICAL_LISTS.iter().map(|a| Some(a.clone())).collect();
        // Parallel titles vec — None for the canonical rows
        // (always-visible), then None for section headers, Some(name)
        // for filterable area/project/tag rows. Phase 7e.
        let mut titles: Vec<Option<String>> = vec![None; CANONICAL_LISTS.len()];

        // v0.6.7 — top-tier rows. Agenda joins the canonical set in
        // both modes; Forecast and Review join only in Builder;
        // Logbook trails as the "completed past" bookend (v0.6.16
        // moved it here from CANONICAL_LISTS — see `top_tier_extras`).
        // No section header — these read as kindred to Inbox /
        // Today / etc., with their own accent tints (see
        // `canonical_accent_class` and `data/style.css`).
        let builder = self.imp().current_mode_is_builder.get();
        let mut new_logbook_badge: Option<gtk::Label> = None;
        for (active, label) in top_tier_extras(builder) {
            let (row, badge) = sidebar_row(icon_for(&active), label, 8);
            if let Some(class) = canonical_accent_class(&active) {
                row.add_css_class(class);
            }
            // v0.6.16 — Logbook's badge needs to update on TaskChanges
            // just like a canonical row. Stash it for
            // `refresh_canonical_badges` to find.
            if matches!(active, ActiveList::Logbook) {
                new_logbook_badge = Some(badge);
            }
            list_box.append(&row);
            targets.push(Some(active));
            titles.push(None); // top-tier rows don't filter
        }
        self.imp().logbook_badge.replace(new_logbook_badge);

        // v0.6.7 — Perspectives section moves up to right after
        // the top-tier group (was previously at the end of the
        // sidebar). Above Areas, below the Inbox group.
        // v0.7.3 — section header gains a trailing "+" affordance
        // that opens the perspective editor in create mode.
        let mut perspective_titles: HashMap<i64, String> = HashMap::new();
        let mut perspective_meta: HashMap<i64, atrium_core::Perspective> = HashMap::new();
        if builder {
            let perspectives = pool
                .with(atrium_core::db::read::list_perspectives)
                .unwrap_or_default();
            list_box.append(&self.build_perspectives_section_header());
            targets.push(None);
            titles.push(None);
            for p in &perspectives {
                perspective_titles.insert(p.id, p.name.clone());
                perspective_meta.insert(p.id, p.clone());
                let icon = p.icon.as_deref().unwrap_or("view-grid-symbolic");
                let (row, _badge) = sidebar_row(icon, &p.name, 8);
                self.install_perspective_context_menu(&row, p.id);
                list_box.append(&row);
                targets.push(Some(ActiveList::Perspective(p.id)));
                titles.push(Some(p.name.clone()));
            }
        }
        self.imp().perspective_titles.replace(perspective_titles);
        self.imp().perspective_meta.replace(perspective_meta);

        let areas = match pool.with(atrium_core::db::read::list_areas) {
            Ok(a) => a,
            Err(e) => {
                error!(?e, "failed to read areas for sidebar");
                self.imp().sidebar_targets.replace(targets);
                return;
            }
        };
        let projects = match pool.with(atrium_core::db::read::list_projects) {
            Ok(p) => p,
            Err(e) => {
                error!(?e, "failed to read projects for sidebar");
                self.imp().sidebar_targets.replace(targets);
                return;
            }
        };

        // Cache titles for content-pane resolution.
        let mut project_titles: HashMap<i64, String> = HashMap::new();
        let mut area_titles: HashMap<i64, String> = HashMap::new();
        let mut area_colors: HashMap<i64, Option<String>> = HashMap::new();
        let mut area_review_intervals: HashMap<i64, Option<i64>> = HashMap::new();
        for a in &areas {
            area_titles.insert(a.id, a.title.clone());
            area_colors.insert(a.id, a.color.clone());
            area_review_intervals.insert(a.id, a.default_review_interval_days);
        }
        for p in &projects {
            project_titles.insert(p.id, p.title.clone());
        }
        self.imp().area_titles.replace(area_titles);
        self.imp().area_colors.replace(area_colors);
        self.imp()
            .area_review_intervals
            .replace(area_review_intervals);
        self.imp().project_titles.replace(project_titles);

        // Group projects by area_id for nesting.
        let mut by_area: HashMap<Option<i64>, Vec<&Project>> = HashMap::new();
        for p in &projects {
            by_area.entry(p.area_id).or_default().push(p);
        }

        // Areas section
        let mut project_badges: HashMap<i64, gtk::Label> = HashMap::new();
        let mut area_badges: HashMap<i64, gtk::Label> = HashMap::new();
        if !areas.is_empty() {
            list_box.append(&build_section_header("Areas"));
            targets.push(None);
            titles.push(None);
            for area in &areas {
                let (row, badge) = build_area_row(area);
                self.install_area_context_menu(&row, area.id);
                list_box.append(&row);
                targets.push(Some(ActiveList::Area(area.id)));
                titles.push(Some(area.title.clone()));
                area_badges.insert(area.id, badge);
                if let Some(area_projects) = by_area.get(&Some(area.id)) {
                    for project in area_projects {
                        let (row, badge) = build_project_row(project, true);
                        self.install_drop_target_for_project(&row, Some(project.id));
                        self.install_project_context_menu(&row, project.id);
                        list_box.append(&row);
                        targets.push(Some(ActiveList::Project(project.id)));
                        titles.push(Some(project.title.clone()));
                        project_badges.insert(project.id, badge);
                    }
                }
            }
        }

        // Unfiled projects section
        if let Some(unfiled) = by_area.get(&None)
            && !unfiled.is_empty()
        {
            list_box.append(&build_section_header("Unfiled"));
            targets.push(None);
            titles.push(None);
            for project in unfiled {
                let (row, badge) = build_project_row(project, false);
                self.install_drop_target_for_project(&row, Some(project.id));
                self.install_project_context_menu(&row, project.id);
                list_box.append(&row);
                targets.push(Some(ActiveList::Project(project.id)));
                titles.push(Some(project.title.clone()));
                project_badges.insert(project.id, badge);
            }
        }
        self.imp().project_badges.replace(project_badges);
        self.imp().area_badges.replace(area_badges);

        // Tags section (Phase 6a — real now).
        let tags = pool
            .with(atrium_core::db::read::list_tags)
            .unwrap_or_default();
        let mut tag_titles: HashMap<i64, String> = HashMap::new();
        let mut tag_colors: HashMap<i64, Option<String>> = HashMap::new();
        let mut tag_badges: HashMap<i64, gtk::Label> = HashMap::new();
        if !tags.is_empty() {
            list_box.append(&build_section_header("Tags"));
            targets.push(None);
            titles.push(None);
            for tag in &tags {
                tag_titles.insert(tag.id, tag.name.clone());
                tag_colors.insert(tag.id, tag.color.clone());
                let (row, badge) = build_tag_row(tag);
                self.install_tag_context_menu(&row, tag.id);
                list_box.append(&row);
                targets.push(Some(ActiveList::Tag(tag.id)));
                titles.push(Some(tag.name.clone()));
                tag_badges.insert(tag.id, badge);
            }
        }
        self.imp().tag_titles.replace(tag_titles);
        self.imp().tag_colors.replace(tag_colors);
        self.imp().tag_badges.replace(tag_badges);

        // Cache project metadata so the project extras toolbar can
        // populate when a project view is selected.
        self.refresh_project_meta(&projects);

        self.imp().sidebar_targets.replace(targets);
        self.imp().sidebar_titles.replace(titles);
        self.refresh_dynamic_badges();

        // v0.2.2 — empty-library hint. Reveals only when there are
        // no areas, no projects, *and* no tags. Tags-only is a valid
        // workflow (capture-by-tag rather than capture-by-project)
        // so we don't pester the user when they've started with that
        // shape; areas-without-projects is unusual but treated as
        // "in progress" rather than empty.
        let library_empty = areas.is_empty() && projects.is_empty() && tags.is_empty();
        self.imp()
            .sidebar_empty_hint
            .set_reveal_child(library_empty);

        // Re-apply any active filter so the freshly-built rows respect
        // it (e.g., a tag rename that lands while a filter is typed).
        let query = self.imp().sidebar_filter.text().to_string();
        if !query.is_empty() {
            self.apply_sidebar_filter(&query);
        }
    }
}
