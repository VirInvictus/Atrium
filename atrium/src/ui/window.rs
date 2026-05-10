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
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use atrium_core::db::read::CanonicalCounts;
use atrium_core::db::read_pool::ReadPool;
use atrium_core::{
    APP_ID, Area, AreaUpdate, LibraryChanges, NewArea, NewPerspective, NewProject, NewTag, NewTask,
    PerspectiveUpdate, Project, ProjectUpdate, Tag, TagUpdate, Task, TaskChanges, TaskUpdate,
    WorkerHandle,
};
use chrono::Local;
use gtk::glib::Propagation;
use gtk::glib::clone;
use gtk::{CompositeTemplate, gio, glib};
use tracing::{debug, error, warn};

use crate::ui::task_list::{
    ActiveList, TagMap, build_factory, replace_store_with_tags_seq, sort_by_position,
};

/// Shared cell used by both the undo toast button and the `Ctrl+Z`
/// accel (Phase 7f). The inner `Option` is the still-alive callback
/// (consumed by whichever path fires first); the outer level lets
/// the cell be replaced wholesale every time `show_undo_toast` runs.
type UndoCell = Rc<RefCell<Option<Box<dyn FnOnce()>>>>;

mod imp {
    use super::*;

    #[derive(Default, CompositeTemplate)]
    #[template(file = "../../../data/window.ui")]
    pub struct AtriumWindow {
        #[template_child]
        pub overlay_split: TemplateChild<adw::OverlaySplitView>,
        #[template_child]
        pub inspector_pane_host: TemplateChild<adw::Bin>,
        #[template_child]
        pub split_view: TemplateChild<adw::NavigationSplitView>,
        #[template_child]
        pub menu_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub sidebar_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub sidebar_filter: TemplateChild<gtk::SearchEntry>,
        /// v0.2.2 — empty-library hint. Reveals when no areas /
        /// projects / tags exist; the embedded button dispatches
        /// `app.new-project` to bootstrap the first project.
        #[template_child]
        pub sidebar_empty_hint: TemplateChild<gtk::Revealer>,
        #[template_child]
        pub content_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub content_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub task_list_view: TemplateChild<gtk::ListView>,
        #[template_child]
        pub content_status: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub forecast_host: TemplateChild<adw::Bin>,
        #[template_child]
        pub review_host: TemplateChild<adw::Bin>,
        /// v0.6.0 (Slice C2) — Logbook page host. Window mounts the
        /// day-band layout from `logbook::build_page` here whenever
        /// `ActiveList::Logbook` is selected.
        #[template_child]
        pub logbook_host: TemplateChild<adw::Bin>,
        /// v0.6.0 (Slice D1 GUI) — kanban board page host. Window
        /// mounts the column layout from `board::build_page` here
        /// whenever the active Perspective has `renderer = "board"`.
        #[template_child]
        pub board_host: TemplateChild<adw::Bin>,
        /// v0.6.4 (Slice D2) — Agenda canonical page host. Window
        /// mounts the chronological-section layout from
        /// `agenda::build_page` here whenever `ActiveList::Agenda`
        /// is selected.
        #[template_child]
        pub agenda_host: TemplateChild<adw::Bin>,
        /// Phase 12.5 (v0.11.0) — Calendar Month View host.
        /// Window mounts the 7×N grid from `calendar::build_page`
        /// here whenever `ActiveList::Calendar` is selected.
        /// Builder-only.
        #[template_child]
        pub calendar_host: TemplateChild<adw::Bin>,
        // v0.7.0 — magazine-spread page title strip that lives
        // between the header bar and the content stack. Bound in
        // set_active_list (big label = view title, subtitle =
        // optional supporting line; subtitle is hidden when empty).
        #[template_child]
        pub page_title_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub page_subtitle_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub new_task_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub new_task_entry: TemplateChild<gtk::Entry>,
        #[template_child]
        pub search_button: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub search_bar: TemplateChild<gtk::SearchBar>,
        #[template_child]
        pub search_entry: TemplateChild<gtk::SearchEntry>,
        /// v0.4.1 — `?` button at the right end of the search bar;
        /// hosts the operator-reference popover built in
        /// `wire_search_bar`.
        #[template_child]
        pub search_help_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub selection_revealer: TemplateChild<gtk::Revealer>,
        #[template_child]
        pub selection_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub project_extras_revealer: TemplateChild<gtk::Revealer>,
        #[template_child]
        pub project_sequential_switch: TemplateChild<gtk::Switch>,
        #[template_child]
        pub project_review_spin: TemplateChild<gtk::SpinButton>,

        pub debug_enabled: Cell<bool>,
        pub active_list: RefCell<ActiveList>,
        pub store: RefCell<Option<gio::ListStore>>,
        pub worker: OnceCell<WorkerHandle>,
        pub read_pool: OnceCell<ReadPool>,
        /// Phase 12.5 — first-of-month for the calendar page's
        /// currently-displayed month. `None` until the user opens
        /// Calendar for the first time, at which point we lazily
        /// init to today's month. Mutated by prev / next / today /
        /// month-picker handlers; the page rebuilds from this on
        /// every refresh. `Cell` (not `RefCell`) because
        /// `NaiveDate: Copy`.
        pub calendar_viewed: Cell<Option<chrono::NaiveDate>>,

        /// Aligned with the sidebar rows. `None` marks non-selectable
        /// header rows (e.g., "Areas", "Unfiled"); `Some(active)`
        /// dispatches to that list when the row is activated.
        pub sidebar_targets: RefCell<Vec<Option<ActiveList>>>,
        /// Aligned with `sidebar_targets`. Holds the user-visible label
        /// for filterable rows (areas, projects, tags). `None` for
        /// canonical rows (always visible) and section headers (which
        /// follow their children's visibility). Phase 7e.
        pub sidebar_titles: RefCell<Vec<Option<String>>>,
        /// Project / area title caches populated when the sidebar is
        /// built; consulted by `set_active_list` to resolve the
        /// content-pane title for `Project(id)` / `Area(id)`.
        pub project_titles: RefCell<HashMap<i64, String>>,
        pub area_titles: RefCell<HashMap<i64, String>>,
        /// v0.5.0 (Slice B2) — per-area colour cache (hex strings or
        /// None). The Edit Area dialog reads it for picker pre-select;
        /// the row factory consults it (resolved through `project_meta`'s
        /// `area_id`) to paint the 3 px area-accent stripe on each row.
        pub area_colors: RefCell<HashMap<i64, Option<String>>>,

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
        /// v0.6.16 — Logbook moved from `CANONICAL_LISTS` to the
        /// trailing slot of `top_tier_extras`, so its badge isn't
        /// in the `canonical_badges` Vec. Tracked separately so
        /// `refresh_canonical_badges` can still update its count.
        pub logbook_badge: RefCell<Option<gtk::Label>>,
        pub project_badges: RefCell<HashMap<i64, gtk::Label>>,
        pub area_badges: RefCell<HashMap<i64, gtk::Label>>,
        pub tag_badges: RefCell<HashMap<i64, gtk::Label>>,

        /// Tag-name cache populated from `db::read::list_tags` when
        /// the sidebar is built. `set_active_list` consults it for
        /// `Tag(id)` content-pane titles.
        pub tag_titles: RefCell<HashMap<i64, String>>,
        /// v0.3.0 — per-tag colour cache (hex strings or None).
        /// Used by the rename-tag dialog to pre-select the swatch
        /// and by the task-row factory to render coloured #pills.
        pub tag_colors: RefCell<HashMap<i64, Option<String>>>,

        /// Phase 14 — saved-perspective caches populated from
        /// `db::read::list_perspectives` during sidebar build. The
        /// title cache resolves the content-pane heading for
        /// `Perspective(id)`; the full meta cache lets
        /// `refresh_active_list` re-parse the saved filter expression
        /// without a round-trip to the read pool, and powers the
        /// rename-prefill / delete-confirmation prompts.
        pub perspective_titles: RefCell<HashMap<i64, String>>,
        pub perspective_meta: RefCell<HashMap<i64, atrium_core::Perspective>>,

        /// Shared "most recent undo" callback. `show_undo_toast`
        /// stashes a fresh cell here so that either the toast button
        /// *or* the `Ctrl+Z` accel can take it (whoever fires first
        /// wins; the loser sees an empty cell and no-ops). Phase 7f.
        pub last_undo: RefCell<Option<UndoCell>>,

        /// v0.2.2 — fingerprint of the last filter-parse warning we
        /// surfaced as a toast. Refreshes of the same query (e.g.
        /// TaskChanges arrivals on a SearchResults view) check this
        /// before re-toasting, so the user sees one toast per typo
        /// rather than one per refresh tick.
        pub last_filter_warning: RefCell<Option<String>>,

        /// v0.4.1 — search-history ring buffer. The last
        /// `SEARCH_HISTORY_MAX` non-empty queries the user committed
        /// to, newest at the end. ↑ / ↓ inside the search entry
        /// cycles through this; the cursor is `None` when the user
        /// isn't navigating, `Some(n)` while they walk back through
        /// history. In-memory only for v0.4.1 — restarts forget;
        /// persistence is a follow-up if usage warrants it.
        pub search_history: RefCell<Vec<String>>,
        pub search_history_cursor: RefCell<Option<usize>>,

        /// Phase 10 — Builder Mode Inspector pane handle. `None`
        /// until `attach_data_layer` runs (the pane needs a
        /// `WorkerHandle`); from then on the window calls
        /// `set_task` / `clear` on it as the selection moves.
        pub inspector_pane: RefCell<Option<Rc<crate::ui::inspector_pane::InspectorPane>>>,
        /// v0.1.6 — synchronous mode tracker. `apply_mode` is the
        /// single writer; everything that needs to know "are we
        /// in Builder right now" reads from this Cell rather than
        /// `gio::Settings::new(APP_ID).string("mode")`. v0.1.5
        /// surfaced a case where the GSettings string was returning
        /// a value that didn't match what `apply_mode` had just
        /// flipped to — most likely a per-instance staleness in
        /// the dconf backend during a same-frame read. The Cell is
        /// updated synchronously inside `apply_mode`, so any later
        /// callback in the same event loop iteration reads the
        /// just-applied value.
        pub current_mode_is_builder: Cell<bool>,
        /// Cached project metadata for the active Project view —
        /// needed so the `Sequential` switch + `Review interval`
        /// SpinButton can populate from current values when the
        /// user selects a project. Keyed by project id.
        pub project_meta: RefCell<HashMap<i64, atrium_core::Project>>,
        /// True while we're populating the project extras toolbar
        /// programmatically, so the value-changed handlers don't
        /// echo back as worker writes.
        pub project_extras_syncing: Cell<bool>,
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
            self.active_list.replace(ActiveList::Today);

            let obj = self.obj();
            obj.bind_window_state();
            obj.install_menu();
            obj.build_sidebar();
            obj.wire_project_extras();
            obj.init_list_view();
            obj.wire_new_task_entry();
            obj.wire_search_bar();
            obj.install_window_actions();
        }
    }
    impl WidgetImpl for AtriumWindow {}
    impl WindowImpl for AtriumWindow {
        fn close_request(&self) -> Propagation {
            let obj = self.obj();
            obj.save_window_state();
            // Phase 8h — clean up phantom-child popovers before the
            // rows finalize, so GTK doesn't log a warning per row.
            obj.unparent_sidebar_context_menus();
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

/// How the row-context chip should render for a given active list.
/// `build_context_resolver` selects one of these and the resolver
/// closure dispatches per row.
#[derive(Debug, Clone, Copy)]
enum ContextMode {
    /// Don't render anything — the heading already names the
    /// project (e.g. on `Project(_)` views).
    Suppressed,
    /// Render just the project name. Used on `Area(_)` views: the
    /// area is the heading, the project is the contextual scope
    /// inside it.
    ProjectOnly,
    /// Render `Area › Project`. The full hierarchy chip; used
    /// everywhere else.
    AreaAndProject,
}

/// Sidebar's persistent top-tier rows in display order.
///
/// v0.6.16 dropped `Logbook` from this set and moved it to the
/// trailing slot of `top_tier_extras` — the original ordering put
/// "completed past" between the active lists and the
/// Agenda / Forecast / Review block, which read as out of place.
/// Logbook now bookends the top tier where the past belongs.
const CANONICAL_LISTS: &[ActiveList] = &[
    ActiveList::Inbox,
    ActiveList::Today,
    ActiveList::Upcoming,
    ActiveList::Anytime,
    ActiveList::Someday,
];

fn icon_for(list: &ActiveList) -> &'static str {
    match list {
        ActiveList::Inbox => "inbox-symbolic",
        ActiveList::Today => "starred-symbolic",
        ActiveList::Upcoming => "x-office-calendar-symbolic",
        ActiveList::Anytime => "view-list-symbolic",
        ActiveList::Someday => "weather-clear-night-symbolic",
        ActiveList::Logbook => "document-open-recent-symbolic",
        ActiveList::Project(_) => "view-list-bullet-symbolic",
        ActiveList::Area(_) => "folder-symbolic",
        ActiveList::Tag(_) => "tag-symbolic",
        ActiveList::SearchResults(_) => "system-search-symbolic",
        ActiveList::Forecast => "x-office-calendar-symbolic",
        ActiveList::Review => "object-select-symbolic",
        ActiveList::Agenda => "alarm-symbolic",
        ActiveList::Calendar => "x-office-calendar-symbolic",
        ActiveList::Perspective(_) => "view-grid-symbolic",
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
    fn install_tag_context_menu(&self, row: &gtk::ListBoxRow, tag_id: i64) {
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
    fn install_area_context_menu(&self, row: &gtk::ListBoxRow, area_id: i64) {
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
    fn build_perspectives_section_header(&self) -> gtk::ListBoxRow {
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
    fn install_perspective_context_menu(&self, row: &gtk::ListBoxRow, perspective_id: i64) {
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

    /// Update canonical-row badges from `canonical_counts`. v0.6.16
    /// split out the Logbook badge — it lives in the trailing slot
    /// of `top_tier_extras` now and is tracked in `logbook_badge`
    /// rather than `canonical_badges`. Both still refresh here so
    /// callers don't need to remember the split.
    fn refresh_canonical_badges(&self) {
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
    fn refresh_dynamic_badges(&self) {
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
        for a in &areas {
            area_titles.insert(a.id, a.title.clone());
            area_colors.insert(a.id, a.color.clone());
        }
        for p in &projects {
            project_titles.insert(p.id, p.title.clone());
        }
        self.imp().area_titles.replace(area_titles);
        self.imp().area_colors.replace(area_colors);
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

    fn init_list_view(&self) {
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
        let win_weak4 = self.downgrade();
        let pool_source = move || {
            win_weak4
                .upgrade()
                .and_then(|w| w.imp().read_pool.get().cloned())
        };
        let factory = build_factory(on_toggle, on_rename, on_reorder, pool_source);
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
    fn install_calendar_width_watcher(&self) {
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
    fn install_inspector_pane(&self, worker: WorkerHandle) {
        let win_weak = self.downgrade();
        let pane = crate::ui::inspector_pane::InspectorPane::install(
            &self.imp().inspector_pane_host,
            worker,
            move |task_id| {
                if let Some(win) = win_weak.upgrade() {
                    win.open_tag_editor_for(task_id);
                }
            },
        );
        *self.imp().inspector_pane.borrow_mut() = Some(pane);
    }

    /// Subscribe to GSettings `mode` and route changes through
    /// `apply_mode`. Per spec §3 / CLAUDE.md commitment #1, this is
    /// pure UI rerender — no worker dispatch.
    fn install_mode_observer(&self) {
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
    fn refresh_project_meta(&self, projects: &[Project]) {
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
    fn wire_project_extras(&self) {
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
    fn populate_project_extras(&self, project_id: i64) {
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
    fn title_for(&self, active: ActiveList) -> String {
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
                .map(|n| format!("#{n}"))
                .unwrap_or_else(|| "Tag".into()),
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
    fn subtitle_for(&self, active: &ActiveList) -> String {
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
    fn project_areas_map(&self) -> HashMap<i64, Option<i64>> {
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
    fn build_area_color_resolver(&self) -> impl Fn(&Task) -> String + use<> {
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
    fn build_cookie_resolver(&self) -> impl Fn(&Task) -> String + use<> {
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

    fn build_context_resolver(&self, active: &ActiveList) -> impl Fn(&Task) -> String + use<> {
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

        // Phase 12 — Forecast is a Builder stub no longer; it
        // renders a real calendar-axis page.
        if matches!(active, ActiveList::Forecast) {
            store.remove_all();
            self.refresh_forecast_page();
            self.imp().content_stack.set_visible_child_name("forecast");
            return;
        }

        // Phase 13 — Review is a Builder stub no longer; it
        // renders the project-review queue.
        if matches!(active, ActiveList::Review) {
            store.remove_all();
            self.refresh_review_page();
            self.imp().content_stack.set_visible_child_name("review");
            return;
        }

        // v0.6.0 (Slice C2) — Logbook gets its own page with day-band
        // grouping (Today / Yesterday / Last 7 Days / Older). The
        // regular list page used to render Logbook flat; the new
        // grouping is harder to express through a single GtkListView,
        // so we split it out the same way Forecast / Review do.
        if matches!(active, ActiveList::Logbook) {
            store.remove_all();
            self.refresh_logbook_page();
            self.imp().content_stack.set_visible_child_name("logbook");
            return;
        }

        // v0.6.4 (Slice D2) — Agenda canonical page. Org-mode-style
        // chronological view with five sections.
        if matches!(active, ActiveList::Agenda) {
            store.remove_all();
            self.refresh_agenda_page();
            self.imp().content_stack.set_visible_child_name("agenda");
            return;
        }

        // Phase 12.5 (v0.11.0) — Calendar Month View. Builder-only
        // paper-calendar lens; sidebar entry already filtered out
        // in Simple mode, but defend in depth here too.
        if matches!(active, ActiveList::Calendar) {
            store.remove_all();
            self.refresh_calendar_page();
            self.imp().content_stack.set_visible_child_name("calendar");
            return;
        }

        // Phase 14 — saved perspective. Resolve the filter
        // expression from the meta cache, run it through the same
        // parse + apply pipeline as the search bar, and render the
        // matching tasks in the standard list view. The "list" page
        // owns the rendering — the perspective is a saved query, not
        // a separate page.
        if let ActiveList::Perspective(id) = &active {
            // v0.6.0 (Slice D1 GUI) — perspective whose
            // `renderer = "board"` renders as a kanban instead of
            // a flat list. We branch *before* the list path: the
            // board page has its own host AdwBin in the content
            // stack, no shared GtkListView state.
            let perspective_snapshot = self.imp().perspective_meta.borrow().get(id).cloned();
            if let Some(p) = perspective_snapshot
                && p.renderer.eq_ignore_ascii_case("board")
            {
                store.remove_all();
                match self.refresh_board_page(&p) {
                    Ok(()) => {
                        self.imp().content_stack.set_visible_child_name("board");
                        return;
                    }
                    Err(err) => {
                        // Bad renderer_config — surface a toast and
                        // fall through to the list path so the user
                        // still sees their tasks.
                        error!(
                            ?err,
                            perspective_id = id,
                            "board renderer_config malformed; falling back to list"
                        );
                    }
                }
            }
            self.imp().content_stack.set_visible_child_name("list");
            let expr = self
                .imp()
                .perspective_meta
                .borrow()
                .get(id)
                .map(|p| p.filter_expr.clone());
            let Some(expr) = expr else {
                // Perspective row vanished from underneath us
                // (e.g., deleted in another worker iteration). Drop
                // back to Today.
                store.remove_all();
                self.update_empty_state(&store);
                return;
            };
            let parsed = crate::ui::filter::parse(&expr);
            // v0.2.2 — surface unknown-token warnings against the
            // saved expression so users notice when a Perspective's
            // filter has a typo. Deduped so we don't re-toast on
            // every refresh.
            self.surface_filter_warnings(&parsed);
            // v0.6.18 — SQL fast-path for the list-renderer
            // perspective path. v0.5.3 shipped the translation
            // evaluator and v0.6.6 wired it into the board path;
            // this loop was the deferred case noted in the v0.5.3
            // patchnote. Translatable filters (most: is:open,
            // tag:work, due:today, …) load only matching rows
            // instead of pulling the full task table and
            // filtering in Rust. The fallback path keeps the
            // in-memory `filter::apply` for expressions the
            // translator can't yet express (regex, fuzzy,
            // composite is:today / etc.).
            //
            // We need both the name-only `TagMap` (for filter
            // evaluation) and the colour-bearing `TagPillMap`
            // (for row rendering). Pre-v0.6.18 these were two
            // separate DB roundtrips with the same JOIN; now
            // we fetch the pill map once and derive the name
            // map from it.
            let tag_pills: crate::ui::task_list::TagPillMap = pool
                .with(atrium_core::db::read::tag_info_per_task)
                .unwrap_or_default();
            let tag_map: TagMap = crate::ui::task_list::tag_names_from_pills(&tag_pills);
            let project_areas = self.project_areas_map();
            let mut tasks: Vec<Task> = if let Some(expr) = &parsed.expr
                && let Some(clause) = atrium_search::try_translate(expr, today)
            {
                let params: Vec<atrium_core::SqlBindValue> =
                    clause.params.iter().map(Into::into).collect();
                pool.with(|conn| {
                    atrium_core::db::read::list_tasks_matching(conn, &clause.sql, &params)
                })
                .unwrap_or_default()
            } else {
                let loaded = match pool.with(atrium_core::db::read::list_all_tasks) {
                    Ok(t) => t,
                    Err(e) => {
                        error!(?e, perspective_id = id, "failed to load perspective");
                        store.remove_all();
                        self.update_empty_state(&store);
                        return;
                    }
                };
                crate::ui::filter::apply(
                    loaded,
                    &parsed,
                    today,
                    &tag_map,
                    &self.imp().project_titles.borrow(),
                    &project_areas,
                    &self.imp().area_titles.borrow(),
                )
            };
            // sort: modifiers — both paths need this; only the
            // in-memory `filter::apply` would have applied it
            // pre-v0.6.18. Honour the modifier on either path.
            if !parsed.sorts.is_empty() {
                crate::ui::filter::sort_tasks_by_specs(&mut tasks, &parsed.sorts);
            }
            // v0.5.2 — bm25-rank a Perspective whose filter
            // contains bare text and doesn't pin a sort.
            // `bm25_pinned_sort` mirrors the meaning of
            // `parsed.sorts.is_empty()` for the post-store
            // sort_by_position skip below.
            let bm25_pinned_sort = if parsed.sorts.is_empty() {
                let terms = parsed
                    .expr
                    .as_ref()
                    .map(atrium_search::collect_text_terms)
                    .unwrap_or_default();
                if !terms.is_empty() {
                    let scores = pool
                        .with(|conn| atrium_core::db::read::bm25_for_terms(conn, &terms))
                        .unwrap_or_default();
                    if !scores.is_empty() {
                        crate::ui::filter::rank_by_bm25_recency(&mut tasks, &scores, today);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            };
            let context_for = self.build_context_resolver(&active);
            let area_color_for = self.build_area_color_resolver();
            let cookie_for = self.build_cookie_resolver();
            replace_store_with_tags_seq(
                &store,
                &tasks,
                &tag_pills,
                false,
                context_for,
                area_color_for,
                cookie_for,
            );
            // v0.4.1 — `sort:KEY` modifiers in the saved
            // perspective override position order. apply()
            // already sorted the Vec; just don't clobber it
            // with sort_by_position. v0.5.2 — same skip when
            // bm25 ranking ordered the Vec.
            if parsed.sorts.is_empty() && !bm25_pinned_sort {
                sort_by_position(&store);
            }
            self.update_empty_state(&store);
            return;
        }

        let result: Result<Vec<Task>, _> = pool.with(|conn| match &active {
            ActiveList::Inbox => atrium_core::db::read::list_inbox(conn),
            ActiveList::Today => atrium_core::db::read::list_today(conn, today),
            ActiveList::Upcoming => atrium_core::db::read::list_upcoming(conn, today),
            ActiveList::Anytime => atrium_core::db::read::list_anytime(conn, today),
            ActiveList::Someday => atrium_core::db::read::list_someday(conn),
            ActiveList::Logbook => atrium_core::db::read::list_logbook(conn),
            ActiveList::Project(id) => atrium_core::db::read::list_project(conn, *id),
            ActiveList::Area(id) => atrium_core::db::read::list_area(conn, *id),
            ActiveList::Tag(id) => atrium_core::db::read::list_tasks_with_tag(conn, *id),
            ActiveList::SearchResults(query) => {
                // v0.6.18 — search expressions take the SQL
                // fast-path when the parser can translate them.
                // For 80%+ of typed queries (`tag:work`,
                // `is:overdue`, `due:today`, …) this avoids the
                // load-everything-then-filter-in-Rust pattern.
                // Untranslatable expressions (regex / fuzzy /
                // composite is:today) fall through to the
                // load-everything path; `filter::apply` below
                // handles the rest.
                let parsed = crate::ui::filter::parse(query);
                let Some(expr) = parsed.expr.as_ref() else {
                    return Ok(Vec::new());
                };
                if let Some(clause) = atrium_search::try_translate(expr, today) {
                    let params: Vec<atrium_core::SqlBindValue> =
                        clause.params.iter().map(Into::into).collect();
                    atrium_core::db::read::list_tasks_matching(conn, &clause.sql, &params)
                } else {
                    atrium_core::db::read::list_all_tasks(conn)
                }
            }
            ActiveList::Forecast
            | ActiveList::Review
            | ActiveList::Agenda
            | ActiveList::Calendar
            | ActiveList::Perspective(_) => {
                // Unreachable — gated above. Keeps the match exhaustive.
                Ok(Vec::new())
            }
        });

        match result {
            Ok(tasks) => {
                // v0.6.18 — single tag-info fetch covers both maps.
                let tag_pills: crate::ui::task_list::TagPillMap = pool
                    .with(atrium_core::db::read::tag_info_per_task)
                    .unwrap_or_default();
                let tag_map: TagMap = crate::ui::task_list::tag_names_from_pills(&tag_pills);
                // v0.4.1 — capture whether the user's search expression
                // pinned a sort order so the post-store sort_by_position
                // call can skip when the query already sorted the Vec.
                let mut search_pinned_sort = false;
                let tasks = if let ActiveList::SearchResults(q) = &active {
                    let parsed = crate::ui::filter::parse(q);
                    search_pinned_sort = !parsed.sorts.is_empty();
                    let project_areas = self.project_areas_map();
                    let mut filtered = crate::ui::filter::apply(
                        tasks,
                        &parsed,
                        today,
                        &tag_map,
                        &self.imp().project_titles.borrow(),
                        &project_areas,
                        &self.imp().area_titles.borrow(),
                    );
                    // v0.5.2 — bm25 ranking when bare text is in the
                    // query and the user hasn't pinned a sort. We
                    // flip `search_pinned_sort` so the post-store
                    // sort_by_position doesn't clobber the rank.
                    if !search_pinned_sort {
                        let terms = parsed
                            .expr
                            .as_ref()
                            .map(atrium_search::collect_text_terms)
                            .unwrap_or_default();
                        if !terms.is_empty() {
                            let scores = pool
                                .with(|conn| atrium_core::db::read::bm25_for_terms(conn, &terms))
                                .unwrap_or_default();
                            if !scores.is_empty() {
                                crate::ui::filter::rank_by_bm25_recency(
                                    &mut filtered,
                                    &scores,
                                    today,
                                );
                                search_pinned_sort = true;
                            }
                        }
                    }
                    filtered
                } else {
                    tasks
                };
                // Phase 11 — sequential project rendering. Only on
                // a single-project view AND only when the project
                // has sequential=true. Other views (Today, Inbox,
                // Area aggregates) never dim rows.
                let sequential = match &active {
                    ActiveList::Project(id) => self
                        .imp()
                        .project_meta
                        .borrow()
                        .get(id)
                        .is_some_and(|p| p.sequential),
                    _ => false,
                };
                let context_for = self.build_context_resolver(&active);
                let area_color_for = self.build_area_color_resolver();
                let cookie_for = self.build_cookie_resolver();
                replace_store_with_tags_seq(
                    &store,
                    &tasks,
                    &tag_pills,
                    sequential,
                    context_for,
                    area_color_for,
                    cookie_for,
                );
                // Skip the position sort when the search expression
                // pinned a sort — apply() already ordered the Vec.
                if !search_pinned_sort {
                    sort_by_position(&store);
                }
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
            ActiveList::Perspective(id) => changes.perspectives_deleted.contains(&id),
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
            if t.as_ref() == Some(&active)
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
        // Phase 12 — Forecast view rebuilds in full on any task
        // delta. Day-card layout depends on date grouping that's
        // cheaper to recompute than to diff in place.
        if matches!(active, ActiveList::Forecast) {
            self.refresh_forecast_page();
            self.refresh_counts();
            self.refresh_canonical_badges();
            self.refresh_dynamic_badges();
            return;
        }
        // v0.6.0 (Slice C2) — Logbook day-band view. Same shape as
        // Forecast: rebuild on any delta so a freshly-completed task
        // lands in the Today band immediately.
        if matches!(active, ActiveList::Logbook) {
            self.refresh_logbook_page();
            self.refresh_counts();
            self.refresh_canonical_badges();
            self.refresh_dynamic_badges();
            return;
        }
        // v0.6.4 (Slice D2) — Agenda canonical page. Composite over
        // dates + completion + defer; rebuild on any delta so a
        // toggled task slides between sections immediately.
        if matches!(active, ActiveList::Agenda) {
            self.refresh_agenda_page();
            self.refresh_counts();
            self.refresh_canonical_badges();
            self.refresh_dynamic_badges();
            return;
        }
        // Phase 12.5 — Calendar Month View rebuilds in full on any
        // task delta. Drag-to-reschedule produces a TaskChanges
        // update for the dropped task, and the cleanest path to
        // re-render the cells is to rebuild the grid from scratch
        // (same shape as Forecast / Agenda above).
        if matches!(active, ActiveList::Calendar) {
            self.refresh_calendar_page();
            self.refresh_counts();
            self.refresh_canonical_badges();
            self.refresh_dynamic_badges();
            return;
        }
        // v0.7.4 — Review canonical page's "This week" section
        // depends on the weekly-walk filter result + the per-task
        // last_reviewed_at exclusion. Any task delta (especially a
        // MarkTaskReviewed update) needs to rerun both, so rebuild
        // the page in full when the active view is Review.
        if matches!(active, ActiveList::Review) {
            self.refresh_review_page();
            self.refresh_counts();
            self.refresh_canonical_badges();
            self.refresh_dynamic_badges();
            return;
        }
        // Phase 14 — perspective views run a saved filter expression
        // against the global task set. The diff applier doesn't have
        // visibility into the filter, so rerun the read query (same
        // path SearchResults takes — cheap; FTS5-backed when the
        // expression has freeform text).
        if matches!(active, ActiveList::Perspective(_)) {
            self.refresh_active_list();
            self.refresh_counts();
            self.refresh_canonical_badges();
            self.refresh_dynamic_badges();
            return;
        }
        let today = Local::now().date_naive();
        // Re-load the per-task tag pill map so the diff applier
        // renders updated pills with their colours. Drop the older
        // name-only TagMap here — the diff applier no longer needs
        // it (only filter::apply does, and it isn't called inside
        // apply_task_changes).
        let tag_pills: crate::ui::task_list::TagPillMap = self
            .read_pool()
            .and_then(|p| p.with(atrium_core::db::read::tag_info_per_task).ok())
            .unwrap_or_default();
        // Phase 11 — propagate the sequential flag so the diff
        // applier recomputes queued state when the active view is
        // a sequential project.
        let sequential = match &active {
            ActiveList::Project(id) => self
                .imp()
                .project_meta
                .borrow()
                .get(id)
                .is_some_and(|p| p.sequential),
            _ => false,
        };
        let context_for = self.build_context_resolver(&active);
        let area_color_for = self.build_area_color_resolver();
        let cookie_for = self.build_cookie_resolver();
        crate::ui::task_list::apply_changes_seq(
            &store,
            changes,
            active,
            today,
            &tag_pills,
            sequential,
            context_for,
            area_color_for,
            cookie_for,
        );
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
            let (title, description) = self.empty_state_copy(&active);
            status.set_title(&title);
            status.set_description(Some(&description));
            status.set_icon_name(Some(icon_for(&active)));
            stack.set_visible_child_name("empty");
        } else {
            stack.set_visible_child_name("list");
        }
    }

    fn empty_state_copy(&self, active: &ActiveList) -> (String, String) {
        match active {
            ActiveList::Inbox => (
                "Inbox zero".into(),
                "Catch a thought with Ctrl+N or the entry below — Atrium will keep it safe until you place it.".into(),
            ),
            ActiveList::Today => (
                "Clear plate today".into(),
                "Nothing scheduled and no deadlines crossing the horizon. Glance at Upcoming for what's next, or take the afternoon back.".into(),
            ),
            ActiveList::Upcoming => (
                "Open horizon".into(),
                "Schedule a task to a future date and it'll surface here, sorted by when.".into(),
            ),
            ActiveList::Anytime => (
                "Nothing waiting".into(),
                "Open tasks without a date land here — your low-pressure pool to dip into when there's time.".into(),
            ),
            ActiveList::Someday => (
                "Park it for later".into(),
                "Ideas and maybes belong here. Scheduled to Someday means \"on the radar, no commitment yet\".".into(),
            ),
            ActiveList::Logbook => (
                "Nothing logged yet".into(),
                "Completed tasks settle here in reverse chronological order — your record of the work done.".into(),
            ),
            ActiveList::Project(_) => (
                format!("{} is empty", self.title_for(active.clone())),
                "Add the first task with the entry below, or capture quickly with Ctrl+Alt+Space.".into(),
            ),
            ActiveList::Area(_) => (
                format!("Nothing open in {}", self.title_for(active.clone())),
                "An area aggregates open tasks across its projects. Add a project under it, then file tasks into the project.".into(),
            ),
            ActiveList::Tag(_) => (
                format!("No tasks tagged {}", self.title_for(active.clone())),
                "Apply this tag from a task's Inspector or with #tag in Quick Entry.".into(),
            ),
            ActiveList::SearchResults(q) if q.trim().is_empty() => (
                "Search Atrium".into(),
                "Type to find tasks by title or note. Try filters too: tag:errand, due:today, is:overdue.".into(),
            ),
            ActiveList::SearchResults(q) => (
                format!("No matches for \u{201c}{q}\u{201d}"),
                "Search covers task titles, notes, and filter expressions. Check spelling, or try a broader term.".into(),
            ),
            ActiveList::Forecast => (
                "Open horizon".into(),
                "Schedule, deadline, or defer a task and it'll appear here on its day. Drag rows between days to reschedule.".into(),
            ),
            ActiveList::Review => (
                "All caught up".into(),
                "Projects with a review interval surface here when their last review goes stale — oldest first.".into(),
            ),
            ActiveList::Perspective(_) => (
                format!("{} is quiet", self.title_for(active.clone())),
                "No tasks currently match this perspective's filter expression. Adjust the filter or wait for matches to appear.".into(),
            ),
            ActiveList::Agenda => (
                "Nothing on the agenda".into(),
                "No overdue, today, or near-term scheduled tasks — the next two weeks are clear.".into(),
            ),
            ActiveList::Calendar => (
                "Open month".into(),
                "Schedule a task and its day cell will fill in. Page Up / Page Down to navigate months; Today resets to the current month.".into(),
            ),
        }
    }

    /// Toggle handler — fires the worker call. The worker emits a
    /// `TaskChanges` delta which the bridge applies; we don't update
    /// the model here.
    fn handle_toggle(&self, id: i64, want_completed: bool) {
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
    fn handle_rename(&self, id: i64, new_title: String) {
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

    fn focused_task_id(&self) -> Option<i64> {
        self.selected_task_ids().first().copied()
    }

    /// All task ids currently selected in the active list. Order
    /// matches the model (low index → high index).
    fn selected_task_ids(&self) -> Vec<i64> {
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
    fn update_selection_bar(&self, n: i64) {
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

        // Builder Mode — route through the side pane. Repopulate
        // if the pane isn't already showing this task (e.g., the
        // user right-clicked a row that wasn't selected; the
        // selection-changed signal hasn't fired yet so the pane
        // still shows the previously-selected row). Either way,
        // grab keyboard focus on the title.
        let builder = self.imp().current_mode_is_builder.get();
        if builder && let Some(pane) = self.imp().inspector_pane.borrow().clone() {
            if pane.current_task_id() != Some(task_id) {
                pane.set_task(task, projects, tag_count);
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
        crate::ui::inspector::open(self, worker, task, projects, tag_count, on_edit_tags);
    }

    /// `Ctrl+I` shortcut entry point — operates on the focused /
    /// first-selected task. The mode-specific routing lives in
    /// `open_inspector_for`; this is just the focus-resolver wrapper.
    pub fn open_inspector_focused(&self) {
        if let Some(id) = self.focused_task_id() {
            self.open_inspector_for(id);
        }
    }

    /// Phase 12 — rebuild the Forecast page from the read pool
    /// and mount it into the `forecast_host` AdwBin. Called from
    /// `refresh_active_list` when the active view becomes
    /// Forecast, and from `apply_task_changes` if the active view
    /// is currently Forecast (so a drag-to-reschedule, completion
    /// toggle, or worker-driven mutation refreshes the cards).
    fn refresh_forecast_page(&self) {
        let Some(pool) = self.read_pool() else {
            self.imp().forecast_host.set_child(None::<&gtk::Widget>);
            return;
        };
        let today = Local::now().date_naive();
        let forecast_tasks = pool
            .with(|conn| {
                atrium_core::db::read::list_forecast(
                    conn,
                    today,
                    crate::ui::forecast::FORECAST_WINDOW_DAYS,
                )
            })
            .unwrap_or_default();
        let overdue = pool
            .with(|conn| atrium_core::db::read::list_overdue(conn, today))
            .unwrap_or_default();
        // v0.6.17 — click → open in Inspector. Same shape board /
        // agenda use; reuses the existing `win.edit-details-for(id)`
        // action so keyboard shortcut + row click + this callback
        // all funnel through one path.
        let weak = self.downgrade();
        let on_click = move |task_id: i64| {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let _ = WidgetExt::activate_action(
                &window,
                "win.edit-details-for",
                Some(&task_id.to_variant()),
            );
        };
        let widget = crate::ui::forecast::build_page(
            today,
            &forecast_tasks,
            &overdue,
            self.worker(),
            on_click,
        );
        self.imp().forecast_host.set_child(Some(&widget));
    }

    /// Phase 13 → v0.7.2 — rebuild the Review page. Renders two
    /// sections in one surface: the project review queue (Phase 13),
    /// and the canonical Weekly Walk (the open-tasks-this-week
    /// filter formerly seeded as the "Weekly Review" Perspective).
    /// Called when the active list becomes Review, and from
    /// `apply_library_changes` so Mark-Reviewed clicks drop the
    /// row immediately, and from `apply_task_changes` so
    /// completions in the weekly walk drop their row immediately.
    fn refresh_review_page(&self) {
        let Some(pool) = self.read_pool() else {
            self.imp().review_host.set_child(None::<&gtk::Widget>);
            return;
        };
        let today = Local::now().date_naive();
        let queue = pool
            .with(|conn| atrium_core::db::read::list_review_queue(conn, today))
            .unwrap_or_default();

        // Weekly walk — open tasks matching REVIEW_WEEKLY_WALK_FILTER.
        // We load every task and filter in-memory; the weekly walk
        // isn't a hot path (it rebuilds only on Review-page open or
        // a relevant delta), and the filter expression has predicates
        // that the SQL fast-path can't all translate cleanly.
        let all_tasks = pool
            .with(atrium_core::db::read::list_all_tasks)
            .unwrap_or_default();
        let tag_names = pool
            .with(atrium_core::db::read::tag_names_per_task)
            .unwrap_or_default();
        let project_titles = self.imp().project_titles.borrow().clone();
        let area_titles = self.imp().area_titles.borrow().clone();
        let project_areas = self.project_areas_map();
        let query = crate::ui::filter::parse(atrium_core::db::REVIEW_WEEKLY_WALK_FILTER);
        let weekly_tasks = crate::ui::filter::apply(
            all_tasks,
            &query,
            today,
            &tag_names,
            &project_titles,
            &project_areas,
            &area_titles,
        );
        // v0.7.4 — exclude tasks marked reviewed within the last 7
        // days. Mark Reviewed is a manual user action; the page
        // hides the row for one cycle. After 7 days the row
        // resurfaces if it still matches the weekly-walk filter.
        let cutoff = today - chrono::Duration::days(7);
        let weekly_tasks: Vec<atrium_core::Task> = weekly_tasks
            .into_iter()
            .filter(|t| {
                t.last_reviewed_at
                    .map(|when| when.date_naive() < cutoff)
                    .unwrap_or(true)
            })
            .collect();

        let tag_pills: crate::ui::task_list::TagPillMap = pool
            .with(atrium_core::db::read::tag_info_per_task)
            .unwrap_or_default();

        let weak = self.downgrade();
        let on_click = move |task_id: i64| {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let _ = WidgetExt::activate_action(
                &window,
                "win.edit-details-for",
                Some(&task_id.to_variant()),
            );
        };

        // v0.7.4 — Mark Reviewed callback. Clicking the per-row
        // button dispatches `worker.mark_task_reviewed(id)`; the
        // worker emits a TaskChanges{updated} delta which triggers
        // refresh_review_page (apply_task_changes routes to the
        // Review-rebuild branch when the active list is Review).
        let weak = self.downgrade();
        let on_mark_reviewed = move |task_id: i64| {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let Some(worker) = window.worker() else {
                return;
            };
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker.mark_task_reviewed(task_id).await {
                    error!(?e, task_id, "mark_task_reviewed failed");
                }
            });
        };

        let widget = crate::ui::review::build_page(
            today,
            &queue,
            &weekly_tasks,
            &project_titles,
            &area_titles,
            &tag_pills,
            self.worker(),
            on_click,
            on_mark_reviewed,
        );
        self.imp().review_host.set_child(Some(&widget));
    }

    /// v0.6.0 (Slice C2) — rebuild the Logbook page with day-band
    /// grouping (Today / Yesterday / Last 7 Days / Older) and mount
    /// it into the `logbook_host` AdwBin. Replaces the flat list
    /// rendering Logbook used to share with Inbox / Today / etc.
    /// Called when the active list becomes Logbook, and from
    /// `apply_task_changes` if the active list is Logbook (so a
    /// completion toggle on another list drops a freshly-finished
    /// task into the Today band immediately).
    fn refresh_logbook_page(&self) {
        let Some(pool) = self.read_pool() else {
            self.imp().logbook_host.set_child(None::<&gtk::Widget>);
            return;
        };
        let today = Local::now().date_naive();
        let tasks = pool
            .with(atrium_core::db::read::list_logbook)
            .unwrap_or_default();
        let tag_pills: crate::ui::task_list::TagPillMap = pool
            .with(atrium_core::db::read::tag_info_per_task)
            .unwrap_or_default();
        let project_titles = self.imp().project_titles.borrow().clone();
        let area_titles = self.imp().area_titles.borrow().clone();
        let project_areas: HashMap<i64, Option<i64>> = self
            .imp()
            .project_meta
            .borrow()
            .iter()
            .map(|(id, p)| (*id, p.area_id))
            .collect();
        let widget = crate::ui::logbook::build_page(
            today,
            &tasks,
            &project_titles,
            &project_areas,
            &area_titles,
            &tag_pills,
        );
        self.imp().logbook_host.set_child(Some(&widget));
    }

    /// v0.6.4 (Slice D2) — rebuild the Agenda canonical page. Loads
    /// every open task, runs each through `agenda::classify` to
    /// bucket into Overdue / Today / Tomorrow / This Week / Next
    /// Week, and mounts the resulting widget into `agenda_host`.
    fn refresh_agenda_page(&self) {
        let Some(pool) = self.read_pool() else {
            self.imp().agenda_host.set_child(None::<&gtk::Widget>);
            return;
        };
        let today = Local::now().date_naive();
        // The agenda is open-tasks-only; we'd otherwise pay the cost
        // of pulling completed rows from the Logbook just to filter
        // them out. `list_all_tasks` is the simplest one-shot but
        // the same compose-of-existing-helpers we use elsewhere.
        let tasks = pool
            .with(atrium_core::db::read::list_all_tasks)
            .unwrap_or_default();
        let project_titles = self.imp().project_titles.borrow().clone();
        let tag_pills: crate::ui::task_list::TagPillMap = pool
            .with(atrium_core::db::read::tag_info_per_task)
            .unwrap_or_default();
        let weak = self.downgrade();
        let on_click = move |task_id: i64| {
            let Some(window) = weak.upgrade() else {
                return;
            };
            let _ = WidgetExt::activate_action(
                &window,
                "win.edit-details-for",
                Some(&task_id.to_variant()),
            );
        };
        let widget =
            crate::ui::agenda::build_page(today, &tasks, &project_titles, &tag_pills, on_click);
        self.imp().agenda_host.set_child(Some(&widget));
    }

    /// Phase 12.5 — open the Calendar Month View. No-op in Simple
    /// Mode (Calendar is Builder-only); the accelerator stays
    /// bound system-wide so users in Builder always get the
    /// shortcut, but it doesn't leak the Builder feature into
    /// Simple's surface.
    pub fn show_calendar(&self) {
        let mode = self.settings().string("mode");
        if mode != "builder" {
            return;
        }
        self.set_active_list(ActiveList::Calendar);
    }

    /// Phase 12.5 — return the cached calendar viewed-month, or
    /// today's first-of-month if the user hasn't navigated yet.
    /// Lazy init keeps the field default-clean (NaiveDate has no
    /// Default).
    fn calendar_viewed_or_today(&self) -> chrono::NaiveDate {
        let cached = self.imp().calendar_viewed.get();
        cached.unwrap_or_else(|| {
            let today = Local::now().date_naive();
            crate::ui::calendar::first_of_month(today)
        })
    }

    /// Phase 12.5 — set the calendar's currently-viewed month and
    /// refresh the page if Calendar is the active view. Always
    /// stores `first_of_month(date)` to keep the field canonical.
    pub fn set_calendar_viewed(&self, date: chrono::NaiveDate) {
        let normalised = crate::ui::calendar::first_of_month(date);
        self.imp().calendar_viewed.set(Some(normalised));
        if matches!(self.active_list(), ActiveList::Calendar) {
            self.refresh_calendar_page();
            self.refresh_page_subtitle();
        }
    }

    /// Phase 12.5 — bump the calendar's viewed-month by ±1.
    pub fn calendar_step_month(&self, forward: bool) {
        let current = self.calendar_viewed_or_today();
        let next = if forward {
            crate::ui::calendar::next_month(current)
        } else {
            crate::ui::calendar::previous_month(current)
        };
        self.set_calendar_viewed(next);
    }

    /// Phase 12.5 — jump back to today's month.
    pub fn calendar_jump_to_today(&self) {
        let today = Local::now().date_naive();
        self.set_calendar_viewed(today);
    }

    /// Re-render only the page-title subtitle (used by
    /// `set_calendar_viewed` so the month/year header tracks nav
    /// without a full set_active_list pass).
    fn refresh_page_subtitle(&self) {
        let active = self.active_list();
        let subtitle = self.subtitle_for(&active);
        let lbl = &self.imp().page_subtitle_label;
        lbl.set_label(&subtitle);
        lbl.set_visible(!subtitle.is_empty());
    }

    /// Phase 12.5 — rebuild the Calendar Month View from the read
    /// pool and mount it into `calendar_host`. Called from
    /// `refresh_active_list` when Calendar becomes active and from
    /// the nav helpers above when the user pages months.
    fn refresh_calendar_page(&self) {
        let Some(pool) = self.read_pool() else {
            self.imp().calendar_host.set_child(None::<&gtk::Widget>);
            return;
        };
        let today = Local::now().date_naive();
        let viewed = self.calendar_viewed_or_today();
        // Calendar uses every open task with a scheduled date —
        // load all tasks and let `build_month_grid` filter. The
        // month-scoped query would be tighter, but the worst-case
        // count (10K open tasks * map-by-date) is still fast.
        let tasks = pool
            .with(atrium_core::db::read::list_all_tasks)
            .unwrap_or_default();
        let weak_prev = self.downgrade();
        let on_prev = move || {
            if let Some(win) = weak_prev.upgrade() {
                win.calendar_step_month(false);
            }
        };
        let weak_next = self.downgrade();
        let on_next = move || {
            if let Some(win) = weak_next.upgrade() {
                win.calendar_step_month(true);
            }
        };
        let weak_today = self.downgrade();
        let on_today = move || {
            if let Some(win) = weak_today.upgrade() {
                win.calendar_jump_to_today();
            }
        };
        let weak_pick = self.downgrade();
        let on_pick = move |year: i32, month: u32| {
            let Some(win) = weak_pick.upgrade() else {
                return;
            };
            if let Some(d) = chrono::NaiveDate::from_ymd_opt(year, month, 1) {
                win.set_calendar_viewed(d);
            }
        };
        let weak_click = self.downgrade();
        let on_row_click = move |task_id: i64| {
            let Some(win) = weak_click.upgrade() else {
                return;
            };
            let _ = WidgetExt::activate_action(
                &win,
                "win.edit-details-for",
                Some(&task_id.to_variant()),
            );
        };
        // Double-click on a calendar cell drills into the standard
        // list view scoped to `scheduled:<DATE>`. Reuses the
        // SearchResults active list so the user gets the full
        // editing affordances (drag, multi-select, complete) on
        // the day's tasks rather than being stuck in the calendar
        // peek-popover.
        let weak_drill = self.downgrade();
        let on_day_drill = move |target: chrono::NaiveDate| {
            let Some(win) = weak_drill.upgrade() else {
                return;
            };
            let expr = format!("scheduled:{}", target.format("%Y-%m-%d"));
            win.set_active_list(ActiveList::SearchResults(expr));
        };
        let compact = self.default_width() > 0
            && self.default_width() < crate::ui::calendar::COMPACT_WIDTH_THRESHOLD;
        let widget = crate::ui::calendar::build_page(
            viewed,
            today,
            &tasks,
            self.worker(),
            compact,
            crate::ui::calendar::CalendarCallbacks {
                on_prev,
                on_next,
                on_today,
                on_pick_month: on_pick,
                on_row_click,
                on_day_drill,
            },
        );
        self.imp().calendar_host.set_child(Some(&widget));
    }

    /// v0.6.0 (Slice D1 GUI) — rebuild the kanban board page for a
    /// saved Perspective whose `renderer = "board"`. The grouping
    /// engine lives in `atrium_core::render`; this method just
    /// orchestrates the load + group + mount. Returns `Ok(())`
    /// when the page renders, `Err` when the perspective's renderer
    /// config is malformed (caller falls back to list rendering).
    fn refresh_board_page(
        &self,
        perspective: &atrium_core::Perspective,
    ) -> Result<(), atrium_core::RendererError> {
        let renderer = atrium_core::Renderer::from_columns(
            &perspective.renderer,
            perspective.renderer_config.as_deref(),
        )?;
        let cfg = match renderer {
            atrium_core::Renderer::Board(cfg) => cfg,
            atrium_core::Renderer::List => return Ok(()),
        };
        let Some(pool) = self.read_pool() else {
            self.imp().board_host.set_child(None::<&gtk::Widget>);
            return Ok(());
        };
        // Same load shape as the list-renderer perspective path —
        // run the saved filter expression, apply sorts / bm25 /
        // SQL fast-path, then group. v0.6.6 — use the SQL
        // translation evaluator (shipped at v0.5.3) to push the
        // filter to SQLite when expressible. At fixture scale
        // (~1000 tasks, ~870 open) the in-memory path was loading
        // every row + iterating in Rust on every drop. The SQL
        // path lets us skip both the round-trip cost and the
        // per-row evaluator work for the 80% of expressions that
        // translate cleanly.
        let parsed = crate::ui::filter::parse(&perspective.filter_expr);
        let today = Local::now().date_naive();
        let tag_map: HashMap<i64, Vec<String>> = pool
            .with(atrium_core::db::read::tag_names_per_task)
            .unwrap_or_default();
        let project_titles = self.imp().project_titles.borrow().clone();
        let project_areas = self.project_areas_map();
        let area_titles = self.imp().area_titles.borrow().clone();
        let mut filtered: Vec<atrium_core::Task> = if let Some(expr) = &parsed.expr
            && let Some(clause) = atrium_search::try_translate(expr, today)
        {
            // SQL fast-path: load only the matching rows. Saves the
            // load-everything + iterate-in-Rust cost on every drop.
            let params: Vec<atrium_core::SqlBindValue> =
                clause.params.iter().map(Into::into).collect();
            pool.with(|conn| atrium_core::db::read::list_tasks_matching(conn, &clause.sql, &params))
                .unwrap_or_default()
        } else {
            // Fallback: load all + in-memory filter (regex / fuzzy
            // / composite is:today / etc. that the translator
            // doesn't yet cover).
            let tasks = pool
                .with(atrium_core::db::read::list_all_tasks)
                .unwrap_or_default();
            crate::ui::filter::apply(
                tasks,
                &parsed,
                today,
                &tag_map,
                &project_titles,
                &project_areas,
                &area_titles,
            )
        };
        // Apply explicit `sort:` modifiers when present. Mirrors
        // what filter::apply would have done on the fallback path.
        if !parsed.sorts.is_empty() {
            // sort_tasks lives on filter::apply's path; the SQL
            // path skips it. Re-sort here so both paths agree.
            crate::ui::filter::sort_tasks_by_specs(&mut filtered, &parsed.sorts);
        }
        // bm25 ranking still applies inside a board's rows when the
        // saved expression has bare text and no explicit sort.
        if parsed.sorts.is_empty()
            && let Some(expr) = &parsed.expr
        {
            let terms = atrium_search::collect_text_terms(expr);
            if !terms.is_empty() {
                let scores = pool
                    .with(|conn| atrium_core::db::read::bm25_for_terms(conn, &terms))
                    .unwrap_or_default();
                if !scores.is_empty() {
                    crate::ui::filter::rank_by_bm25_recency(&mut filtered, &scores, today);
                }
            }
        }
        let columns = atrium_core::group_into_board(&filtered, &cfg, &tag_map);

        // Tag pills + worker handle for the row's secondary metadata
        // line and interactive checkbox. The pill map carries the
        // colour each tag was configured with so the kanban renders
        // the same Pango-coloured pills the regular list does.
        let tag_pills: crate::ui::task_list::TagPillMap = pool
            .with(atrium_core::db::read::tag_info_per_task)
            .unwrap_or_default();
        let worker = self.worker();

        // Click → open the task in the Inspector. Reuses the
        // already-wired `win.edit-details-for(i64)` action that the
        // regular list and the keyboard shortcut both go through.
        let weak_click = self.downgrade();
        let on_click = move |task_id: i64| {
            let Some(window) = weak_click.upgrade() else {
                return;
            };
            // Disambiguate: WidgetExt::activate_action walks up the
            // hierarchy looking for a matching action group; we want
            // the window's own action group (the "win." namespace).
            let _ = WidgetExt::activate_action(
                &window,
                "win.edit-details-for",
                Some(&task_id.to_variant()),
            );
        };

        // v0.6.3 — drag-drop between columns. Each card on the
        // board is a drop target; on a drop we recompute the task's
        // tag set with `atrium_core::move_to_column` and dispatch
        // ensure_tag + set_task_tags through the worker. The pool
        // and worker are re-fetched per drop so the closure stays
        // a plain Fn (no captured borrows of cell-borrowed maps).
        let cfg_for_drop = cfg.clone();
        let weak_drop = self.downgrade();
        let on_drop = move |task_id: i64, dest: crate::ui::board::DropDestination| {
            let Some(window) = weak_drop.upgrade() else {
                return;
            };
            let Some(worker) = window.worker() else {
                return;
            };
            let Some(pool) = window.read_pool() else {
                return;
            };
            let map = pool
                .with(atrium_core::db::read::tag_names_per_task)
                .unwrap_or_default();
            let current = map.get(&task_id).cloned().unwrap_or_default();
            let dest_str: Option<String> = match dest {
                crate::ui::board::DropDestination::Column(n) => Some(n),
                crate::ui::board::DropDestination::Other => None,
            };
            let new_names =
                atrium_core::move_to_column(&current, &cfg_for_drop, dest_str.as_deref());
            // Skip the worker round-trip when nothing actually
            // changed (drop on the same column the task is in).
            if tag_lists_equal_case_insensitive(&current, &new_names) {
                return;
            }
            glib::MainContext::default().spawn_local(async move {
                let mut ids: Vec<i64> = Vec::with_capacity(new_names.len());
                for name in new_names {
                    match worker.ensure_tag(name).await {
                        Ok(t) => ids.push(t.id),
                        Err(e) => warn!(?e, "kanban move ensure_tag failed"),
                    }
                }
                if let Err(e) = worker.set_task_tags(task_id, ids).await {
                    error!(?e, task_id, "kanban move set_task_tags failed");
                }
            });
        };

        let widget = crate::ui::board::build_page(
            &perspective.name,
            &columns,
            &tag_pills,
            &project_titles,
            worker,
            on_click,
            on_drop,
        );
        self.imp().board_host.set_child(Some(&widget));
        Ok(())
    }

    /// Phase 10 — refresh the side pane based on the current
    /// selection. Single-task selection → populate; otherwise →
    /// clear back to the empty-state placeholder.
    ///
    /// v0.1.4 — gated on `mode = builder`. In Simple Mode the pane
    /// host is hidden and `pane.clear()` is held permanently; we
    /// don't want a selection change in Simple Mode to repopulate
    /// the editor with a stale task that the user can't see anyway
    /// (and that would resurface immediately on a flip back to
    /// Builder, ignoring whatever they're actually selecting now).
    fn refresh_inspector_pane(&self) {
        let pane_opt = self.imp().inspector_pane.borrow().clone();
        let Some(pane) = pane_opt else {
            debug!("refresh_inspector_pane: no pane installed yet");
            return;
        };
        // v0.1.6 — read from the synchronous Cell instead of
        // round-tripping through GSettings. apply_mode is the only
        // writer; the value is set before any of apply_mode's
        // sibling work runs, so any callback that lands here in
        // the same event-loop iteration sees the just-flipped
        // mode (which the GSettings string compare in v0.1.5 was
        // sometimes missing).
        let builder = self.imp().current_mode_is_builder.get();
        if !builder {
            debug!("refresh_inspector_pane: simple mode → clear");
            pane.clear();
            return;
        }
        let selected = self.selected_task_ids();
        if selected.len() != 1 {
            debug!(
                n = selected.len(),
                "refresh_inspector_pane: not 1-selected → clear"
            );
            pane.clear();
            return;
        }
        let id = selected[0];
        if pane.current_task_id() == Some(id) {
            debug!(
                id,
                "refresh_inspector_pane: already showing this task → noop"
            );
            return;
        }
        let Some(pool) = self.read_pool() else {
            debug!("refresh_inspector_pane: no read pool yet");
            return;
        };
        let task = match pool.with(|c| atrium_core::db::read::task_by_id(c, id)) {
            Ok(Some(t)) => t,
            _ => {
                debug!(id, "refresh_inspector_pane: task not found → clear");
                pane.clear();
                return;
            }
        };
        let projects = pool
            .with(atrium_core::db::read::list_projects)
            .unwrap_or_default();
        let tag_count = pool
            .with(|c| atrium_core::db::read::tag_ids_for_task(c, id))
            .unwrap_or_default()
            .len();
        debug!(id, "refresh_inspector_pane: set_task");
        pane.set_task(task, projects, tag_count);
    }

    /// Open the per-task tag editor for `task_id` (Phase 7g).
    /// Loads the current tag set + the full tag library from the
    /// read pool, then hands off to `ui::tag_editor::open` which
    /// owns the dialog lifecycle and dispatches the apply call.
    pub fn open_tag_editor_for(&self, task_id: i64) {
        let Some(pool) = self.read_pool() else {
            return;
        };
        let Some(worker) = self.worker() else {
            return;
        };
        let task = match pool.with(|conn| atrium_core::db::read::task_by_id(conn, task_id)) {
            Ok(Some(t)) => t,
            Ok(None) => {
                error!(task_id, "tag editor: task not found");
                return;
            }
            Err(e) => {
                error!(?e, task_id, "tag editor: task_by_id failed");
                return;
            }
        };
        let current_tag_ids = pool
            .with(|conn| atrium_core::db::read::tag_ids_for_task(conn, task_id))
            .unwrap_or_default();
        let all_tags = pool
            .with(atrium_core::db::read::list_tags)
            .unwrap_or_default();
        crate::ui::tag_editor::open(self, worker, task_id, task.title, current_tag_ids, all_tags);
    }

    /// `Ctrl+T` shortcut + right-click menu entry point — operates
    /// on the focused / first-selected task. No-op if nothing is
    /// selected.
    pub fn edit_tags_focused(&self) {
        if let Some(id) = self.focused_task_id() {
            self.open_tag_editor_for(id);
        }
    }

    fn wire_search_bar(&self) {
        let bar = self.imp().search_bar.clone();
        let entry = self.imp().search_entry.clone();
        let button = self.imp().search_button.clone();
        let help_button = self.imp().search_help_button.clone();

        // v0.6.9 — register the SearchEntry as the bar's input. GTK
        // emits "The search bar does not have an entry connected to
        // it. Call gtk_search_bar_connect_entry() to connect one."
        // on every captured key event when this isn't done. The
        // `key-capture-widget=task_list_view` property on the bar
        // forwards keystrokes; without `connect_entry` they have
        // nowhere to land. Our entry sits inside a wrapper Box (so
        // the `?` help button can sit alongside), so the bar can't
        // auto-discover it as a direct child.
        bar.connect_entry(&entry);

        // Hook the toggle button to the search bar's search-mode.
        button
            .bind_property("active", &bar, "search-mode-enabled")
            .sync_create()
            .bidirectional()
            .build();

        // v0.4.1 — operator-reference popover. Attaches to the `?`
        // GtkMenuButton in the search bar; click opens a structured
        // quick-reference for the search expression language. The
        // popover content is built once at wire time; subsequent
        // opens reuse the same widget.
        help_button.set_popover(Some(&build_search_help_popover()));

        // search-changed fires after `search-delay` ms (set in .ui).
        // We use it as our debounced input.
        entry.connect_search_changed(clone!(
            #[weak(rename_to = win)]
            self,
            move |entry| {
                let q = entry.text().to_string();
                if q.trim().is_empty() {
                    // If search bar is open and user cleared the
                    // text, fall back to Today rather than rendering
                    // empty results. Also clear any standing
                    // filter-warning fingerprint so next typo
                    // re-toasts, and clear any warning styling on
                    // the entry.
                    win.imp().last_filter_warning.replace(None);
                    entry.remove_css_class("warning");
                    if matches!(win.active_list(), ActiveList::SearchResults(_)) {
                        win.set_active_list(ActiveList::Today);
                        win.select_sidebar_row_for(ActiveList::Today);
                    }
                    return;
                }
                // v0.2.2 — flag obvious typos before the SELECT runs.
                // The parsed FilterQuery is computed cheaply; the
                // warning toast deduplicates against the last
                // fingerprint so successive refreshes don't spam.
                //
                // v0.4.0 — also tint the search entry with the
                // libadwaita `.warning` accent when the expression
                // has unknown tokens. Removed when the user fixes
                // the typo.
                let parsed = crate::ui::filter::parse(&q);
                win.surface_filter_warnings(&parsed);
                if parsed.warnings.is_empty() {
                    entry.remove_css_class("warning");
                } else {
                    entry.add_css_class("warning");
                }
                // v0.4.1 — push the committed query onto the history
                // ring buffer (de-duped against the most recent entry,
                // capped at SEARCH_HISTORY_MAX). Reset the navigation
                // cursor — typing always represents "fresh search,"
                // not "I'm browsing through history."
                {
                    let mut history = win.imp().search_history.borrow_mut();
                    push_history_entry(&mut history, q.clone(), SEARCH_HISTORY_MAX);
                }
                win.imp().search_history_cursor.replace(None);
                win.set_active_list(ActiveList::SearchResults(q));
            }
        ));

        // v0.4.1 — search-history navigation. ↑ recalls the previous
        // query, ↓ moves toward newer / current. The handler reads
        // and mutates `search_history_cursor`; cycle_history_cursor
        // is a pure-Rust helper so the logic is unit-testable.
        let key_ctrl = gtk::EventControllerKey::new();
        key_ctrl.connect_key_pressed(clone!(
            #[weak(rename_to = win)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, _| {
                let direction = match key {
                    gtk::gdk::Key::Up => HistoryDirection::Older,
                    gtk::gdk::Key::Down => HistoryDirection::Newer,
                    _ => return glib::Propagation::Proceed,
                };
                let entry = win.imp().search_entry.clone();
                let history = win.imp().search_history.borrow().clone();
                let cursor = *win.imp().search_history_cursor.borrow();
                let next = cycle_history_cursor(cursor, history.len(), direction);
                win.imp().search_history_cursor.replace(next);
                if let Some(idx) = next
                    && let Some(text) = history.get(idx)
                {
                    // set_text re-fires the search-changed handler,
                    // which pushes onto history. The dedup-against-
                    // last-entry guard in push_history_entry keeps
                    // that from snowballing.
                    entry.set_text(text);
                    entry.set_position(-1);
                }
                glib::Propagation::Stop
            }
        ));
        entry.add_controller(key_ctrl);

        // Esc inside the entry closes the bar.
        entry.connect_stop_search(clone!(
            #[weak]
            bar,
            #[weak]
            button,
            move |_| {
                bar.set_search_mode(false);
                button.set_active(false);
            }
        ));
    }

    /// Public action target — `Ctrl+F` opens the search bar and
    /// focuses the entry.
    pub fn focus_search(&self) {
        self.imp().search_bar.set_search_mode(true);
        self.imp().search_button.set_active(true);
        self.imp().search_entry.grab_focus();
    }

    /// Show a toast with an Undo button. The undo closure runs at
    /// most once — whichever of the toast button or the `Ctrl+Z`
    /// accel (Phase 7f) fires first consumes it. Default 6 s timeout.
    /// Phase 7b's daily-driver safety net.
    /// Generic toast helper. Used for non-undo notifications like
    /// the filter-parse warning surface. Times out at 4 seconds —
    /// long enough to read, short enough not to linger.
    pub fn show_toast(&self, message: &str) {
        let toast = adw::Toast::new(message);
        toast.set_timeout(4);
        self.imp().toast_overlay.add_toast(toast);
    }

    /// v0.2.2 — surface unknown `key:value` tokens in a search /
    /// perspective expression as a toast so users notice typos
    /// (`tga:foo`) instead of having the filter silently no-op.
    /// Deduplicated against `last_filter_warning` so refreshes of
    /// the same query (e.g. TaskChanges arrivals on a SearchResults
    /// view) don't re-toast.
    pub fn surface_filter_warnings(&self, parsed: &crate::ui::filter::FilterQuery) {
        if parsed.warnings.is_empty() {
            // Clear the cell so the same warning re-toasts later if
            // the user edits and re-types the same typo.
            self.imp().last_filter_warning.replace(None);
            return;
        }
        // De-duplicate by joined-warning fingerprint. Same fingerprint
        // = same bad input, don't re-toast.
        let fingerprint = parsed.warnings.join(" ");
        if self.imp().last_filter_warning.borrow().as_ref() == Some(&fingerprint) {
            return;
        }
        self.imp().last_filter_warning.replace(Some(fingerprint));
        let preview = parsed.warnings.iter().take(3).cloned().collect::<Vec<_>>();
        let suffix = if parsed.warnings.len() > preview.len() {
            format!(" (+{} more)", parsed.warnings.len() - preview.len())
        } else {
            String::new()
        };
        let message = format!("Unknown filter: {}{}", preview.join(", "), suffix);
        self.show_toast(&message);
    }

    pub fn show_undo_toast<F: FnOnce() + 'static>(&self, message: &str, undo: F) {
        let toast = adw::Toast::new(message);
        toast.set_button_label(Some("Undo"));
        toast.set_timeout(6);
        let cell: UndoCell = Rc::new(RefCell::new(Some(Box::new(undo))));
        // Share the cell with the window so `win.undo` (Ctrl+Z) can
        // take from the same slot.
        self.imp().last_undo.replace(Some(cell.clone()));
        let cell_for_button = cell.clone();
        toast.connect_button_clicked(move |t| {
            if let Some(f) = cell_for_button.borrow_mut().take() {
                f();
            }
            t.dismiss();
        });
        self.imp().toast_overlay.add_toast(toast);
    }

    /// Walk every sidebar row and unparent any stashed context-menu
    /// popover. Idempotent — rows without a stashed popover (the
    /// canonical rows, section headers) are skipped. Phase 8h fix
    /// for the "Finalizing GtkListBoxRow … but it still has children
    /// left" GTK warning. Called from `rebuild_dynamic_sidebar`
    /// before the remove-rows loop, and from `close_request` so the
    /// app close path is also clean.
    fn unparent_sidebar_context_menus(&self) {
        let list_box = self.imp().sidebar_list.clone();
        let mut idx = 0;
        while let Some(row) = list_box.row_at_index(idx) {
            unsafe {
                if let Some(popover) = row.steal_data::<gtk::PopoverMenu>("atrium-context-popover")
                {
                    popover.unparent();
                }
            }
            idx += 1;
        }
    }

    /// Add or remove the `atrium-high-legibility` CSS class on the
    /// window. The matching selector in `data/style.css` swaps the
    /// UI font family to Atkinson Hyperlegible. Phase 8c.
    fn apply_high_legibility(&self, on: bool) {
        if on {
            self.add_css_class("atrium-high-legibility");
        } else {
            self.remove_css_class("atrium-high-legibility");
        }
    }

    /// If a task row holds focus (or is the ancestor / focus-target
    /// inside the list view), flip its title stack into edit mode
    /// and return `true`. Used by F2 (Phase 7f) so the same chord
    /// that renames a sidebar item also opens the title editor on
    /// the focused task row. Replaces the v0.0.36 EditableLabel-based
    /// path; the stack's "edit" page is a plain GtkEntry that we
    /// populate from the bound display label and focus + select-all.
    pub fn start_edit_focused_row(&self) -> bool {
        let Some(focused) = self.focus() else {
            return false;
        };
        if let Some(row) = find_task_row(&focused) {
            return start_edit_on_row(&row);
        }
        false
    }

    /// Invoke the most recent undo callback, if any is still alive.
    /// Bound to `Ctrl+Z` via `win.undo`. Idempotent — once consumed,
    /// the cell stays empty until the next `show_undo_toast`.
    pub fn invoke_last_undo(&self) {
        let cell_opt = self.imp().last_undo.borrow().clone();
        if let Some(cell) = cell_opt
            && let Some(f) = cell.borrow_mut().take()
        {
            f();
        }
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
        // v0.13 Slice 3 — the tab-completion popover gets attached
        // from `attach_data_layer` instead of here so the read
        // pool is guaranteed to exist when tag candidates need
        // fetching.
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
            let Some((title, color)) =
                prompt_for_named_color(&win, "New Area", "Area name", "", None, "Create").await
            else {
                return;
            };
            let Some(worker) = win.worker() else { return };
            if let Err(e) = worker.create_area(NewArea { title, color }).await {
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

    fn prompt_rename_active(&self) {
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
                glib::MainContext::default().spawn_local(async move {
                    let Some((title, color)) = prompt_for_named_color(
                        &win,
                        "Edit Area",
                        "Area name",
                        &current_name,
                        current_color.as_deref(),
                        "Save",
                    )
                    .await
                    else {
                        return;
                    };
                    let Some(worker) = win.worker() else { return };
                    if let Err(e) = worker
                        .update_area(AreaUpdate::new(id).title(title).color(color))
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
                    let Some((name, color)) = prompt_for_named_color(
                        &win,
                        "Edit Tag",
                        "Tag name",
                        &current_name,
                        current_color.as_deref(),
                        "Save",
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
            let Some((name, color)) =
                prompt_for_named_color(&win, "New Tag", "Tag name", "", None, "Create").await
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
    fn prompt_save_perspective(&self) {
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
    fn prompt_configure_renderer(&self) {
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
    fn prompt_edit_perspective(&self) {
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
            self.set_active_list(active.clone());
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
    // Phase 14 — saved perspective from the current search query.
    // Disabled implicitly when not on SearchResults (the action's
    // enabled state tracks the active list).
    library_section.append(
        Some("Save Search as Perspective…"),
        Some("win.save-perspective"),
    );
    menu.append_section(None, &library_section);

    let mode_section = gio::Menu::new();
    let mode_submenu = gio::Menu::new();
    mode_submenu.append(Some("Simple"), Some("app.mode::simple"));
    mode_submenu.append(Some("Builder"), Some("app.mode::builder"));
    mode_section.append_submenu(Some("Mode"), &mode_submenu);
    // Phase 8c — accessibility toggle. Stateful win action backed by
    // the `high-legibility-font` GSetting; the menu surfaces it as a
    // checkable item.
    let accessibility_submenu = gio::Menu::new();
    accessibility_submenu.append(
        Some("Use High-Legibility Font"),
        Some("win.high-legibility-font"),
    );
    mode_section.append_submenu(Some("Accessibility"), &accessibility_submenu);
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

        // Phase 8e — live RSS / heap readout against the spec §8 budget.
        debug_submenu.append(Some("Memory Watch"), Some("app.show-memory-watch"));

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
/// v0.3.0 — six-swatch palette used by the tag-color picker. Hex
/// values were picked from libadwaita's accent palette so they look
/// right in both light and dark themes. The first `(label, None)`
/// entry is the "no colour" option; selecting it stores `NULL` in
/// `tag.color`.
const TAG_COLORS: &[(&str, Option<&str>)] = &[
    ("None", None),
    ("Blue", Some("#3584e4")),
    ("Green", Some("#33d17a")),
    ("Yellow", Some("#e5a50a")),
    ("Orange", Some("#ff7800")),
    ("Red", Some("#e01b24")),
    ("Purple", Some("#9141ac")),
];

/// Prompt for a name + colour. Returns `Some((name, color))` on
/// confirmation; `None` on cancel or empty name. The `color_initial`
/// is matched against the palette; unrecognised colours fall back to
/// "None" in the picker (the underlying value is preserved through
/// the rename if the user doesn't change the picker selection).
///
/// v0.5.0 (Slice B2) generalised over `placeholder` so the same
/// six-swatch picker drives both tag and area new/rename flows.
async fn prompt_for_named_color(
    parent: &impl IsA<gtk::Widget>,
    heading: &str,
    placeholder: &str,
    name_initial: &str,
    color_initial: Option<&str>,
    confirm_label: &str,
) -> Option<(String, Option<String>)> {
    let entry = gtk::Entry::builder()
        .placeholder_text(placeholder)
        .text(name_initial)
        .activates_default(true)
        .build();

    // Swatch row — one toggle button per palette entry.
    let swatches = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(6)
        .halign(gtk::Align::Start)
        .build();
    let group: Rc<RefCell<Option<gtk::ToggleButton>>> = Rc::new(RefCell::new(None));
    let selected_color: Rc<RefCell<Option<String>>> =
        Rc::new(RefCell::new(color_initial.map(str::to_string)));

    for (label, hex) in TAG_COLORS {
        let toggle = gtk::ToggleButton::builder()
            .tooltip_text(*label)
            .width_request(28)
            .height_request(28)
            .build();
        toggle.add_css_class("circular");
        toggle.add_css_class("atrium-swatch");
        if hex.is_some() {
            // Lower-case the colour name → CSS class. style.css defines
            // .atrium-swatch-{blue,green,yellow,orange,red,purple} as
            // coloured circular buttons with a checked-state ring.
            toggle.add_css_class(&format!("atrium-swatch-{}", label.to_ascii_lowercase()));
        } else {
            toggle.set_label("\u{2300}"); // diameter sign as a "no colour" mark
        }
        if let Some(rb) = group.borrow().as_ref() {
            toggle.set_group(Some(rb));
        }
        if group.borrow().is_none() {
            *group.borrow_mut() = Some(toggle.clone());
        }
        // Pre-select if the initial colour matches.
        if hex.map(str::to_string) == color_initial.map(str::to_string) {
            toggle.set_active(true);
        }
        let sel = selected_color.clone();
        let stored = hex.map(str::to_string);
        toggle.connect_toggled(move |b| {
            if b.is_active() {
                *sel.borrow_mut() = stored.clone();
            }
        });
        swatches.append(&toggle);
    }

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();
    body.append(&entry);
    body.append(&swatches);

    let dialog = adw::AlertDialog::new(Some(heading), None);
    dialog.set_extra_child(Some(&body));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("ok", confirm_label);
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("cancel");
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);

    let response = dialog.choose_future(parent).await;
    if response.as_str() == "ok" {
        let text = entry.text().to_string().trim().to_string();
        if text.is_empty() {
            None
        } else {
            Some((text, selected_color.borrow().clone()))
        }
    } else {
        None
    }
}

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

/// v0.6.2 — perspective renderer configuration dialog. Pick `List`
/// or `Board`; when `Board`, edit the comma-separated column list
/// in a single text entry. Returns `(renderer, config_json)` on
/// confirm, `None` on cancel. The config_json is `None` for List
/// or for an empty Board (no columns picked yet); `Some(json)` for
/// a Board with at least one column.
async fn prompt_configure_renderer_dialog(
    parent: &impl IsA<gtk::Widget>,
    perspective: &atrium_core::Perspective,
) -> Option<(String, Option<String>)> {
    // Existing values for sane defaults.
    let is_board = perspective.renderer.eq_ignore_ascii_case("board");
    let existing_cols: Vec<String> = perspective
        .renderer_config
        .as_deref()
        .and_then(|json| atrium_core::BoardConfig::from_json(json).ok())
        .map(|cfg| cfg.columns)
        .unwrap_or_default();
    let existing_cols_text = existing_cols.join(", ");

    // Form layout — a vertical Box holding the two radio toggles
    // and the columns entry. The entry's sensitive-state is bound
    // to the Board radio.
    let form = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();

    let list_radio = gtk::CheckButton::builder()
        .label("List \u{2014} flat task list (default)")
        .active(!is_board)
        .build();
    let board_radio = gtk::CheckButton::builder()
        .label("Board \u{2014} columns by tag")
        .active(is_board)
        .build();
    board_radio.set_group(Some(&list_radio));
    form.append(&list_radio);
    form.append(&board_radio);

    // Columns entry, only sensitive when Board is selected. Comma-
    // separated for now; an editable list-row UI is a polish item.
    let columns_label = gtk::Label::builder()
        .label("Columns (comma-separated tag names):")
        .halign(gtk::Align::Start)
        .build();
    columns_label.add_css_class("dim-label");
    form.append(&columns_label);

    let columns_entry = gtk::Entry::builder()
        .placeholder_text("todo, doing, done")
        .text(&existing_cols_text)
        .activates_default(true)
        .build();
    columns_entry.set_sensitive(is_board);
    form.append(&columns_entry);

    // Wire the radios → entry sensitivity.
    let entry_clone = columns_entry.clone();
    board_radio.connect_active_notify(move |btn| {
        entry_clone.set_sensitive(btn.is_active());
    });

    let dialog = adw::AlertDialog::new(
        Some(&format!("Configure renderer for “{}”", perspective.name)),
        None,
    );
    dialog.set_extra_child(Some(&form));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("ok", "Save");
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("cancel");
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);

    let response = dialog.choose_future(parent).await;
    if response.as_str() != "ok" {
        return None;
    }
    if board_radio.is_active() {
        let raw = columns_entry.text().to_string();
        let columns: Vec<String> = raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if columns.is_empty() {
            // No columns picked — store as Board with NULL config so
            // the kanban renders only the trailing "Other" bucket.
            // The caller's `Renderer::from_columns` rejects NULL on
            // Board, so we instead fall back to List in that case.
            return Some(("list".into(), None));
        }
        let cfg = atrium_core::BoardConfig {
            axis: atrium_core::BoardAxis::Tag,
            columns,
        };
        let json = cfg.to_json().ok();
        Some(("board".into(), json))
    } else {
        // List renderer needs no config — clear renderer_config.
        Some(("list".into(), None))
    }
}

/// v0.7.3 — captured fields from the perspective editor dialog.
/// The caller converts these into either `NewPerspective` (for
/// create flows) or `PerspectiveUpdate` (for edit flows). Empty
/// `name` or `filter_expr` is rejected by the dialog itself; the
/// caller can trust both fields are non-empty.
pub(crate) struct EditedPerspectiveFields {
    pub name: String,
    pub filter_expr: String,
    pub renderer: String,
    pub renderer_config: Option<String>,
}

/// v0.7.3 — perspective editor dialog. Used for both create
/// (`existing = None`) and edit (`existing = Some(&perspective)`).
/// Renders a single AdwAlertDialog form with Name + Filter +
/// Renderer (List / Board radios) + Columns (sensitive only when
/// Board is active). Returns `Some(EditedPerspectiveFields)` on
/// Save, `None` on Cancel or empty Name/Filter.
///
/// Mirrors the renderer-config form shape from
/// `prompt_configure_renderer_dialog` for visual consistency.
async fn prompt_edit_perspective(
    parent: &impl IsA<gtk::Widget>,
    existing: Option<&atrium_core::Perspective>,
) -> Option<EditedPerspectiveFields> {
    let (existing_name, existing_filter, is_board, existing_cols_text) = match existing {
        Some(p) => {
            let is_board = p.renderer.eq_ignore_ascii_case("board");
            let cols: Vec<String> = p
                .renderer_config
                .as_deref()
                .and_then(|json| atrium_core::BoardConfig::from_json(json).ok())
                .map(|cfg| cfg.columns)
                .unwrap_or_default();
            (
                p.name.clone(),
                p.filter_expr.clone(),
                is_board,
                cols.join(", "),
            )
        }
        None => (String::new(), String::new(), false, String::new()),
    };

    let form = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();

    let name_label = gtk::Label::builder()
        .label("Name")
        .halign(gtk::Align::Start)
        .build();
    name_label.add_css_class("dim-label");
    form.append(&name_label);
    let name_entry = gtk::Entry::builder()
        .placeholder_text("e.g. Today + Errands")
        .text(&existing_name)
        .activates_default(true)
        .build();
    form.append(&name_entry);

    let filter_label = gtk::Label::builder()
        .label("Filter expression")
        .halign(gtk::Align::Start)
        .build();
    filter_label.add_css_class("dim-label");
    form.append(&filter_label);
    let filter_entry = gtk::Entry::builder()
        .placeholder_text("e.g. is:open AND tag:errand")
        .text(&existing_filter)
        .build();
    form.append(&filter_entry);

    let renderer_label = gtk::Label::builder()
        .label("Renderer")
        .halign(gtk::Align::Start)
        .build();
    renderer_label.add_css_class("dim-label");
    form.append(&renderer_label);

    let list_radio = gtk::CheckButton::builder()
        .label("List \u{2014} flat task list (default)")
        .active(!is_board)
        .build();
    let board_radio = gtk::CheckButton::builder()
        .label("Board \u{2014} columns by tag")
        .active(is_board)
        .build();
    board_radio.set_group(Some(&list_radio));
    form.append(&list_radio);
    form.append(&board_radio);

    let columns_label = gtk::Label::builder()
        .label("Columns (comma-separated tag names):")
        .halign(gtk::Align::Start)
        .build();
    columns_label.add_css_class("dim-label");
    form.append(&columns_label);

    let columns_entry = gtk::Entry::builder()
        .placeholder_text("todo, doing, done")
        .text(&existing_cols_text)
        .build();
    columns_entry.set_sensitive(is_board);
    form.append(&columns_entry);

    let entry_clone = columns_entry.clone();
    board_radio.connect_active_notify(move |btn| {
        entry_clone.set_sensitive(btn.is_active());
    });

    let heading = if existing.is_some() {
        format!("Edit “{}”", existing_name)
    } else {
        "New perspective".to_string()
    };
    let dialog = adw::AlertDialog::new(Some(&heading), None);
    dialog.set_extra_child(Some(&form));
    dialog.add_response("cancel", "Cancel");
    dialog.add_response("ok", if existing.is_some() { "Save" } else { "Create" });
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("cancel");
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);

    let response = dialog.choose_future(parent).await;
    if response.as_str() != "ok" {
        return None;
    }

    let name = name_entry.text().trim().to_string();
    let filter_expr = filter_entry.text().trim().to_string();
    if name.is_empty() || filter_expr.is_empty() {
        // Empty required field — silently abort. The caller can
        // surface a toast if it wants to nag; for now an empty
        // submission is treated like Cancel.
        return None;
    }

    let (renderer, renderer_config) = if board_radio.is_active() {
        let raw = columns_entry.text().to_string();
        let columns: Vec<String> = raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if columns.is_empty() {
            ("list".into(), None)
        } else {
            let cfg = atrium_core::BoardConfig {
                axis: atrium_core::BoardAxis::Tag,
                columns,
            };
            ("board".into(), cfg.to_json().ok())
        }
    } else {
        ("list".into(), None)
    };

    Some(EditedPerspectiveFields {
        name,
        filter_expr,
        renderer,
        renderer_config,
    })
}

/// True when two tag-name lists hold the same set under case-
/// insensitive comparison. Used by the kanban drop handler to skip
/// a worker round-trip when the user dropped a task on the same
/// column it was already in (the move helper round-trips column
/// tags, so the lists end up identical modulo order).
fn tag_lists_equal_case_insensitive(a: &[String], b: &[String]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut a_lower: Vec<String> = a.iter().map(|s| s.to_ascii_lowercase()).collect();
    let mut b_lower: Vec<String> = b.iter().map(|s| s.to_ascii_lowercase()).collect();
    a_lower.sort();
    b_lower.sort();
    a_lower == b_lower
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

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let head: String = s.chars().take(max_chars).collect();
        format!("{head}…")
    }
}

fn build_canonical_row(active: &ActiveList) -> (gtk::ListBoxRow, gtk::Label) {
    let (row, badge) = sidebar_row(icon_for(active), active.canonical_title(), 8);
    // v0.5.0 — quiet accent colour per canonical list. Each class
    // reaches in via CSS (see data/style.css) and tints only the
    // leading symbolic icon, not the label or the row chrome. The
    // alpha-wrapped libadwaita named colours auto-respect light /
    // dark / high-contrast.
    if let Some(class) = canonical_accent_class(active) {
        row.add_css_class(class);
    }
    (row, badge)
}

/// v0.4.1 — search-history ring buffer cap. Twenty entries is the
/// shell convention (bash/zsh fc default); short enough to navigate
/// with ↑ / ↓ without losing context, long enough to recover the
/// session's worth of queries.
const SEARCH_HISTORY_MAX: usize = 20;

/// v0.4.1 — build the operator-reference popover for the `?` menu
/// button on the search bar. Compact quick-reference, organised by
/// section, with monospace operator examples paired against
/// short descriptions. Sections cover the boolean / field /
/// modifier / comparison / date / state / sort layers of the
/// expression language; spec.md §4.3 is the authoritative deeper
/// reference.
fn build_search_help_popover() -> gtk::Popover {
    // ── Layout: vertical box of sections inside a scrolled window
    //    so a tall reference doesn't push the popover off-screen.
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(14)
        .margin_start(14)
        .margin_end(14)
        .margin_top(14)
        .margin_bottom(14)
        .build();

    let intro = gtk::Label::builder()
        .label("Search expression reference")
        .halign(gtk::Align::Start)
        .build();
    intro.add_css_class("title-4");
    body.append(&intro);

    let sub = gtk::Label::builder()
        .label("Compose freely with AND / OR / NOT and parens.")
        .halign(gtk::Align::Start)
        .wrap(true)
        .build();
    sub.add_css_class("dim-label");
    sub.add_css_class("caption");
    body.append(&sub);

    // Sections — each is (title, [(operator, meaning), …]).
    let sections: &[(&str, &[(&str, &str)])] = &[
        (
            "Boolean",
            &[
                ("a AND b", "both must match (implicit between bare tokens)"),
                ("a OR b", "either matches"),
                ("NOT a / !a", "negation"),
                ("(a OR b) AND c", "parens override precedence"),
            ],
        ),
        (
            "Fields",
            &[
                ("tag:work", "task has a tag matching \"work\""),
                ("area:Personal", "task's project sits under that area"),
                ("project:\"Q3 plans\"", "task lives in that project"),
                ("title:milk / note:foo", "column-scoped text match"),
                ("due: / scheduled: / defer:", "date fields"),
                ("created: / modified: / completed:", "datetime fields"),
                ("estimated:", "numeric (minutes)"),
                ("repeats:true / :false", "has a repeat rule, or doesn't"),
            ],
        ),
        (
            "Match modifiers",
            &[
                ("tag:work", "substring (default, case-insensitive)"),
                ("tag:=work", "exact match"),
                ("tag:~mystery.*", "regex (RE2 syntax)"),
                ("tag:?wrok", "fuzzy (typo / transposition tolerant)"),
                ("tag:true / tag:false", "has any tag, or has none"),
            ],
        ),
        (
            "Comparison & range",
            &[
                ("due:>today", "deadline after today"),
                ("estimated:>=30", "30 minutes or more"),
                ("due:2026-05-01..2026-05-31", "inclusive range"),
            ],
        ),
        (
            "Date keywords",
            &[
                ("today / yesterday / tomorrow", "single days"),
                ("thisweek / lastweek / nextweek", "ISO Mon-start week"),
                ("thismonth / lastmonth / nextmonth", "calendar month"),
                ("thisyear", "calendar year"),
                ("5daysago / 3daysout", "Ndaysago / Ndaysout"),
            ],
        ),
        (
            "State predicates",
            &[
                ("is:open / is:done / is:overdue", "completion state"),
                (
                    "is:scheduled / is:deadline / is:deferred",
                    "has the field set",
                ),
                ("is:repeating / is:archived / is:tagged", "presence flags"),
                (
                    "is:today / is:inbox / is:upcoming",
                    "canonical-list mirrors",
                ),
                ("is:anytime / is:someday", "more list mirrors"),
            ],
        ),
        (
            "Sort",
            &[
                ("sort:KEY", "ascending (due, scheduled, title, …)"),
                ("sort:-KEY", "descending"),
                (
                    "sort:-due sort:title",
                    "primary by deadline desc, ties by title",
                ),
            ],
        ),
    ];

    for (title, rows) in sections {
        body.append(&build_help_section(title, rows));
    }

    let footer = gtk::Label::builder()
        .label("Full reference: spec.md §4.3 · ↑/↓ recall recent searches")
        .halign(gtk::Align::Start)
        .wrap(true)
        .build();
    footer.add_css_class("dim-label");
    footer.add_css_class("caption");
    body.append(&footer);

    let scrolled = gtk::ScrolledWindow::builder()
        .child(&body)
        .min_content_width(420)
        .min_content_height(360)
        .max_content_height(540)
        .propagate_natural_height(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build();

    let popover = gtk::Popover::new();
    popover.set_child(Some(&scrolled));
    popover.set_position(gtk::PositionType::Bottom);
    popover.add_css_class("atrium-search-help");
    popover
}

/// One section in the operator-reference popover: a heading label
/// followed by `op | meaning` rows. Operators land in monospace via
/// the `.monospace` style class so they read as code.
fn build_help_section(title: &str, rows: &[(&str, &str)]) -> gtk::Box {
    let section = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .build();

    let heading = gtk::Label::builder()
        .label(title)
        .halign(gtk::Align::Start)
        .build();
    heading.add_css_class("heading");
    heading.add_css_class("caption");
    heading.add_css_class("atrium-search-help-heading");
    section.append(&heading);

    for (op, meaning) in rows {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .build();
        let op_label = gtk::Label::builder()
            .label(*op)
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .width_chars(28)
            .max_width_chars(28)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build();
        op_label.add_css_class("monospace");
        op_label.add_css_class("caption");
        let meaning_label = gtk::Label::builder()
            .label(*meaning)
            .halign(gtk::Align::Start)
            .xalign(0.0)
            .wrap(true)
            .hexpand(true)
            .build();
        meaning_label.add_css_class("caption");
        meaning_label.add_css_class("dim-label");
        row.append(&op_label);
        row.append(&meaning_label);
        section.append(&row);
    }

    section
}

/// Direction of a single ↑/↓ keypress in the search-history cursor
/// state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryDirection {
    /// ↑ — toward older entries (lower indices in our newest-last vec).
    Older,
    /// ↓ — toward newer / "current" entry.
    Newer,
}

/// Append `entry` to the history buffer, deduplicating against the
/// most-recent entry (so repeatedly running the same query doesn't
/// flood the buffer) and capping at `max` entries (drops from the
/// front when full). Empty / whitespace-only entries are ignored.
fn push_history_entry(history: &mut Vec<String>, entry: String, max: usize) {
    if entry.trim().is_empty() {
        return;
    }
    if history.last().map(String::as_str) == Some(entry.as_str()) {
        return;
    }
    history.push(entry);
    while history.len() > max {
        history.remove(0);
    }
}

/// Compute the next history cursor given the current cursor, the
/// length of the history buffer, and the direction of the ↑/↓ press.
///
/// The state machine treats `None` as "the user is on the live entry"
/// and `Some(n)` as "the user has stepped back to history\[n\]." ↑
/// from `None` lands on the most recent entry; ↓ off the most recent
/// returns to `None` (the live entry, which the search bar already
/// holds).
fn cycle_history_cursor(
    cursor: Option<usize>,
    len: usize,
    direction: HistoryDirection,
) -> Option<usize> {
    if len == 0 {
        return None;
    }
    match (cursor, direction) {
        // Stepping back from the live entry → most recent history.
        (None, HistoryDirection::Older) => Some(len - 1),
        // Already at the oldest entry — clamp.
        (Some(0), HistoryDirection::Older) => Some(0),
        (Some(n), HistoryDirection::Older) => Some(n - 1),
        // Stepping forward past the most recent → live entry.
        (Some(n), HistoryDirection::Newer) if n + 1 >= len => None,
        (Some(n), HistoryDirection::Newer) => Some(n + 1),
        // Stepping forward from the live entry has nowhere to go.
        (None, HistoryDirection::Newer) => None,
    }
}

/// CSS class supplying the canonical-list accent colour. Returned
/// per `ActiveList`; `None` for the lists that intentionally stay
/// neutral (Anytime — "no time pressure" reads as no colour).
fn canonical_accent_class(active: &ActiveList) -> Option<&'static str> {
    match active {
        ActiveList::Inbox => Some("atrium-canonical-inbox"),
        ActiveList::Today => Some("atrium-canonical-today"),
        ActiveList::Upcoming => Some("atrium-canonical-upcoming"),
        ActiveList::Someday => Some("atrium-canonical-someday"),
        ActiveList::Logbook => Some("atrium-canonical-logbook"),
        // v0.6.7 — Agenda / Forecast / Review live in the top tier
        // alongside the canonicals. They each get their own subtle
        // accent so the icons read as a kindred set: Agenda is the
        // urgent/red of an alarm clock; Forecast is the cool blue
        // of a calendar; Review is the green of a checkmark.
        ActiveList::Agenda => Some("atrium-canonical-agenda"),
        ActiveList::Forecast => Some("atrium-canonical-forecast"),
        ActiveList::Calendar => Some("atrium-canonical-calendar"),
        ActiveList::Review => Some("atrium-canonical-review"),
        ActiveList::Anytime => None,
        _ => None,
    }
}

/// v0.6.7 — non-canonical rows that join the top tier (alongside
/// Inbox / Today / etc.). v0.6.16 reordered the trailing block:
///
/// - Agenda: mode-agnostic now-picture across days. Right after
///   Someday so the active-lists block hands off into "broader
///   now" cleanly.
/// - Forecast / Review: Builder-only — calendar projection and
///   project review queue. Sit between Agenda and Logbook so
///   the Builder-mode block reads as a contiguous group.
/// - Logbook: completed past. Always last so the sidebar's top
///   tier ends on "what's done" rather than interrupting the
///   future-facing flow.
fn top_tier_extras(builder: bool) -> Vec<(ActiveList, &'static str)> {
    let mut out: Vec<(ActiveList, &'static str)> = Vec::with_capacity(5);
    out.push((ActiveList::Agenda, "Agenda"));
    if builder {
        out.push((ActiveList::Forecast, "Forecast"));
        out.push((ActiveList::Calendar, "Calendar"));
        out.push((ActiveList::Review, "Review"));
    }
    out.push((ActiveList::Logbook, "Logbook"));
    out
}

fn build_area_row(area: &Area) -> (gtk::ListBoxRow, gtk::Label) {
    let (row, badge) = sidebar_row(icon_for(&ActiveList::Area(area.id)), &area.title, 8);
    if let Some(label) = row
        .child()
        .and_downcast::<gtk::Box>()
        .and_then(|b| b.first_child())
        .and_then(|icon| icon.next_sibling())
        .and_downcast::<gtk::Label>()
    {
        label.add_css_class("heading");
    }
    // v0.5.0 (Slice B2) — when the area has a colour, swap the
    // leading folder icon for a coloured dot. Same pattern as
    // `build_tag_row`'s tag-colour dot. Areas without a colour keep
    // the folder symbol so the sidebar still reads at a glance.
    if let Some(hex) = area.color.as_deref()
        && let Some(row_box) = row.child().and_downcast::<gtk::Box>()
        && let Some(icon) = row_box.first_child().and_downcast::<gtk::Image>()
    {
        let dot = gtk::Box::builder()
            .width_request(12)
            .height_request(12)
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .tooltip_text(hex)
            .build();
        dot.add_css_class("atrium-tag-dot");
        if let Some(class) = swatch_class_for_hex(hex) {
            dot.add_css_class(class);
        }
        row_box.insert_child_after(&dot, Some(&icon));
        row_box.remove(&icon);
    }
    (row, badge)
}

fn build_project_row(project: &Project, indented: bool) -> (gtk::ListBoxRow, gtk::Label) {
    let margin = if indented { 24 } else { 8 };
    sidebar_row(
        icon_for(&ActiveList::Project(project.id)),
        &project.title,
        margin,
    )
}

fn build_tag_row(tag: &Tag) -> (gtk::ListBoxRow, gtk::Label) {
    let (row, badge) = sidebar_row(icon_for(&ActiveList::Tag(tag.id)), &tag.name, 8);
    // v0.3.0 — when the tag has a colour, swap the leading icon for
    // a coloured dot so the sidebar row reads at a glance. The
    // existing CSS swatch classes (`.atrium-swatch-{color}`) supply
    // the dot's fill; we just walk the row's child layout to replace
    // the GtkImage with a small Box that carries the swatch class.
    if let Some(hex) = tag.color.as_deref()
        && let Some(row_box) = row.child().and_downcast::<gtk::Box>()
        && let Some(icon) = row_box.first_child().and_downcast::<gtk::Image>()
    {
        let dot = gtk::Box::builder()
            .width_request(12)
            .height_request(12)
            .valign(gtk::Align::Center)
            .halign(gtk::Align::Center)
            .tooltip_text(hex)
            .build();
        dot.add_css_class("atrium-tag-dot");
        if let Some(class) = swatch_class_for_hex(hex) {
            dot.add_css_class(class);
        }
        row_box.insert_child_after(&dot, Some(&icon));
        row_box.remove(&icon);
    }
    (row, badge)
}

/// Map a stored hex colour back to one of the named swatch classes
/// declared in `style.css`. Returns `None` for hex values outside the
/// palette — the caller can still render a dot, just without the
/// pre-defined background colour (the `.atrium-tag-dot` base class
/// gives it a neutral grey fallback).
fn swatch_class_for_hex(hex: &str) -> Option<&'static str> {
    match hex {
        "#3584e4" => Some("atrium-swatch-blue"),
        "#33d17a" => Some("atrium-swatch-green"),
        "#e5a50a" => Some("atrium-swatch-yellow"),
        "#ff7800" => Some("atrium-swatch-orange"),
        "#e01b24" => Some("atrium-swatch-red"),
        "#9141ac" => Some("atrium-swatch-purple"),
        _ => None,
    }
}

fn build_section_header(label: &str) -> gtk::ListBoxRow {
    let l = gtk::Label::builder()
        .label(label)
        .halign(gtk::Align::Start)
        .margin_start(8)
        .margin_end(8)
        .margin_top(14)
        .margin_bottom(4)
        .build();
    l.add_css_class("dim-label");
    l.add_css_class("caption-heading");
    l.add_css_class("atrium-sidebar-section");
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
    // Accessibility (Phase 8f): name the row for screen readers.
    // The visible Label already announces its text, but the row
    // itself is what `gtk::ListBox` keyboard navigation lands on,
    // so a redundant label keeps SR readout consistent across
    // pointer + keyboard interaction. Tooltips repeat the same
    // text — useful when the label ellipsises.
    row.set_tooltip_text(Some(label));
    row.update_property(&[gtk::accessible::Property::Label(label)]);
    (row, badge)
}

/// Translate an open-task count into an "available-task" count for
/// sidebar badge display in Builder Mode. A sequential project has
/// at most one available task (the head row); a parallel project's
/// available count equals its open count.
fn available_count(open: i64, sequential: bool) -> i64 {
    if sequential && open > 0 { 1 } else { open }
}

/// Set a badge label's text from a count, hiding when zero.
fn apply_badge_label(badge: &gtk::Label, count: i64) {
    if count > 0 {
        badge.set_label(&count.to_string());
        badge.set_visible(true);
        // v0.2.2 — give screen readers the *meaning* of the
        // number, not just the digit. The visible label stays
        // "5"; the accessible label reads as "5 open tasks", so
        // SR users hear "Today, 5 open tasks" instead of "Today,
        // 5". Singular form when count == 1.
        let aria = if count == 1 {
            "1 open task".to_string()
        } else {
            format!("{count} open tasks")
        };
        badge.update_property(&[gtk::accessible::Property::Label(&aria)]);
    } else {
        badge.set_visible(false);
    }
}

/// Walk up from `start` to find an `atrium-task-row` ancestor; if
/// nothing is found upward, walk down through `start`'s children
/// (the focused widget might be a `GtkListItemWidget` whose child
/// is our row Box). Returns the first match, or `None`.
fn find_task_row(start: &gtk::Widget) -> Option<gtk::Widget> {
    let mut current = start.clone();
    loop {
        if current.has_css_class("atrium-task-row") {
            return Some(current);
        }
        match current.parent() {
            Some(p) => current = p,
            None => break,
        }
    }
    fn walk(w: &gtk::Widget) -> Option<gtk::Widget> {
        if w.has_css_class("atrium-task-row") {
            return Some(w.clone());
        }
        let mut child = w.first_child();
        while let Some(c) = child {
            if let Some(found) = walk(&c) {
                return Some(found);
            }
            child = c.next_sibling();
        }
        None
    }
    walk(start)
}

/// Flip the row's title stack into edit mode, populate the entry
/// from the bound display label, and grab + select-all on the
/// entry. Returns true on success, false if the row's stack /
/// label / entry data isn't present (e.g., a row factory recycle
/// where unbind has already run).
pub fn start_edit_on_row(row: &gtk::Widget) -> bool {
    let has_class = row.has_css_class("atrium-task-row");
    unsafe {
        let stack = row
            .data::<gtk::Stack>("atrium-title-stack")
            .map(|p| p.as_ref().clone());
        let label = row
            .data::<gtk::Label>("atrium-title-label")
            .map(|p| p.as_ref().clone());
        let entry = row
            .data::<gtk::Entry>("atrium-title-entry")
            .map(|p| p.as_ref().clone());
        let has_stack = stack.is_some();
        let has_label = label.is_some();
        let has_entry = entry.is_some();
        debug!(
            has_class,
            has_stack, has_label, has_entry, "start_edit_on_row"
        );
        if let (Some(stack), Some(label), Some(entry)) = (stack, label, entry) {
            entry.set_text(&label.label());
            stack.set_visible_child_name("edit");
            entry.grab_focus();
            entry.select_region(0, -1);
            return true;
        }
    }
    false
}

/// Pure visibility computation for the sidebar filter (Phase 7e).
/// Inputs are aligned with `sidebar_targets` / `sidebar_titles`:
///   - `query`: the user's current filter string (case-insensitive).
///   - `canonical_count`: number of always-visible rows at the head.
///   - `targets[i] == None` marks a section header.
///   - `titles[i]` holds the user-visible label for filterable rows
///     (None for canonical and section headers).
///
/// Returns one bool per row. Header rows lift to `true` when any
/// child between them and the next header passes the filter.
fn compute_sidebar_visibility(
    query: &str,
    canonical_count: usize,
    targets: &[Option<ActiveList>],
    titles: &[Option<String>],
) -> Vec<bool> {
    let needle = query.trim().to_ascii_lowercase();
    let mut visible: Vec<bool> = Vec::with_capacity(targets.len());
    for (idx, target) in targets.iter().enumerate() {
        if idx < canonical_count {
            visible.push(true);
        } else if target.is_none() {
            // Section header — provisional false; pass 2 promotes it
            // when one of its children passes.
            visible.push(false);
        } else {
            let label = titles.get(idx).and_then(|t| t.as_ref());
            let v = needle.is_empty()
                || label.is_some_and(|s| s.to_ascii_lowercase().contains(&needle));
            visible.push(v);
        }
    }

    let mut last_header: Option<usize> = None;
    for idx in canonical_count..targets.len() {
        if targets[idx].is_none() {
            last_header = Some(idx);
        } else if visible[idx]
            && let Some(h) = last_header
        {
            visible[h] = true;
        }
    }
    visible
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

    // ── v0.4.1 search-history helpers ──────────────────────────────

    #[test]
    fn push_history_entry_appends_normal_case() {
        let mut h = vec!["a".to_string()];
        push_history_entry(&mut h, "b".into(), 5);
        assert_eq!(h, vec!["a", "b"]);
    }

    #[test]
    fn push_history_entry_dedupes_against_last() {
        let mut h = vec!["a".to_string(), "b".into()];
        push_history_entry(&mut h, "b".into(), 5);
        assert_eq!(h, vec!["a", "b"]);
    }

    #[test]
    fn push_history_entry_does_not_dedupe_non_consecutive() {
        // "a" appears then "b" then "a" again — both "a" entries
        // are kept because they're not adjacent.
        let mut h = vec!["a".to_string(), "b".into()];
        push_history_entry(&mut h, "a".into(), 5);
        assert_eq!(h, vec!["a", "b", "a"]);
    }

    #[test]
    fn push_history_entry_caps_at_max() {
        let mut h: Vec<String> = (0..5).map(|i| format!("q{i}")).collect();
        push_history_entry(&mut h, "q5".into(), 5);
        // Oldest dropped from the front; newest at the end.
        assert_eq!(h, vec!["q1", "q2", "q3", "q4", "q5"]);
    }

    #[test]
    fn push_history_entry_ignores_empty_input() {
        let mut h = vec!["a".to_string()];
        push_history_entry(&mut h, "".into(), 5);
        push_history_entry(&mut h, "   ".into(), 5);
        assert_eq!(h, vec!["a"]);
    }

    #[test]
    fn cycle_history_cursor_empty_history_stays_none() {
        assert_eq!(cycle_history_cursor(None, 0, HistoryDirection::Older), None);
        assert_eq!(cycle_history_cursor(None, 0, HistoryDirection::Newer), None);
    }

    #[test]
    fn cycle_history_cursor_older_from_live_lands_on_most_recent() {
        // history len 3 → most recent index is 2
        assert_eq!(
            cycle_history_cursor(None, 3, HistoryDirection::Older),
            Some(2)
        );
    }

    #[test]
    fn cycle_history_cursor_older_walks_back() {
        assert_eq!(
            cycle_history_cursor(Some(2), 3, HistoryDirection::Older),
            Some(1)
        );
        assert_eq!(
            cycle_history_cursor(Some(1), 3, HistoryDirection::Older),
            Some(0)
        );
    }

    #[test]
    fn cycle_history_cursor_older_clamps_at_oldest() {
        // Already at the oldest entry; ↑ shouldn't underflow.
        assert_eq!(
            cycle_history_cursor(Some(0), 3, HistoryDirection::Older),
            Some(0)
        );
    }

    #[test]
    fn cycle_history_cursor_newer_returns_to_live_past_most_recent() {
        // Walking forward off the end of history → live entry (None).
        assert_eq!(
            cycle_history_cursor(Some(2), 3, HistoryDirection::Newer),
            None
        );
    }

    #[test]
    fn cycle_history_cursor_newer_walks_forward() {
        assert_eq!(
            cycle_history_cursor(Some(0), 3, HistoryDirection::Newer),
            Some(1)
        );
        assert_eq!(
            cycle_history_cursor(Some(1), 3, HistoryDirection::Newer),
            Some(2)
        );
    }

    #[test]
    fn cycle_history_cursor_newer_from_live_stays_live() {
        assert_eq!(cycle_history_cursor(None, 3, HistoryDirection::Newer), None);
    }

    #[test]
    fn primary_menu_includes_debug_section_when_enabled() {
        let menu = build_primary_menu(true);
        // New + Library + Mode + Debug + About sections.
        assert_eq!(menu.n_items(), 5);
    }

    #[test]
    fn sidebar_lists_cover_simple_mode() {
        // v0.6.16 — Logbook moved to the trailing slot of
        // top_tier_extras; CANONICAL_LISTS holds five rows now.
        assert_eq!(CANONICAL_LISTS.len(), 5);
        assert!(CANONICAL_LISTS.contains(&ActiveList::Inbox));
        assert!(CANONICAL_LISTS.contains(&ActiveList::Today));
        assert!(!CANONICAL_LISTS.contains(&ActiveList::Logbook));
    }

    #[test]
    fn top_tier_extras_simple_mode_has_agenda_and_logbook() {
        let extras = top_tier_extras(false);
        // v0.6.16 — Agenda + Logbook trail the canonical set in
        // both modes; Forecast and Review only appear in Builder.
        assert_eq!(extras.len(), 2);
        assert_eq!(extras[0].0, ActiveList::Agenda);
        assert_eq!(extras[1].0, ActiveList::Logbook);
    }

    #[test]
    fn top_tier_extras_builder_mode_inserts_forecast_calendar_and_review() {
        let extras = top_tier_extras(true);
        // Logbook is the trailing bookend; Forecast / Calendar /
        // Review sit between Agenda and Logbook in that order
        // (Calendar slots between Forecast and Review per Phase
        // 12.5's design — paper-calendar lens lives next to the
        // 30-day strip).
        assert_eq!(extras.len(), 5);
        assert_eq!(extras[0].0, ActiveList::Agenda);
        assert_eq!(extras[1].0, ActiveList::Forecast);
        assert_eq!(extras[2].0, ActiveList::Calendar);
        assert_eq!(extras[3].0, ActiveList::Review);
        assert_eq!(extras[4].0, ActiveList::Logbook);
    }

    #[test]
    fn agenda_forecast_review_have_accent_classes() {
        // v0.6.7 — top-tier extras tint their icons just like the
        // canonical rows. Pinning the class names so a future tweak
        // doesn't quietly drop the accent and turn the icons grey.
        assert_eq!(
            canonical_accent_class(&ActiveList::Agenda),
            Some("atrium-canonical-agenda")
        );
        assert_eq!(
            canonical_accent_class(&ActiveList::Forecast),
            Some("atrium-canonical-forecast")
        );
        assert_eq!(
            canonical_accent_class(&ActiveList::Review),
            Some("atrium-canonical-review")
        );
    }

    // Build a fake sidebar layout: 2 canonical, then "Areas" header
    // + 2 areas, then "Tags" header + 2 tags. (We use 2 canonical
    // instead of 6 to keep the fixtures small; the helper takes the
    // canonical count as a parameter.)
    fn fake_sidebar() -> (Vec<Option<ActiveList>>, Vec<Option<String>>) {
        let targets = vec![
            Some(ActiveList::Inbox),    // 0
            Some(ActiveList::Today),    // 1
            None,                       // 2 — Areas header
            Some(ActiveList::Area(10)), // 3 — "Work"
            Some(ActiveList::Area(11)), // 4 — "Home"
            None,                       // 5 — Tags header
            Some(ActiveList::Tag(20)),  // 6 — "errand"
            Some(ActiveList::Tag(21)),  // 7 — "work-focus"
        ];
        let titles = vec![
            None,
            None,
            None,
            Some("Work".into()),
            Some("Home".into()),
            None,
            Some("errand".into()),
            Some("work-focus".into()),
        ];
        (targets, titles)
    }

    #[test]
    fn empty_query_shows_everything() {
        let (t, n) = fake_sidebar();
        let v = compute_sidebar_visibility("", 2, &t, &n);
        assert_eq!(v, vec![true; 8]);
    }

    #[test]
    fn filter_matches_one_section_hides_other_header() {
        let (t, n) = fake_sidebar();
        let v = compute_sidebar_visibility("err", 2, &t, &n);
        // canonical kept; Areas hidden (no match); errand visible,
        // work-focus hidden; Tags header lifted.
        assert_eq!(v[0..2], [true, true]);
        assert!(!v[2]); // Areas header
        assert!(!v[3] && !v[4]); // areas
        assert!(v[5]); // Tags header
        assert!(v[6] && !v[7]);
    }

    #[test]
    fn filter_promotes_header_when_any_child_matches() {
        let (t, n) = fake_sidebar();
        let v = compute_sidebar_visibility("work", 2, &t, &n);
        // "Work" area matches → Areas header lifts.
        // "work-focus" tag matches → Tags header lifts.
        assert!(v[2]); // Areas header
        assert!(v[3]); // Work
        assert!(!v[4]); // Home
        assert!(v[5]); // Tags header
        assert!(!v[6]); // errand
        assert!(v[7]); // work-focus
    }

    #[test]
    fn filter_is_case_insensitive() {
        let (t, n) = fake_sidebar();
        let lower = compute_sidebar_visibility("home", 2, &t, &n);
        let upper = compute_sidebar_visibility("HOME", 2, &t, &n);
        let mixed = compute_sidebar_visibility("HoMe", 2, &t, &n);
        assert_eq!(lower, upper);
        assert_eq!(lower, mixed);
        assert!(lower[4]); // "Home"
    }

    #[test]
    fn no_match_leaves_only_canonical_visible() {
        let (t, n) = fake_sidebar();
        let v = compute_sidebar_visibility("zzzzz", 2, &t, &n);
        assert_eq!(v[0..2], [true, true]);
        assert!(v[2..].iter().all(|b| !b));
    }

    #[test]
    fn whitespace_query_treated_as_empty() {
        let (t, n) = fake_sidebar();
        let v = compute_sidebar_visibility("   ", 2, &t, &n);
        assert_eq!(v, vec![true; 8]);
    }

    // Phase 11 — available-task badge math.

    #[test]
    fn available_parallel_project_shows_open_count() {
        // Parallel project: every open task is available.
        assert_eq!(available_count(0, false), 0);
        assert_eq!(available_count(1, false), 1);
        assert_eq!(available_count(7, false), 7);
    }

    #[test]
    fn available_sequential_project_caps_at_one() {
        // Sequential project: only the head row is available.
        assert_eq!(available_count(0, true), 0);
        assert_eq!(available_count(1, true), 1);
        assert_eq!(available_count(7, true), 1);
    }
}
