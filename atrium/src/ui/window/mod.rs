// SPDX-License-Identifier: MIT
//! `AtriumWindow` — the application's `gtk::ApplicationWindow` subclass
//! (Phase 22 C8 reparented it off `gtk::ApplicationWindow`).
//!
//! Phase 4 turns the static sidebar / placeholder content from Phase 3
//! into a real working surface:
//!
//! - Sidebar is built programmatically so we can attach click handlers
//!   and (Phase 5+) count badges.
//! - Content pane hosts a `GtkStack` between an empty-state
//!   status page (the owned `status_page` composite) and a
//!   `GtkListView` rendering tasks via the `task_list` factory.
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
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{CompositeTemplate, gio, glib};
use tracing::{debug, error, warn};

use crate::ui::task_list::{
    ActiveList, TagMap, apply_nesting, build_factory, replace_store_with_tags_seq,
};

/// Shared cell used by both the undo toast button and the `Ctrl+Z`
/// accel (Phase 7f). The inner `Option` is the still-alive callback
/// (consumed by whichever path fires first); the outer level lets
/// the cell be replaced wholesale every time `show_undo_toast` runs.
type UndoCell = Rc<RefCell<Option<Box<dyn FnOnce()>>>>;

mod imp {
    use super::*;

    #[derive(Default, CompositeTemplate)]
    #[template(file = "../../../../data/window.ui")]
    pub struct AtriumWindow {
        #[template_child]
        pub overlay_split: TemplateChild<gtk::Paned>,
        #[template_child]
        pub inspector_pane_host: TemplateChild<gtk::Box>,
        #[template_child]
        pub split_view: TemplateChild<gtk::Paned>,
        #[template_child]
        pub menu_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub bulk_schedule_button: TemplateChild<gtk::MenuButton>,
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
        pub content_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub task_list_view: TemplateChild<gtk::ListView>,
        /// Host box for the owned empty-state page (Phase 22 C2). The
        /// `adw::StatusPage` that used to live here in the template was
        /// replaced by [`crate::ui::status_page::StatusPage`], built in
        /// code at setup and parented into this box; the owned page
        /// itself lives in `content_status` below.
        #[template_child]
        pub content_status_host: TemplateChild<gtk::Box>,
        #[template_child]
        pub forecast_host: TemplateChild<gtk::Box>,
        #[template_child]
        pub review_host: TemplateChild<gtk::Box>,
        /// v0.6.0 (Slice C2) — Logbook page host. Window mounts the
        /// day-band layout from `logbook::build_page` here whenever
        /// `ActiveList::Logbook` is selected.
        #[template_child]
        pub logbook_host: TemplateChild<gtk::Box>,
        /// v0.6.0 (Slice D1 GUI) — kanban board page host. Window
        /// mounts the column layout from `board::build_page` here
        /// whenever the active Perspective has `renderer = "board"`.
        #[template_child]
        pub board_host: TemplateChild<gtk::Box>,
        /// v0.6.4 (Slice D2) — Agenda canonical page host. Window
        /// mounts the chronological-section layout from
        /// `agenda::build_page` here whenever `ActiveList::Agenda`
        /// is selected.
        #[template_child]
        pub agenda_host: TemplateChild<gtk::Box>,
        /// Phase 12.5 (v0.11.0) — Calendar Month View host.
        /// Window mounts the 7×N grid from `calendar::build_page`
        /// here whenever `ActiveList::Calendar` is selected.
        /// Builder-only.
        #[template_child]
        pub calendar_host: TemplateChild<gtk::Box>,
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
        /// Owned toast host (Phase 22 C3) — the crossfading revealer, its
        /// label, and its optional Undo button, replacing `adw::Toast` /
        /// `AdwToastOverlay`. Driven by `show_toast` / `show_undo_toast`.
        #[template_child]
        pub toast_revealer: TemplateChild<gtk::Revealer>,
        #[template_child]
        pub toast_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub toast_button: TemplateChild<gtk::Button>,
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

        /// The owned empty-state page parented into `content_status_host`
        /// (Phase 22 C2). Built once at setup; `update_empty_state` swaps
        /// its title / description / icon per active list.
        pub content_status: OnceCell<crate::ui::status_page::StatusPage>,

        pub debug_enabled: Cell<bool>,
        /// v0.31.0 — cached "the library has nothing in it" flag
        /// (no tasks, no projects, no areas). Drives the first-run
        /// onboarding page; recomputed on every task / library change.
        pub db_empty: Cell<bool>,
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
        /// v0.28.0 — per-area default Review cadence cache (days or
        /// None). The Edit Area dialog reads it to pre-fill the
        /// review-interval row.
        pub area_review_intervals: RefCell<HashMap<i64, Option<i64>>>,

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

        /// Phase 22 C3 — pending auto-hide timer for the owned toast.
        /// A new toast cancels the old timer (newest-wins) so a burst of
        /// confirmations keeps the latest up for its full window.
        pub toast_timeout: RefCell<Option<glib::SourceId>>,

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
        /// v0.20.0 — Phase 19.5 reminder service handle. `None`
        /// until `attach_reminder_service` runs at boot (the
        /// service needs the read pool, which `attach_data_layer`
        /// supplies). The bridge_task_changes path calls
        /// `wake_reminder_service` after every batch so a fresh
        /// reminder takes effect immediately.
        pub reminder_service: RefCell<Option<crate::reminders::ReminderService>>,
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
        type ParentType = gtk::ApplicationWindow;

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
}

glib::wrapper! {
    pub struct AtriumWindow(ObjectSubclass<imp::AtriumWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow,
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
    pub fn new(app: &gtk::Application, debug: bool) -> Self {
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
}

#[cfg(test)]
mod tests;

mod actions;
mod drop;
mod lists;
mod onboarding;
mod search;
mod shell;
mod sidebar;
mod tasks;
mod views;

mod widgets;
pub(crate) use widgets::*;
