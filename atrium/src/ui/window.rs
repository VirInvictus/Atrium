// SPDX-License-Identifier: MIT
//! `AtriumWindow` — the application's `AdwApplicationWindow` subclass.
//!
//! Phase 4 turns the static sidebar / placeholder content from Phase 3
//! into a real working surface:
//!
//! - Sidebar is built programmatically so we can attach click handlers
//!   and (Phase 5+) count badges.
//! - Content pane hosts a `GtkStack` between an empty-state
//!   `AdwStatusPage` and a `GtkListView` rendering tasks via the
//!   `task_list` factory.
//! - `switch_to_list` re-populates the store from the read pool.
//! - `apply_task_changes` runs the diff applier on the active store
//!   when the worker emits a delta.
//!
//! Worker handle and read pool are pushed in from `main.rs` after
//! `boot_data_layer` succeeds, so the window can render even on a
//! fresh DB before any worker call has fired.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;

use adw::prelude::*;
use adw::subclass::prelude::*;
use atrium_core::db::read::CanonicalCounts;
use atrium_core::db::read_pool::ReadPool;
use atrium_core::{
    APP_ID, Area, AreaUpdate, LibraryChanges, NewArea, NewProject, NewTag, NewTask, Project,
    ProjectUpdate, Tag, TagUpdate, Task, TaskChanges, TaskUpdate, WorkerHandle,
};
use chrono::Local;
use gtk::glib::Propagation;
use gtk::glib::clone;
use gtk::{CompositeTemplate, gio, glib};
use tracing::{debug, error, warn};

use crate::ui::task_list::{
    ActiveList, TagMap, apply_changes, build_factory, replace_store_with_tags, sort_by_position,
};

mod imp {
    use super::*;

    #[derive(Default, CompositeTemplate)]
    #[template(file = "../../../data/window.ui")]
    pub struct AtriumWindow {
        #[template_child]
        pub split_view: TemplateChild<adw::NavigationSplitView>,
        #[template_child]
        pub menu_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub sidebar_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub content_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub content_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub task_list_view: TemplateChild<gtk::ListView>,
        #[template_child]
        pub content_status: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub new_task_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub new_task_entry: TemplateChild<gtk::Entry>,

        pub debug_enabled: Cell<bool>,
        pub active_list: Cell<ActiveList>,
        pub store: RefCell<Option<gio::ListStore>>,
        pub worker: OnceCell<WorkerHandle>,
        pub read_pool: OnceCell<ReadPool>,

        /// Aligned with the sidebar rows. `None` marks non-selectable
        /// header rows (e.g., "Areas", "Unfiled"); `Some(active)`
        /// dispatches to that list when the row is activated.
        pub sidebar_targets: RefCell<Vec<Option<ActiveList>>>,
        /// Project / area title caches populated when the sidebar is
        /// built; consulted by `set_active_list` to resolve the
        /// content-pane title for `Project(id)` / `Area(id)`.
        pub project_titles: RefCell<HashMap<i64, String>>,
        pub area_titles: RefCell<HashMap<i64, String>>,

        /// Open-task count caches (Phase 5c). Refreshed alongside the
        /// sidebar from `read::count_open_*`; the sidebar consumes
        /// these to render badges (hidden when zero).
        pub canonical_counts: RefCell<CanonicalCounts>,
        pub project_counts: RefCell<HashMap<i64, i64>>,
        pub area_counts: RefCell<HashMap<i64, i64>>,
        pub tag_counts: RefCell<HashMap<i64, i64>>,

        /// References to badge labels per row, so we can update them
        /// in place on `TaskChanges` without rebuilding the sidebar.
        /// Vec aligns with `CANONICAL_LISTS`; HashMaps key on row id.
        pub canonical_badges: RefCell<Vec<gtk::Label>>,
        pub project_badges: RefCell<HashMap<i64, gtk::Label>>,
        pub area_badges: RefCell<HashMap<i64, gtk::Label>>,
        pub tag_badges: RefCell<HashMap<i64, gtk::Label>>,

        /// Tag-name cache populated from `db::read::list_tags` when
        /// the sidebar is built. `set_active_list` consults it for
        /// `Tag(id)` content-pane titles.
        pub tag_titles: RefCell<HashMap<i64, String>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for AtriumWindow {
        const NAME: &'static str = "AtriumWindow";
        type Type = super::AtriumWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for AtriumWindow {
        fn constructed(&self) {
            self.parent_constructed();
            self.active_list.set(ActiveList::Today);

            let obj = self.obj();
            obj.bind_window_state();
            obj.install_menu();
            obj.build_sidebar();
            obj.init_list_view();
            obj.wire_new_task_entry();
            obj.install_window_actions();
        }
    }
    impl WidgetImpl for AtriumWindow {}
    impl WindowImpl for AtriumWindow {
        fn close_request(&self) -> Propagation {
            self.obj().save_window_state();
            self.parent_close_request()
        }
    }
    impl ApplicationWindowImpl for AtriumWindow {}
    impl AdwApplicationWindowImpl for AtriumWindow {}
}

glib::wrapper! {
    pub struct AtriumWindow(ObjectSubclass<imp::AtriumWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionGroup, gio::ActionMap;
}

const CANONICAL_LISTS: &[ActiveList] = &[
    ActiveList::Inbox,
    ActiveList::Today,
    ActiveList::Upcoming,
    ActiveList::Anytime,
    ActiveList::Someday,
    ActiveList::Logbook,
];

fn icon_for(list: ActiveList) -> &'static str {
    match list {
        ActiveList::Inbox => "inbox-symbolic",
        ActiveList::Today => "starred-symbolic",
        ActiveList::Upcoming => "x-office-calendar-symbolic",
        ActiveList::Anytime => "view-list-symbolic",
        ActiveList::Someday => "user-home-symbolic",
        ActiveList::Logbook => "document-open-recent-symbolic",
        ActiveList::Project(_) => "view-list-bullet-symbolic",
        ActiveList::Area(_) => "folder-symbolic",
        ActiveList::Tag(_) => "tag-outline-symbolic",
    }
}

impl AtriumWindow {
    pub fn new(app: &adw::Application, debug: bool) -> Self {
        let win: Self = glib::Object::builder().property("application", app).build();
        win.imp().debug_enabled.set(debug);
        if debug {
            win.install_menu();
        }
        win
    }

    fn settings(&self) -> gio::Settings {
        gio::Settings::new(APP_ID)
    }

    fn bind_window_state(&self) {
        let settings = self.settings();
        let width = settings.int("window-width");
        let height = settings.int("window-height");
        let maximized = settings.boolean("window-maximized");
        self.set_default_size(width, height);
        if maximized {
            self.maximize();
        }
        debug!(width, height, maximized, "restored window state");
    }

    fn save_window_state(&self) {
        let settings = self.settings();
        let (width, height) = self.default_size();
        let _ = settings.set_int("window-width", width);
        let _ = settings.set_int("window-height", height);
        let _ = settings.set_boolean("window-maximized", self.is_maximized());
    }

    fn install_menu(&self) {
        let menu = build_primary_menu(self.imp().debug_enabled.get());
        self.imp().menu_button.set_menu_model(Some(&menu));
    }

    /// Attach a right-click context menu to a project row. The menu
    /// targets `win.*` actions which consult `active_list()`, so we
    /// set the row's project as active before popping the menu —
    /// otherwise Rename / Delete / Archive would operate on whatever
    /// list was selected before the right-click.
    fn install_project_context_menu(&self, row: &gtk::ListBoxRow, project_id: i64) {
        let menu = gio::Menu::new();
        menu.append(Some("Rename"), Some("win.rename-active"));
        menu.append(Some("Archive"), Some("win.archive-active-project"));
        menu.append(Some("Delete"), Some("win.delete-active"));
        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.set_has_arrow(false);
        popover.set_parent(row);

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
    fn install_tag_context_menu(&self, row: &gtk::ListBoxRow, tag_id: i64) {
        let menu = gio::Menu::new();
        menu.append(Some("Rename"), Some("win.rename-active"));
        menu.append(Some("Delete"), Some("win.delete-active"));
        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.set_has_arrow(false);
        popover.set_parent(row);

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
    fn install_area_context_menu(&self, row: &gtk::ListBoxRow, area_id: i64) {
        let menu = gio::Menu::new();
        menu.append(Some("Rename"), Some("win.rename-active"));
        menu.append(Some("Delete"), Some("win.delete-active"));
        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.set_has_arrow(false);
        popover.set_parent(row);

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

    fn install_drop_target_for_project(&self, row: &gtk::ListBoxRow, project_id: Option<i64>) {
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

    fn build_sidebar(&self) {
        let list_box = self.imp().sidebar_list.clone();

        // Phase 4 baseline — canonical rows. `attach_data_layer`
        // appends area/project rows once the read pool is available.
        let mut targets: Vec<Option<ActiveList>> = Vec::new();
        let mut badges: Vec<gtk::Label> = Vec::new();
        for active in CANONICAL_LISTS {
            let (row, badge) = build_canonical_row(*active);
            // Inbox is special — accept dropped tasks to unfile them.
            if matches!(active, ActiveList::Inbox) {
                self.install_drop_target_for_project(&row, None);
            }
            list_box.append(&row);
            targets.push(Some(*active));
            badges.push(badge);
        }
        self.imp().sidebar_targets.replace(targets);
        self.imp().canonical_badges.replace(badges);

        // Pre-select Today (index 1).
        if let Some(today_row) = list_box.row_at_index(1) {
            list_box.select_row(Some(&today_row));
        }

        list_box.connect_row_activated(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, row| {
                let idx = row.index() as usize;
                if let Some(Some(active)) = win.imp().sidebar_targets.borrow().get(idx).copied() {
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
                    if let Some(Some(active)) = win.imp().sidebar_targets.borrow().get(idx).copied()
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
    fn refresh_counts(&self) {
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

    /// Update canonical-row badges from `canonical_counts`.
    fn refresh_canonical_badges(&self) {
        let counts = self.imp().canonical_counts.borrow().clone();
        let badges = self.imp().canonical_badges.borrow();
        let values = [
            counts.inbox,
            counts.today,
            counts.upcoming,
            counts.anytime,
            counts.someday,
            counts.logbook,
        ];
        for (badge, n) in badges.iter().zip(values.iter()) {
            apply_badge_label(badge, *n);
        }
    }

    /// Update project / area / tag badges from the count caches.
    fn refresh_dynamic_badges(&self) {
        let project_counts = self.imp().project_counts.borrow().clone();
        for (pid, badge) in self.imp().project_badges.borrow().iter() {
            let n = project_counts.get(pid).copied().unwrap_or(0);
            apply_badge_label(badge, n);
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
    fn rebuild_dynamic_sidebar(&self) {
        // Refresh counts first so the canonical rows we rebuild use
        // current values.
        self.refresh_counts();
        self.refresh_canonical_badges();

        let Some(pool) = self.read_pool() else {
            return;
        };
        let list_box = self.imp().sidebar_list.clone();

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
            CANONICAL_LISTS.iter().map(|a| Some(*a)).collect();

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
        for a in &areas {
            area_titles.insert(a.id, a.title.clone());
        }
        for p in &projects {
            project_titles.insert(p.id, p.title.clone());
        }
        self.imp().area_titles.replace(area_titles);
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
            for area in &areas {
                let (row, badge) = build_area_row(area);
                self.install_area_context_menu(&row, area.id);
                list_box.append(&row);
                targets.push(Some(ActiveList::Area(area.id)));
                area_badges.insert(area.id, badge);
                if let Some(area_projects) = by_area.get(&Some(area.id)) {
                    for project in area_projects {
                        let (row, badge) = build_project_row(project, true);
                        self.install_drop_target_for_project(&row, Some(project.id));
                        self.install_project_context_menu(&row, project.id);
                        list_box.append(&row);
                        targets.push(Some(ActiveList::Project(project.id)));
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
            for project in unfiled {
                let (row, badge) = build_project_row(project, false);
                self.install_drop_target_for_project(&row, Some(project.id));
                self.install_project_context_menu(&row, project.id);
                list_box.append(&row);
                targets.push(Some(ActiveList::Project(project.id)));
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
        let mut tag_badges: HashMap<i64, gtk::Label> = HashMap::new();
        if !tags.is_empty() {
            list_box.append(&build_section_header("Tags"));
            targets.push(None);
            for tag in &tags {
                tag_titles.insert(tag.id, tag.name.clone());
                let (row, badge) = build_tag_row(tag);
                self.install_tag_context_menu(&row, tag.id);
                list_box.append(&row);
                targets.push(Some(ActiveList::Tag(tag.id)));
                tag_badges.insert(tag.id, badge);
            }
        }
        self.imp().tag_titles.replace(tag_titles);
        self.imp().tag_badges.replace(tag_badges);

        self.imp().sidebar_targets.replace(targets);
        self.refresh_dynamic_badges();
    }

    fn init_list_view(&self) {
        let store = gio::ListStore::new::<crate::ui::task_object::AtriumTask>();
        self.imp().store.replace(Some(store.clone()));

        let selection = gtk::SingleSelection::new(Some(store.clone()));
        selection.set_autoselect(false);
        selection.set_can_unselect(true);
        self.imp().task_list_view.set_model(Some(&selection));

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
        let factory = build_factory(on_toggle, on_rename, on_reorder);
        self.imp().task_list_view.set_factory(Some(&factory));
    }

    /// Push the worker handle / read pool into the window after the
    /// data layer boots.
    pub fn attach_data_layer(&self, worker: WorkerHandle, read_pool: ReadPool) {
        let _ = self.imp().worker.set(worker);
        let _ = self.imp().read_pool.set(read_pool);
        // Append the Areas / Projects sections to the sidebar.
        self.rebuild_dynamic_sidebar();
        // Initial content-pane load now that the read pool exists.
        self.refresh_active_list();
    }

    fn worker(&self) -> Option<WorkerHandle> {
        self.imp().worker.get().cloned()
    }

    /// Public accessor for the worker handle so non-window
    /// surfaces (Quick Entry modal in Phase 6c) can dispatch
    /// commands without round-tripping through window methods.
    pub fn worker_handle_for_quickentry(&self) -> Option<WorkerHandle> {
        self.imp().worker.get().cloned()
    }

    fn read_pool(&self) -> Option<ReadPool> {
        self.imp().read_pool.get().cloned()
    }

    pub fn set_active_list(&self, active: ActiveList) {
        if self.imp().active_list.get() == active {
            return;
        }
        self.imp().active_list.set(active);
        self.imp().content_page.set_title(&self.title_for(active));
        self.refresh_active_list();
    }

    /// Resolve the human-readable title for a given active list.
    /// Canonical lists return their static label; `Project(id)` and
    /// `Area(id)` consult the title caches populated when the sidebar
    /// was built.
    fn title_for(&self, active: ActiveList) -> String {
        match active {
            ActiveList::Project(id) => self
                .imp()
                .project_titles
                .borrow()
                .get(&id)
                .cloned()
                .unwrap_or_else(|| "Project".into()),
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
                .map(|n| format!("#{n}"))
                .unwrap_or_else(|| "Tag".into()),
            other => other.canonical_title().to_string(),
        }
    }

    pub fn active_list(&self) -> ActiveList {
        self.imp().active_list.get()
    }

    pub fn refresh_active_list(&self) {
        let Some(store) = self.imp().store.borrow().clone() else {
            return;
        };
        let Some(pool) = self.read_pool() else {
            // Read pool not attached yet — show empty state, will refresh on attach.
            store.remove_all();
            self.update_empty_state(&store);
            return;
        };

        let active = self.active_list();
        let today = Local::now().date_naive();

        let result: Result<Vec<Task>, _> = pool.with(|conn| match active {
            ActiveList::Inbox => atrium_core::db::read::list_inbox(conn),
            ActiveList::Today => atrium_core::db::read::list_today(conn, today),
            ActiveList::Upcoming => atrium_core::db::read::list_upcoming(conn, today),
            ActiveList::Anytime => atrium_core::db::read::list_anytime(conn, today),
            ActiveList::Someday => atrium_core::db::read::list_someday(conn),
            ActiveList::Logbook => atrium_core::db::read::list_logbook(conn),
            ActiveList::Project(id) => atrium_core::db::read::list_project(conn, id),
            ActiveList::Area(id) => atrium_core::db::read::list_area(conn, id),
            ActiveList::Tag(id) => atrium_core::db::read::list_tasks_with_tag(conn, id),
        });

        match result {
            Ok(tasks) => {
                let tag_map: TagMap = pool
                    .with(atrium_core::db::read::tag_names_per_task)
                    .unwrap_or_default();
                replace_store_with_tags(&store, &tasks, &tag_map);
                sort_by_position(&store);
            }
            Err(e) => {
                error!(?e, ?active, "failed to load active list");
                store.remove_all();
            }
        }
        self.update_empty_state(&store);
    }

    /// React to a library-level delta. The sidebar rebuilds from
    /// scratch (small enough; rare events); the active selection is
    /// re-established afterward by walking the freshly-built
    /// `sidebar_targets` for a matching `ActiveList`. Phase 5.5
    /// polish — keeps the highlight where the user left it instead
    /// of dropping back to Today on every rename / new / move.
    pub fn apply_library_changes(&self, changes: &LibraryChanges) {
        let active = self.active_list();
        let deleted = match active {
            ActiveList::Project(id) => changes.projects_deleted.contains(&id),
            ActiveList::Area(id) => changes.areas_deleted.contains(&id),
            _ => false,
        };
        self.rebuild_dynamic_sidebar();
        if deleted {
            // Active entity is gone — fall back to Today.
            self.set_active_list(ActiveList::Today);
            self.select_sidebar_row_for(ActiveList::Today);
        } else {
            self.select_sidebar_row_for(active);
            self.refresh_active_list();
        }
    }

    /// Find the sidebar row whose target equals `active` and select
    /// it. Used after `rebuild_dynamic_sidebar` to preserve the
    /// user's selection across rebuilds.
    fn select_sidebar_row_for(&self, active: ActiveList) {
        let targets = self.imp().sidebar_targets.borrow();
        for (i, t) in targets.iter().enumerate() {
            if *t == Some(active)
                && let Some(row) = self.imp().sidebar_list.row_at_index(i as i32)
            {
                self.imp().sidebar_list.select_row(Some(&row));
                return;
            }
        }
    }

    pub fn apply_task_changes(&self, changes: &TaskChanges) {
        let Some(store) = self.imp().store.borrow().clone() else {
            return;
        };
        let active = self.active_list();
        let today = Local::now().date_naive();
        // Re-load tag map so the diff applier renders updated pills.
        let tag_map: TagMap = self
            .read_pool()
            .and_then(|p| p.with(atrium_core::db::read::tag_names_per_task).ok())
            .unwrap_or_default();
        apply_changes(&store, changes, active, today, &tag_map);
        self.update_empty_state(&store);
        // Phase 5c: any task delta might have moved a count.
        self.refresh_counts();
        self.refresh_canonical_badges();
        self.refresh_dynamic_badges();
    }

    fn update_empty_state(&self, store: &gio::ListStore) {
        let active = self.active_list();
        let stack = self.imp().content_stack.clone();
        let status = self.imp().content_status.clone();

        if store.n_items() == 0 {
            let (title, description) = self.empty_state_copy(active);
            status.set_title(&title);
            status.set_description(Some(&description));
            status.set_icon_name(Some(icon_for(active)));
            stack.set_visible_child_name("empty");
        } else {
            stack.set_visible_child_name("list");
        }
    }

    fn empty_state_copy(&self, active: ActiveList) -> (String, String) {
        match active {
            ActiveList::Inbox => (
                "Inbox is empty".into(),
                "Press Ctrl+N or use the entry below to capture a task.".into(),
            ),
            ActiveList::Today => (
                "Nothing today".into(),
                "Schedule tasks for today or check Upcoming for what's next.".into(),
            ),
            ActiveList::Upcoming => (
                "Nothing upcoming".into(),
                "Tasks scheduled for future dates appear here.".into(),
            ),
            ActiveList::Anytime => (
                "No anytime tasks".into(),
                "Open tasks without a scheduled date land here.".into(),
            ),
            ActiveList::Someday => (
                "Someday is empty".into(),
                "Park ideas here when you don't want a date attached.".into(),
            ),
            ActiveList::Logbook => (
                "Logbook is empty".into(),
                "Completed tasks accumulate here, newest first.".into(),
            ),
            ActiveList::Project(_) => (
                format!("{} is empty", self.title_for(active)),
                "Add tasks to this project from the entry below.".into(),
            ),
            ActiveList::Area(_) => (
                format!("{} is empty", self.title_for(active)),
                "No open tasks across this area's projects.".into(),
            ),
            ActiveList::Tag(_) => (
                format!("{} is empty", self.title_for(active)),
                "No open tasks bear this tag.".into(),
            ),
        }
    }

    /// Toggle handler — fires the worker call. The worker emits a
    /// `TaskChanges` delta which the bridge applies; we don't update
    /// the model here.
    fn handle_toggle(&self, id: i64, _want: bool) {
        let Some(worker) = self.worker() else {
            warn!("worker not attached; toggle ignored");
            return;
        };
        glib::MainContext::default().spawn_local(async move {
            if let Err(e) = worker.toggle_complete(id).await {
                error!(?e, id, "toggle_complete failed");
            }
        });
    }

    /// Rename handler — fires `update_task` with the new title.
    fn handle_rename(&self, id: i64, new_title: String) {
        let Some(worker) = self.worker() else {
            warn!("worker not attached; rename ignored");
            return;
        };
        glib::MainContext::default().spawn_local(async move {
            if let Err(e) = worker
                .update_task(TaskUpdate::new(id).title(new_title))
                .await
            {
                error!(?e, id, "update_task failed");
            }
        });
    }

    /// Reorder handler — drag-and-drop drops `src_id` onto `dest_id`.
    /// Computes a midpoint position so `src` ends up adjacent to
    /// `dest`, then fires a single `update_task` with the new
    /// position. Active store re-sorts via `apply_changes` after the
    /// worker round-trip.
    fn handle_reorder(&self, src_id: i64, dest_id: i64) {
        if src_id == dest_id {
            return;
        }
        // Drag-reorder is only meaningful for Inbox in Phase 4 — the
        // other lists either auto-sort by date (Today, Upcoming) or
        // aren't user-orderable yet. Silently skip elsewhere.
        if !matches!(self.active_list(), ActiveList::Inbox) {
            return;
        }

        let Some(store) = self.imp().store.borrow().clone() else {
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
        let (Some(&(_, _, src_pos)), Some(&(dest_idx, _, dest_pos))) = (src, dest) else {
            return;
        };

        // Compute the new position. If src is moving DOWN past dest,
        // it lands between dest and the next neighbour after dest.
        // If moving UP, it lands between the row before dest and dest.
        let new_position = if src_pos < dest_pos {
            let next_pos = entries
                .iter()
                .find(|(i, _, _)| *i == dest_idx + 1)
                .map(|(_, _, p)| *p)
                .unwrap_or(dest_pos + 1.0);
            (dest_pos + next_pos) / 2.0
        } else {
            let prev_pos = if dest_idx == 0 {
                dest_pos - 1.0
            } else {
                entries
                    .iter()
                    .find(|(i, _, _)| *i == dest_idx - 1)
                    .map(|(_, _, p)| *p)
                    .unwrap_or(dest_pos - 1.0)
            };
            (prev_pos + dest_pos) / 2.0
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

    /// Create with the given title — fired by the bottom-of-list entry.
    /// Phase 6b: parses inline `#tag` / `@today` / `@yyyy-mm-dd` /
    /// `@deadline yyyy-mm-dd` syntax via `quickentry::parser` and
    /// applies the metadata to the new task.
    fn create_task_with_title(&self, raw_input: String) {
        let Some(worker) = self.worker() else {
            warn!("worker not attached; new-task ignored");
            return;
        };
        let active = self.active_list();
        let parsed = crate::quickentry::parser::parse(&raw_input);
        if parsed.title.is_empty() && parsed.tag_names.is_empty() {
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
                    if !parsed.tag_names.is_empty() {
                        // Resolve tag names → ids, creating any new
                        // tags via `ensure_tag`. Run sequentially so
                        // we collect ids before SetTaskTags fires.
                        let mut tag_ids = Vec::with_capacity(parsed.tag_names.len());
                        for name in parsed.tag_names {
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

    /// Delete handler — operates on the focused list row.
    pub fn delete_focused_task(&self) {
        let Some(id) = self.focused_task_id() else {
            return;
        };
        let Some(worker) = self.worker() else { return };
        glib::MainContext::default().spawn_local(async move {
            if let Err(e) = worker.delete_task(id).await {
                error!(?e, id, "delete_task failed");
            }
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

    fn focused_task_id(&self) -> Option<i64> {
        let model = self.imp().task_list_view.model()?;
        let selection = model.downcast_ref::<gtk::SingleSelection>()?;
        let pos = selection.selected();
        if pos == gtk::INVALID_LIST_POSITION {
            return None;
        }
        let obj = selection.item(pos)?;
        let task = obj.downcast_ref::<crate::ui::task_object::AtriumTask>()?;
        Some(task.id())
    }

    fn wire_new_task_entry(&self) {
        let entry = self.imp().new_task_entry.clone();
        entry.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |entry| {
                let title = entry.text().to_string();
                let trimmed = title.trim();
                if trimmed.is_empty() {
                    return;
                }
                win.create_task_with_title(trimmed.to_string());
                entry.set_text("");
            }
        ));
    }

    /// Focus the bottom-of-list entry. The Ctrl+N action targets this
    /// instead of immediately spawning a "New task" title — the
    /// Things-3 idiom is "type the title first, hit Enter to commit".
    pub fn focus_new_task_entry(&self) {
        self.imp().new_task_entry.grab_focus();
    }

    fn install_window_actions(&self) {
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

        let archive = gio::SimpleAction::new("archive-active-project", None);
        archive.connect_activate(clone!(
            #[weak(rename_to = win)]
            self,
            move |_, _| win.archive_active_project()
        ));
        self.add_action(&archive);
    }

    /// Prompt for an area name and create it. Used by the
    /// `app.new-area` action.
    pub fn prompt_create_area(&self) {
        let win = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let Some(title) = prompt_for_text(&win, "New Area", "Area name", "", "Create").await
            else {
                return;
            };
            let Some(worker) = win.worker() else { return };
            if let Err(e) = worker.create_area(NewArea { title }).await {
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
            let area_id = match win.active_list() {
                ActiveList::Area(id) => Some(id),
                ActiveList::Project(id) => {
                    // If the user is in a project, default the new
                    // project to the same area.
                    win.imp().project_titles.borrow().get(&id).and(None) // We don't cache area_id on projects yet — leave unfiled.
                }
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

    fn prompt_rename_active(&self) {
        let active = self.active_list();
        let win = self.clone();
        match active {
            ActiveList::Area(id) => {
                let current = self
                    .imp()
                    .area_titles
                    .borrow()
                    .get(&id)
                    .cloned()
                    .unwrap_or_default();
                glib::MainContext::default().spawn_local(async move {
                    let Some(title) =
                        prompt_for_text(&win, "Rename Area", "Area name", &current, "Rename").await
                    else {
                        return;
                    };
                    let Some(worker) = win.worker() else { return };
                    if let Err(e) = worker.update_area(AreaUpdate::new(id).title(title)).await {
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
                let current = self
                    .imp()
                    .tag_titles
                    .borrow()
                    .get(&id)
                    .cloned()
                    .unwrap_or_default();
                glib::MainContext::default().spawn_local(async move {
                    let Some(name) =
                        prompt_for_text(&win, "Rename Tag", "Tag name", &current, "Rename").await
                    else {
                        return;
                    };
                    let Some(worker) = win.worker() else { return };
                    if let Err(e) = worker.update_tag(TagUpdate::new(id).name(name)).await {
                        error!(?e, id, "update_tag failed");
                    }
                });
            }
            _ => {
                debug!("rename-active: nothing to rename in canonical list");
            }
        }
    }

    fn prompt_delete_active(&self) {
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
            _ => {
                debug!("delete-active: nothing to delete in canonical list");
            }
        }
    }

    pub fn prompt_create_tag(&self) {
        let win = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let Some(name) = prompt_for_text(&win, "New Tag", "Tag name", "", "Create").await
            else {
                return;
            };
            let Some(worker) = win.worker() else { return };
            if let Err(e) = worker.create_tag(NewTag { name, color: None }).await {
                error!(?e, "create_tag failed");
            }
        });
    }

    fn archive_active_project(&self) {
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
            self.set_active_list(*active);
            if let Some(row) = self.imp().sidebar_list.row_at_index(idx as i32) {
                self.imp().sidebar_list.select_row(Some(&row));
            }
        }
    }
}

/// Build the primary (hamburger) menu. `include_debug` adds the
/// fixture-generator submenu for `--debug` runs.
pub(crate) fn build_primary_menu(include_debug: bool) -> gio::Menu {
    let menu = gio::Menu::new();

    let new_section = gio::Menu::new();
    new_section.append(Some("New Task"), Some("app.new-task"));
    new_section.append(Some("Quick Entry"), Some("app.quick-entry"));
    new_section.append(Some("New Project"), Some("app.new-project"));
    new_section.append(Some("New Area"), Some("app.new-area"));
    new_section.append(Some("New Tag"), Some("app.new-tag"));
    menu.append_section(None, &new_section);

    let library_section = gio::Menu::new();
    library_section.append(Some("Rename Active"), Some("win.rename-active"));
    library_section.append(Some("Archive Project"), Some("win.archive-active-project"));
    library_section.append(Some("Delete Active"), Some("win.delete-active"));
    menu.append_section(None, &library_section);

    let mode_section = gio::Menu::new();
    let mode_submenu = gio::Menu::new();
    mode_submenu.append(Some("Simple"), Some("app.mode::simple"));
    mode_submenu.append(Some("Builder"), Some("app.mode::builder"));
    mode_section.append_submenu(Some("Mode"), &mode_submenu);
    menu.append_section(None, &mode_section);

    if include_debug {
        let debug_section = gio::Menu::new();
        let debug_submenu = gio::Menu::new();

        let fixture_submenu = gio::Menu::new();
        fixture_submenu.append(Some("Small (1K tasks)"), Some("app.fixture::small"));
        fixture_submenu.append(Some("Medium (10K tasks)"), Some("app.fixture::medium"));
        fixture_submenu.append(Some("Large (50K tasks)"), Some("app.fixture::large"));
        fixture_submenu.append(Some("Stress (100K tasks)"), Some("app.fixture::stress"));
        debug_submenu.append_submenu(Some("Generate Fixtures"), &fixture_submenu);

        debug_section.append_submenu(Some("Debug"), &debug_submenu);
        menu.append_section(None, &debug_section);
    }

    let about_section = gio::Menu::new();
    about_section.append(Some("Keyboard Shortcuts"), Some("app.show-shortcuts"));
    about_section.append(Some("About Atrium"), Some("app.about"));
    about_section.append(Some("Quit"), Some("app.quit"));
    menu.append_section(None, &about_section);

    menu
}

/// Open a small `AdwAlertDialog` with a text entry. Returns the
/// trimmed entered text on the configured-action response, or `None`
/// on cancel / empty input.
async fn prompt_for_text(
    parent: &impl IsA<gtk::Widget>,
    heading: &str,
    placeholder: &str,
    initial: &str,
    confirm_label: &str,
) -> Option<String> {
    let entry = gtk::Entry::builder()
        .placeholder_text(placeholder)
        .text(initial)
        .activates_default(true)
        .build();

    let dialog = adw::AlertDialog::new(Some(heading), None);
    dialog.set_extra_child(Some(&entry));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("ok", confirm_label);
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("cancel");
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);

    let response = dialog.choose_future(parent).await;
    if response.as_str() == "ok" {
        let text = entry.text().to_string().trim().to_string();
        if text.is_empty() { None } else { Some(text) }
    } else {
        None
    }
}

/// Confirm a destructive action via `AdwAlertDialog`. Returns `true`
/// only if the user explicitly confirmed.
async fn prompt_confirm_destructive(
    parent: &impl IsA<gtk::Widget>,
    heading: &str,
    body: &str,
    destructive_label: &str,
) -> bool {
    let dialog = adw::AlertDialog::new(Some(heading), Some(body));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("destroy", destructive_label);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.set_response_appearance("destroy", adw::ResponseAppearance::Destructive);

    let response = dialog.choose_future(parent).await;
    response.as_str() == "destroy"
}

fn build_canonical_row(active: ActiveList) -> (gtk::ListBoxRow, gtk::Label) {
    sidebar_row(icon_for(active), active.canonical_title(), 8)
}

fn build_area_row(area: &Area) -> (gtk::ListBoxRow, gtk::Label) {
    let (row, badge) = sidebar_row(icon_for(ActiveList::Area(area.id)), &area.title, 8);
    // The label inside the row gets a `heading` class so areas read
    // bolder than projects.
    if let Some(label) = row
        .child()
        .and_downcast::<gtk::Box>()
        .and_then(|b| b.first_child())
        .and_then(|icon| icon.next_sibling())
        .and_downcast::<gtk::Label>()
    {
        label.add_css_class("heading");
    }
    (row, badge)
}

fn build_project_row(project: &Project, indented: bool) -> (gtk::ListBoxRow, gtk::Label) {
    let margin = if indented { 24 } else { 8 };
    sidebar_row(
        icon_for(ActiveList::Project(project.id)),
        &project.title,
        margin,
    )
}

fn build_tag_row(tag: &Tag) -> (gtk::ListBoxRow, gtk::Label) {
    sidebar_row(icon_for(ActiveList::Tag(tag.id)), &tag.name, 8)
}

fn build_section_header(label: &str) -> gtk::ListBoxRow {
    let l = gtk::Label::builder()
        .label(label)
        .halign(gtk::Align::Start)
        .margin_start(8)
        .margin_end(8)
        .margin_top(10)
        .margin_bottom(4)
        .build();
    l.add_css_class("dim-label");
    l.add_css_class("caption-heading");
    gtk::ListBoxRow::builder()
        .child(&l)
        .selectable(false)
        .activatable(false)
        .build()
}

fn sidebar_row(icon: &str, label: &str, margin_start: i32) -> (gtk::ListBoxRow, gtk::Label) {
    let icon_widget = gtk::Image::from_icon_name(icon);
    let label_widget = gtk::Label::builder()
        .label(label)
        .halign(gtk::Align::Start)
        .hexpand(true)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .build();

    let badge = gtk::Label::builder().visible(false).build();
    badge.add_css_class("dim-label");
    badge.add_css_class("numeric");

    let row_box = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_start(margin_start)
        .margin_end(8)
        .margin_top(6)
        .margin_bottom(6)
        .build();
    row_box.append(&icon_widget);
    row_box.append(&label_widget);
    row_box.append(&badge);

    let row = gtk::ListBoxRow::builder().child(&row_box).build();
    (row, badge)
}

/// Set a badge label's text from a count, hiding when zero.
fn apply_badge_label(badge: &gtk::Label, count: i64) {
    if count > 0 {
        badge.set_label(&count.to_string());
        badge.set_visible(true);
    } else {
        badge.set_visible(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_menu_has_four_sections_no_debug() {
        let menu = build_primary_menu(false);
        // New + Library + Mode + About sections.
        assert_eq!(menu.n_items(), 4);
    }

    #[test]
    fn primary_menu_includes_debug_section_when_enabled() {
        let menu = build_primary_menu(true);
        // New + Library + Mode + Debug + About sections.
        assert_eq!(menu.n_items(), 5);
    }

    #[test]
    fn sidebar_lists_cover_simple_mode() {
        assert_eq!(CANONICAL_LISTS.len(), 6);
        assert!(CANONICAL_LISTS.contains(&ActiveList::Inbox));
        assert!(CANONICAL_LISTS.contains(&ActiveList::Today));
        assert!(CANONICAL_LISTS.contains(&ActiveList::Logbook));
    }
}
