// SPDX-License-Identifier: MIT
//! Task list rendering — `gio::ListStore` of `AtriumTask`s, a
//! `gtk::SignalListItemFactory` that builds row widgets, and a diff
//! applier that turns `TaskChanges` into in-place ListStore mutations.
//!
//! Per spec §3.2 (single-writer worker) and §3 architecture: the UI
//! never queries the DB directly during normal operation. List loads
//! happen on list-switch (full reload via the read pool); subsequent
//! mutations flow through `TaskChanges` deltas the worker emits, and
//! the applier here keeps the visible store in step *without* a full
//! reload — preserving selection, scroll, and animations.

use std::collections::HashMap;

use atrium_core::db::read::TODAY_DEADLINE_WINDOW_DAYS;
use atrium_core::{ScheduledFor, Task, TaskChanges};
use chrono::NaiveDate;
use gtk::glib;
use gtk::prelude::*;
use gtk::{gdk, gio, pango};

use crate::ui::task_object::AtriumTask;

/// Per-task tag-name lookup used by `replace_store_with_tags` and
/// `apply_changes` to populate row pills. `HashMap<task_id, tag_names>`.
pub type TagMap = HashMap<i64, Vec<String>>;

/// v0.6.18 — derive the name-only `TagMap` from a `TagPillMap`
/// (which carries the same names plus per-tag colours). Saves a
/// second DB roundtrip on call sites that need both maps —
/// `tag_info_per_task` already does the JOIN; reuse its output.
pub fn tag_names_from_pills(pills: &TagPillMap) -> TagMap {
    pills
        .iter()
        .map(|(id, entries)| (*id, entries.iter().map(|(name, _)| name.clone()).collect()))
        .collect()
}

/// v0.5.0 (Slice B2) — list of every CSS class the area-accent
/// renderer might apply to a task row. Used by `apply_area_accent`
/// to clear stale classes before applying a new one (recycled rows
/// can carry the previous task's accent).
const AREA_ACCENT_CLASSES: &[&str] = &[
    "atrium-area-accent-blue",
    "atrium-area-accent-green",
    "atrium-area-accent-yellow",
    "atrium-area-accent-orange",
    "atrium-area-accent-red",
    "atrium-area-accent-purple",
];

/// Map a stored area-colour hex back to one of the named area-accent
/// classes declared in `style.css`. Returns `None` for hex values
/// outside the palette — callers fall through to "no accent" rather
/// than rendering an arbitrary stripe colour.
fn area_accent_class_for_hex(hex: &str) -> Option<&'static str> {
    match hex {
        "#3584e4" => Some("atrium-area-accent-blue"),
        "#33d17a" => Some("atrium-area-accent-green"),
        "#e5a50a" => Some("atrium-area-accent-yellow"),
        "#ff7800" => Some("atrium-area-accent-orange"),
        "#e01b24" => Some("atrium-area-accent-red"),
        "#9141ac" => Some("atrium-area-accent-purple"),
        _ => None,
    }
}

/// Replace the row's current area-accent class with the one matching
/// `hex` (or none if `hex` is empty / unrecognised). Always clears
/// every accent class first to handle row recycling cleanly.
fn apply_area_accent(row: &gtk::Box, hex: &str) {
    for class in AREA_ACCENT_CLASSES {
        row.remove_css_class(class);
    }
    if !hex.is_empty()
        && let Some(class) = area_accent_class_for_hex(hex)
    {
        row.add_css_class(class);
    }
}

/// v0.3.0 — pill-shape tag map: per-task list of `(name, optional
/// hex colour)`. The renderer uses this; `TagMap` (name-only) stays
/// for the filter evaluator's substring-matching path.
pub type TagPillMap = HashMap<i64, Vec<(String, Option<String>)>>;

/// Which list is currently displayed in the content pane. Canonical
/// Simple-Mode lists plus the `Project(id)` / `Area(id)` / `Tag(id)`
/// variants and Phase 7a's `SearchResults(query)` virtual list.
///
/// No longer `Copy` (the `String` payload on `SearchResults` makes
/// that impossible); `Clone` is cheap enough — sidebar dispatch
/// clones an enum once per click.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ActiveList {
    Inbox,
    /// First-run lands on Today per spec — derived `Default` reflects
    /// that, so `RefCell<ActiveList>::default()` does the right thing.
    #[default]
    Today,
    Upcoming,
    Anytime,
    Someday,
    Logbook,
    /// Open tasks scoped to a single project. Title comes from the
    /// window's project-title cache, populated when the sidebar is
    /// built.
    Project(i64),
    /// Open tasks aggregated across an area's projects. Same cache
    /// for title resolution.
    Area(i64),
    /// Open tasks bearing a specific tag. Phase 6a.
    Tag(i64),
    /// Phase 7a: FTS5-backed search results for a user-entered query.
    SearchResults(String),
    /// Phase 10: Builder-only sidebar entries. Forecast and Review
    /// have grown into real pages (Phase 12 / 13); they are still
    /// listed here so the Builder Mode sidebar can dispatch to them.
    Forecast,
    Review,
    /// v0.6.4 (Slice D2) — agenda canonical page. Org-mode-style
    /// chronological view: Overdue / Today / Tomorrow / This Week /
    /// Next Week. Available in both Simple and Builder modes
    /// because the agenda is a pure read view (no Builder-only
    /// concepts surface there).
    Agenda,
    /// Phase 12.5 (v0.11.0) — Calendar Month View. Builder-only
    /// canonical page; paper-calendar lens over the same task
    /// data Forecast and Agenda surface differently. Tasks
    /// bucket by `scheduled_for` only (deadline-only tasks
    /// surface in Forecast / Agenda).
    Calendar,
    /// Phase 14 — saved perspective. The payload is the row id from
    /// the `perspective` table; the window resolves the title and
    /// filter expression from the `perspective_titles` /
    /// `perspective_meta` caches populated when the sidebar is built.
    Perspective(i64),
}

impl ActiveList {
    /// Static label for the canonical lists. Project/Area return a
    /// generic placeholder; the window resolves the real title from
    /// its cache because that requires DB-side data.
    pub fn canonical_title(&self) -> &'static str {
        match self {
            Self::Inbox => "Inbox",
            Self::Today => "Today",
            Self::Upcoming => "Upcoming",
            Self::Anytime => "Anytime",
            Self::Someday => "Someday",
            Self::Logbook => "Logbook",
            Self::Project(_) => "Project",
            Self::Area(_) => "Area",
            Self::Tag(_) => "Tag",
            Self::SearchResults(_) => "Search",
            Self::Forecast => "Forecast",
            Self::Review => "Review",
            Self::Agenda => "Agenda",
            Self::Calendar => "Calendar",
            Self::Perspective(_) => "Perspective",
        }
    }

    /// Does `task` belong in this list right now? Used by the diff
    /// applier to decide whether to add / remove / update an updated
    /// row in place.
    ///
    /// For `Project(id)` and `Area(id)`, membership depends on data
    /// not carried on the `Task` struct (the area's project membership
    /// in particular). Returning `false` here means the diff applier
    /// won't add a newly-arriving task to those views — the next list
    /// refresh picks them up. Acceptable: drag-to-project / move
    /// flows trigger a full list refresh anyway.
    pub fn task_matches(&self, task: &Task, today: NaiveDate) -> bool {
        match self {
            Self::Inbox => task.completed_at.is_none() && task.project_id.is_none(),
            Self::Today => {
                if task.completed_at.is_some() {
                    return false;
                }
                let scheduled_match = matches!(
                    &task.scheduled_for,
                    Some(ScheduledFor::Date(d)) if *d <= today
                );
                // Spec §4.2 (v0.0.38) — deadlines surface in Today
                // for a heads-up window of TODAY_DEADLINE_WINDOW_DAYS,
                // not only for `deadline ≤ today`. Mirrors the SQL
                // in `atrium_core::db::read::list_today`.
                let horizon = today + chrono::Duration::days(TODAY_DEADLINE_WINDOW_DAYS);
                let deadline_match = task.deadline.is_some_and(|d| d <= horizon);
                let not_deferred = task.defer_until.is_none_or(|d| d <= today);
                (scheduled_match || deadline_match) && not_deferred
            }
            Self::Anytime => {
                task.completed_at.is_none()
                    && task.scheduled_for.is_none()
                    && task.defer_until.is_none_or(|d| d <= today)
            }
            Self::Someday => {
                task.completed_at.is_none()
                    && matches!(&task.scheduled_for, Some(ScheduledFor::Someday))
            }
            Self::Upcoming => {
                task.completed_at.is_none()
                    && matches!(
                        &task.scheduled_for,
                        Some(ScheduledFor::Date(d)) if *d > today
                    )
            }
            Self::Logbook => task.completed_at.is_some(),
            Self::Project(id) => task.completed_at.is_none() && task.project_id == Some(*id),
            // Area aggregates depend on project→area mapping that
            // isn't on the Task. Fall through to refresh-on-update.
            Self::Area(_) => false,
            // Tag membership lives on task_tag, not on Task. Same.
            Self::Tag(_) => false,
            // Search relevance is FTS5-side; refresh-on-update.
            Self::SearchResults(_) => false,
            // Forecast / Review render their own pages (calendar
            // axis / project list), so the diff applier never
            // matches a task into them. Perspectives drive a real
            // task list, but membership depends on the saved
            // filter expression — refresh-on-update covers it.
            Self::Forecast
            | Self::Review
            | Self::Agenda
            | Self::Calendar
            | Self::Perspective(_) => false,
        }
    }
}

/// Build a `SignalListItemFactory` that produces task-row widgets.
///
/// The row layout (Phase 4 baseline):
///
/// ```text
/// [✓ check]   [editable label title]   [date pill]   [⏰ deadline]
/// ```
///
/// `tag_pills` come in Phase 6 with the tag editor. Notes editor
/// comes in Phase 10 with the Inspector.
pub fn build_factory<ToggleFn, RenameFn, ReorderFn>(
    on_toggle: ToggleFn,
    on_rename: RenameFn,
    on_reorder: ReorderFn,
) -> gtk::SignalListItemFactory
where
    ToggleFn: Fn(i64, bool) + Clone + 'static,
    RenameFn: Fn(i64, String) + Clone + 'static,
    ReorderFn: Fn(i64, i64) + Clone + 'static,
{
    let factory = gtk::SignalListItemFactory::new();

    factory.connect_setup(move |_, item| {
        let item: &gtk::ListItem = item.downcast_ref().expect("ListItem");

        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .margin_start(12)
            .margin_end(12)
            // v0.7.0 — vertical rhythm bump (6 → 9). Brandon's
            // whitespace pass: Things 3 / OmniFocus leave real air
            // between rows, Linux apps habitually do not. The change
            // adds 6 px of total vertical breathing per row without
            // touching density on the row content itself.
            .margin_top(9)
            .margin_bottom(9)
            .build();
        row.add_css_class("atrium-task-row");

        let check = gtk::CheckButton::builder()
            .css_classes(["selection-mode"])
            .tooltip_text("Toggle complete (Space)")
            .build();
        // Accessibility (Phase 8f): screen readers report the
        // CheckButton as a "checkbox", but they need a name to
        // announce. Updating the `LABEL` accessible property gives
        // them one without showing a visible label next to the
        // circle.
        check.update_property(&[gtk::accessible::Property::Label("Task complete")]);

        // Title is a `GtkStack` with two pages: a non-editable
        // display Label, and an Entry for inline rename. v0.0.36
        // and earlier used a `GtkEditableLabel`, but that widget's
        // built-in click gesture intercepted single + double clicks
        // on the title text — so clicks on the title couldn't reach
        // the row's selection model or its double-click-opens-the-
        // Inspector gesture (you'd accidentally enter edit mode
        // depending on cursor position). Splitting into two named
        // pages cleanly separates "render the title" from "rename
        // the task": F2 / right-click → Rename swaps to the Entry,
        // Enter / focus-out commits, Esc cancels. Single clicks on
        // the title text just bubble up to the row's selection
        // gesture like every other part of the row.
        let title_stack = gtk::Stack::builder()
            .hexpand(true)
            .transition_type(gtk::StackTransitionType::None)
            .build();
        let title_label = gtk::Label::builder()
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(pango::EllipsizeMode::End)
            .build();
        title_label.add_css_class("atrium-task-title");
        title_label.update_property(&[gtk::accessible::Property::Label("Task title")]);
        let title_entry = gtk::Entry::builder().hexpand(true).build();
        title_entry.add_css_class("atrium-task-title");
        title_entry.update_property(&[gtk::accessible::Property::Label("Task title")]);
        title_stack.add_named(&title_label, Some("display"));
        title_stack.add_named(&title_entry, Some("edit"));
        title_stack.set_visible_child_name("display");

        // v0.3.0 — tags label renders Pango markup so per-pill
        // colours can ship as inline <span foreground="…"> tokens.
        // Format helper lives in this module; window-side wires the
        // colour resolver through the row factory's bind path.
        let tags = gtk::Label::builder()
            .visible(false)
            .use_markup(true)
            .build();
        tags.add_css_class("atrium-task-tags");
        tags.add_css_class("dim-label");
        tags.set_ellipsize(pango::EllipsizeMode::End);

        // Cross-list context chip: "Area › Project" surfaces the
        // task's hierarchy on views that don't already heading it
        // (Today / Inbox / Anytime / Someday / Logbook / Tag /
        // Forecast / Perspective). Suppressed on Project / Area
        // views by the window-side resolver returning an empty
        // string. Ellipsizes to keep long area + project names
        // from pushing the schedule/deadline pills off-screen on
        // narrow windows.
        let context = gtk::Label::builder().visible(false).build();
        context.add_css_class("atrium-task-context");
        context.add_css_class("dim-label");
        context.set_ellipsize(pango::EllipsizeMode::End);
        context.set_max_width_chars(40);

        // Schedule / deadline pills hold short, fixed-shape text
        // ("May 7", "Due May 15"). Earlier versions set
        // `ellipsize=End` on both, which combined with the title's
        // hexpand starvation produced rows that read just "May" —
        // the day-of-month was being chopped. They now render at
        // their natural width; the title pays the ellipsis cost.
        let schedule = gtk::Label::builder().visible(false).build();
        schedule.add_css_class("atrium-task-schedule");
        schedule.add_css_class("dim-label");

        let deadline = gtk::Label::builder().visible(false).build();
        deadline.add_css_class("atrium-task-deadline");
        deadline.add_css_class("dim-label");

        // v0.6.14 — small recurrence icon for tasks whose
        // `repeat_rule` is set. Lives at the tail of the row so the
        // existing next_sibling chain in `bind` doesn't shift.
        let repeat_icon = gtk::Image::from_icon_name("view-refresh-symbolic");
        repeat_icon.set_tooltip_text(Some("Repeating task"));
        repeat_icon.set_visible(false);
        repeat_icon.add_css_class("atrium-task-repeating");
        repeat_icon.add_css_class("dim-label");

        row.append(&check);
        row.append(&title_stack);
        row.append(&tags);
        row.append(&context);
        row.append(&schedule);
        row.append(&deadline);
        row.append(&repeat_icon);

        item.set_child(Some(&row));
    });

    factory.connect_bind(move |_, item| {
        let item: &gtk::ListItem = item.downcast_ref().expect("ListItem");
        let task = item
            .item()
            .and_downcast::<AtriumTask>()
            .expect("AtriumTask");
        let row = item.child().and_downcast::<gtk::Box>().expect("row Box");

        // Walk the children. Layout order is fixed per `setup`.
        let check = row
            .first_child()
            .and_downcast::<gtk::CheckButton>()
            .expect("check");
        let title_stack = check
            .next_sibling()
            .and_downcast::<gtk::Stack>()
            .expect("title stack");
        let title_label = title_stack
            .child_by_name("display")
            .and_downcast::<gtk::Label>()
            .expect("display label");
        let title_entry = title_stack
            .child_by_name("edit")
            .and_downcast::<gtk::Entry>()
            .expect("edit entry");
        let tags = title_stack
            .next_sibling()
            .and_downcast::<gtk::Label>()
            .expect("tags");
        let context = tags
            .next_sibling()
            .and_downcast::<gtk::Label>()
            .expect("context");
        let schedule = context
            .next_sibling()
            .and_downcast::<gtk::Label>()
            .expect("schedule");
        let deadline = schedule
            .next_sibling()
            .and_downcast::<gtk::Label>()
            .expect("deadline");
        let repeat_icon = deadline
            .next_sibling()
            .and_downcast::<gtk::Image>()
            .expect("repeat icon");

        // Title bindings: model → display label is one-way. The
        // entry is populated from the label only when edit mode
        // begins (see `start_edit_focused_row` in window.rs); commit
        // routes through the rename callback below.
        let bindings = vec![
            task.bind_property("title", &title_label, "label")
                .sync_create()
                .build(),
            task.bind_property("completed", &check, "active")
                .sync_create()
                .bidirectional()
                .build(),
            task.bind_property("tag-names-csv", &tags, "label")
                .sync_create()
                .build(),
            task.bind_property("context-label", &context, "label")
                .sync_create()
                .build(),
            task.bind_property("schedule-label", &schedule, "label")
                .sync_create()
                .build(),
            task.bind_property("deadline-label", &deadline, "label")
                .sync_create()
                .build(),
        ];
        // Stash bindings so `unbind` can drop them and we don't leak.
        unsafe {
            row.set_data("atrium-bindings", bindings);
        }

        // Empty schedule/deadline/tags labels hide the widget so the
        // row stays clean. Schedule and deadline only flip on
        // `refresh_from` (which fires their `_label` notify); the
        // tags label needs an explicit notify hook because adding
        // or removing a tag is a `tag-names-csv` property change
        // that arrives independently of the schedule/deadline path.
        schedule.set_visible(!task.schedule_label().is_empty());
        deadline.set_visible(!task.deadline_label().is_empty());
        tags.set_visible(!task.tag_names_csv().is_empty());
        context.set_visible(!task.context_label().is_empty());

        let tags_for_notify = tags.clone();
        let tags_handler = task.connect_tag_names_csv_notify(move |t| {
            tags_for_notify.set_visible(!t.tag_names_csv().is_empty());
        });
        let context_for_notify = context.clone();
        let context_handler = task.connect_context_label_notify(move |t| {
            context_for_notify.set_visible(!t.context_label().is_empty());
        });

        // Always start a freshly-bound row in display mode so a
        // recycled row doesn't carry a previous task's edit state.
        title_stack.set_visible_child_name("display");

        // .completed CSS class for the fade transition.
        if task.completed() {
            row.add_css_class("completed");
        } else {
            row.remove_css_class("completed");
        }

        // v0.6.12 (Patch B) — state-aware row class for the
        // checkbox + date pill colouring. The four state classes
        // are mutually exclusive; we drop all three before adding
        // the current one so a row that flips between states (a
        // task gets toggled-completed, then re-opened past its
        // deadline) doesn't carry stale classes.
        for stale in [
            "atrium-task-row-overdue",
            "atrium-task-row-today",
            "atrium-task-row-upcoming",
        ] {
            row.remove_css_class(stale);
        }
        match task.row_state().as_str() {
            "overdue" => row.add_css_class("atrium-task-row-overdue"),
            "today" => row.add_css_class("atrium-task-row-today"),
            "upcoming" => row.add_css_class("atrium-task-row-upcoming"),
            _ => {}
        }
        let row_for_state = row.clone();
        let state_handler = task.connect_row_state_notify(move |t| {
            for stale in [
                "atrium-task-row-overdue",
                "atrium-task-row-today",
                "atrium-task-row-upcoming",
            ] {
                row_for_state.remove_css_class(stale);
            }
            match t.row_state().as_str() {
                "overdue" => row_for_state.add_css_class("atrium-task-row-overdue"),
                "today" => row_for_state.add_css_class("atrium-task-row-today"),
                "upcoming" => row_for_state.add_css_class("atrium-task-row-upcoming"),
                _ => {}
            }
        });

        // v0.6.14 — show the repeat icon for tasks whose
        // `repeat_rule` is set. Initial visibility from the
        // property; notify keeps it in sync when the user adds /
        // removes a repeat rule via the Inspector.
        repeat_icon.set_visible(task.repeating());
        let repeat_icon_for_notify = repeat_icon.clone();
        let repeating_handler = task.connect_repeating_notify(move |t| {
            repeat_icon_for_notify.set_visible(t.repeating());
        });

        // Phase 11 — .queued CSS class dims sequential-project
        // rows past the first incomplete one. Window populates the
        // `queued` property on each AtriumTask before it lands in
        // the store; the factory mirrors the bool to the row class
        // on bind, plus listens for runtime flips (e.g., the head
        // task gets completed and the next promotes to "available").
        if task.queued() {
            row.add_css_class("queued");
        } else {
            row.remove_css_class("queued");
        }
        let row_for_queued = row.clone();
        let queued_handler = task.connect_queued_notify(move |t| {
            if t.queued() {
                row_for_queued.add_css_class("queued");
            } else {
                row_for_queued.remove_css_class("queued");
            }
        });

        // v0.5.0 (Slice B2) — area-accent stripe. The window-side
        // resolver writes `area_color` (hex string, empty for none)
        // before the row binds; we mirror it to the matching
        // `.atrium-area-accent-{color}` CSS class. The notify hook
        // keeps the class in step when a task moves between projects
        // (and thus possibly a different area colour) via the diff
        // applier.
        apply_area_accent(&row, &task.area_color());
        let row_for_accent = row.clone();
        let area_color_handler = task.connect_area_color_notify(move |t| {
            apply_area_accent(&row_for_accent, &t.area_color());
        });

        // Wire the user-input handlers. `connect_*_notify` fires on
        // *any* property change including programmatic ones, so we
        // gate by comparing against the model — only fire the worker
        // call when the change came from the widget.
        let task_id = task.id();
        let on_toggle = on_toggle.clone();
        let toggle_handler = check.connect_active_notify(move |b| {
            on_toggle(task_id, b.is_active());
        });

        // Inline-rename commit handlers. Enter dispatches a rename;
        // losing focus while the entry is showing also commits
        // (Things-3-style autosave); Esc reverts to the bound label
        // text and flips the stack back without writing.
        let activate_handler = title_entry.connect_activate({
            let on_rename = on_rename.clone();
            let task_for_rename = task.clone();
            let stack = title_stack.clone();
            move |entry| {
                let new = entry.text().to_string();
                let old = task_for_rename.title();
                if new != old {
                    on_rename(task_id, new);
                }
                stack.set_visible_child_name("display");
            }
        });

        let focus_ctrl = gtk::EventControllerFocus::new();
        let focus_handler = focus_ctrl.connect_leave({
            let entry = title_entry.clone();
            let stack = title_stack.clone();
            let on_rename = on_rename.clone();
            let task_for_rename = task.clone();
            move |_| {
                let visible = stack.visible_child_name();
                tracing::debug!(
                    task_id,
                    visible_child = ?visible.as_deref(),
                    "title-entry focus-leave"
                );
                // Only fire when we're actually in edit mode —
                // recycled rows traverse the controller during bind
                // even though the entry isn't on screen.
                if visible.as_deref() != Some("edit") {
                    return;
                }
                let new = entry.text().to_string();
                let old = task_for_rename.title();
                if new != old {
                    on_rename(task_id, new);
                }
                stack.set_visible_child_name("display");
            }
        });
        title_entry.add_controller(focus_ctrl.clone());

        let key_ctrl = gtk::EventControllerKey::new();
        key_ctrl.connect_key_pressed({
            let stack = title_stack.clone();
            let label = title_label.clone();
            let entry = title_entry.clone();
            move |_, key, _, _| {
                if key == gdk::Key::Escape {
                    entry.set_text(&label.label());
                    stack.set_visible_child_name("display");
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
        });
        title_entry.add_controller(key_ctrl.clone());

        // Stash widgets / handlers so unbind can disconnect them
        // and `start_edit_focused_row` (window.rs) can find the
        // stack + entry to flip into edit mode on F2.
        unsafe {
            row.set_data("atrium-toggle-handler", toggle_handler);
            row.set_data("atrium-activate-handler", activate_handler);
            row.set_data("atrium-focus-handler", focus_handler);
            row.set_data("atrium-queued-handler", queued_handler);
            row.set_data("atrium-tags-handler", tags_handler);
            row.set_data("atrium-context-handler", context_handler);
            row.set_data("atrium-area-color-handler", area_color_handler);
            row.set_data("atrium-row-state-handler", state_handler);
            row.set_data("atrium-repeating-handler", repeating_handler);
            row.set_data("atrium-task-obj", task.clone());
            row.set_data("atrium-check", check.clone());
            row.set_data("atrium-title-stack", title_stack.clone());
            row.set_data("atrium-title-label", title_label.clone());
            row.set_data("atrium-title-entry", title_entry.clone());
            row.set_data("atrium-title-focus-ctrl", focus_ctrl);
            row.set_data("atrium-title-key-ctrl", key_ctrl);
        }

        // Drag source: this row reports its task id so a drop
        // target on a sibling can ask the worker to reorder.
        let drag_source = gtk::DragSource::builder()
            .actions(gdk::DragAction::MOVE)
            .build();
        let id_for_drag = task_id;
        drag_source.connect_prepare(move |_, _, _| {
            Some(gdk::ContentProvider::for_value(&id_for_drag.to_value()))
        });
        row.add_controller(drag_source.clone());

        // Drop target: receives the source row's task id; we have
        // our own task id from the bound model, so we know "drop A
        // onto B" = move A to B's position.
        let drop_target = gtk::DropTarget::new(i64::static_type(), gdk::DragAction::MOVE);
        let on_reorder_clone = on_reorder.clone();
        let dest_id = task_id;
        drop_target.connect_drop(move |_, value, _, _| {
            if let Ok(src_id) = value.get::<i64>() {
                if src_id != dest_id {
                    on_reorder_clone(src_id, dest_id);
                }
                return true;
            }
            false
        });
        row.add_controller(drop_target.clone());

        unsafe {
            row.set_data("atrium-drag-source", drag_source);
            row.set_data("atrium-drop-target", drop_target);
        }

        // Phase 7g — right-click context menu. Single entry (Edit
        // Tags…) targeting a parameterized win action so the menu
        // works even when the right-click row isn't part of the
        // current selection. The popover is `set_parent`-ed to the
        // row, so it must be unparented on unbind to avoid the
        // GtkListBoxRow finalizer warning that bit us in Phase 8h.
        let menu_model = gio::Menu::new();
        let edit_details_item = gio::MenuItem::new(Some("Edit Details…"), None);
        edit_details_item
            .set_action_and_target_value(Some("win.edit-details-for"), Some(&task_id.to_variant()));
        menu_model.append_item(&edit_details_item);
        let edit_tags_item = gio::MenuItem::new(Some("Edit Tags…"), None);
        edit_tags_item
            .set_action_and_target_value(Some("win.edit-tags-for"), Some(&task_id.to_variant()));
        menu_model.append_item(&edit_tags_item);
        let popover = gtk::PopoverMenu::from_model(Some(&menu_model));
        popover.set_has_arrow(false);
        popover.set_parent(&row);

        let context_gesture = gtk::GestureClick::new();
        context_gesture.set_button(gdk::BUTTON_SECONDARY);
        let popover_for_gesture = popover.clone();
        context_gesture.connect_pressed(move |_, _, x, y| {
            popover_for_gesture
                .set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            popover_for_gesture.popup();
        });
        row.add_controller(context_gesture.clone());

        // v0.1.10 — primary-button double-click enters inline title
        // edit (same as F2). Single click selects + holds focus
        // (MultiSelection's job, no extra wiring). The Inspector is
        // accessible via Ctrl+I, right-click → *Edit Details…*, or
        // F2; double-click is the lighter "just rename this" path.
        //
        // Earlier versions wired this to `win.edit-details-for(id)`
        // for the modal Inspector, but Brandon found the double-
        // click semantics felt heavy — clicking around to inspect
        // tasks shouldn't always pop a dialog. Things-3 idiom:
        // double-click renames in place, separate affordance opens
        // the full editor.
        let activate_gesture = gtk::GestureClick::new();
        activate_gesture.set_button(gdk::BUTTON_PRIMARY);
        // v0.1.11 — explicit Capture phase. The Bubble default
        // wasn't firing reliably because the parent
        // GtkListItemWidget's selection-handling gesture was
        // consuming events before they bubbled up to us.
        activate_gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
        // v0.1.12+ — own time-window double-click detection. GTK's
        // `gtk-double-click-time` defaults to 400 ms; natural
        // trackpad cadence often lands at 600–900 ms, so the
        // system never registered those as a double. 800 ms is
        // generous for users who don't double-click at machine
        // speed, still tight enough that "click then click 850 ms
        // later" doesn't accidentally collapse two distinct
        // intents into a double.
        //
        // v0.1.13 — gate the match on "not already editing": once
        // the title stack is on its `edit` page, further clicks
        // (within or outside the window) shouldn't re-fire
        // start_edit_on_row, which would reset the entry's text
        // and cursor underneath whatever the user is typing. This
        // also defends against trackpad chatter — a stray third
        // click during a fast double won't bounce focus.
        let last_release: std::rc::Rc<std::cell::Cell<Option<std::time::Instant>>> =
            std::rc::Rc::new(std::cell::Cell::new(None));
        activate_gesture.connect_released(move |gesture, n_press, _, _| {
            let now = std::time::Instant::now();
            let prev = last_release.replace(Some(now));
            let is_double_click = prev
                .is_some_and(|p| now.duration_since(p) <= std::time::Duration::from_millis(800));
            // Already-editing check: read the stack's current page
            // off the row Box (gesture.widget() is the row, where
            // we stash the stack on bind).
            let already_editing = gesture
                .widget()
                .map(|w| unsafe {
                    w.data::<gtk::Stack>("atrium-title-stack")
                        .map(|p| p.as_ref().visible_child_name().as_deref() == Some("edit"))
                        .unwrap_or(false)
                })
                .unwrap_or(false);
            tracing::debug!(
                n_press,
                is_double_click,
                already_editing,
                "row activate-gesture released"
            );
            if is_double_click
                && !already_editing
                && let Some(widget) = gesture.widget()
            {
                last_release.set(None);
                // v0.1.14 — defer the edit-start to the next idle
                // tick. The click event is still propagating when
                // this callback fires; GtkListView's internal click
                // handler grabs focus on the row's ListItemWidget
                // *after* we'd have grabbed focus on the entry,
                // which triggers our focus-leave handler and
                // commits + closes the editor before the user sees
                // it. Idle deferral runs us *after* the event
                // settles, so our grab_focus is the last word.
                glib::idle_add_local_once(move || {
                    let did_edit = crate::ui::window::start_edit_on_row(&widget);
                    tracing::debug!(
                        did_edit,
                        "row activate-gesture: start_edit_on_row returned (idle)"
                    );
                });
            }
        });
        row.add_controller(activate_gesture.clone());

        unsafe {
            row.set_data("atrium-context-gesture", context_gesture);
            row.set_data("atrium-context-popover", popover);
            row.set_data("atrium-activate-gesture", activate_gesture);
        }
    });

    factory.connect_unbind(move |_, item| {
        let item: &gtk::ListItem = item.downcast_ref().expect("ListItem");
        let row = item.child().and_downcast::<gtk::Box>().expect("row Box");

        // Drop bindings so they stop syncing properties on a recycled row.
        unsafe {
            let _bindings: Option<Vec<glib::Binding>> = row.steal_data("atrium-bindings");
            if let (Some(check), Some(handler)) = (
                row.steal_data::<gtk::CheckButton>("atrium-check"),
                row.steal_data::<glib::SignalHandlerId>("atrium-toggle-handler"),
            ) {
                check.disconnect(handler);
            }
            // Disconnect the property-notify handlers stashed on
            // the AtriumTask. queued (Phase 11), tag-names-csv (the
            // pill-visibility fix). Both were connected to the
            // task object that's about to be unbound from this row;
            // disconnect so we don't leak handler IDs.
            let task_obj = row.steal_data::<AtriumTask>("atrium-task-obj");
            if let (Some(task), Some(handler)) = (
                task_obj.clone(),
                row.steal_data::<glib::SignalHandlerId>("atrium-queued-handler"),
            ) {
                task.disconnect(handler);
            }
            if let (Some(task), Some(handler)) = (
                task_obj.clone(),
                row.steal_data::<glib::SignalHandlerId>("atrium-tags-handler"),
            ) {
                task.disconnect(handler);
            }
            if let (Some(task), Some(handler)) = (
                task_obj.clone(),
                row.steal_data::<glib::SignalHandlerId>("atrium-context-handler"),
            ) {
                task.disconnect(handler);
            }
            if let (Some(task), Some(handler)) = (
                task_obj.clone(),
                row.steal_data::<glib::SignalHandlerId>("atrium-area-color-handler"),
            ) {
                task.disconnect(handler);
            }
            if let (Some(task), Some(handler)) = (
                task_obj.clone(),
                row.steal_data::<glib::SignalHandlerId>("atrium-row-state-handler"),
            ) {
                task.disconnect(handler);
            }
            if let (Some(task), Some(handler)) = (
                task_obj,
                row.steal_data::<glib::SignalHandlerId>("atrium-repeating-handler"),
            ) {
                task.disconnect(handler);
            }
            // Title entry: disconnect the activate + focus-leave
            // handlers, drop the controllers, drop the cached
            // widget references. The next bind builds fresh.
            let title_entry = row.steal_data::<gtk::Entry>("atrium-title-entry");
            if let (Some(entry), Some(handler)) = (
                title_entry.clone(),
                row.steal_data::<glib::SignalHandlerId>("atrium-activate-handler"),
            ) {
                entry.disconnect(handler);
            }
            if let Some(focus_ctrl) =
                row.steal_data::<gtk::EventControllerFocus>("atrium-title-focus-ctrl")
            {
                if let (Some(entry), Some(handler)) = (
                    title_entry.clone(),
                    row.steal_data::<glib::SignalHandlerId>("atrium-focus-handler"),
                ) {
                    entry.remove_controller(&focus_ctrl);
                    let _ = (entry, handler); // already disconnected via remove_controller
                } else {
                    let _ = focus_ctrl;
                }
            }
            if let Some(key_ctrl) =
                row.steal_data::<gtk::EventControllerKey>("atrium-title-key-ctrl")
                && let Some(entry) = title_entry.clone()
            {
                entry.remove_controller(&key_ctrl);
            }
            let _ = title_entry;
            let _ = row.steal_data::<gtk::Stack>("atrium-title-stack");
            let _ = row.steal_data::<gtk::Label>("atrium-title-label");
            // Tear down the drag controllers; the next bind installs
            // fresh ones with the new task id captured.
            if let Some(drag_source) = row.steal_data::<gtk::DragSource>("atrium-drag-source") {
                row.remove_controller(&drag_source);
            }
            if let Some(drop_target) = row.steal_data::<gtk::DropTarget>("atrium-drop-target") {
                row.remove_controller(&drop_target);
            }
            // Phase 7g — tear down the right-click context menu.
            // The popover was set_parent-ed to the row; it must be
            // unparented before the row recycles or the next bind
            // double-parents and GTK warns.
            if let Some(gesture) = row.steal_data::<gtk::GestureClick>("atrium-context-gesture") {
                row.remove_controller(&gesture);
            }
            if let Some(popover) = row.steal_data::<gtk::PopoverMenu>("atrium-context-popover") {
                popover.unparent();
            }
            // Phase 7j — tear down the activate gesture too.
            if let Some(gesture) = row.steal_data::<gtk::GestureClick>("atrium-activate-gesture") {
                row.remove_controller(&gesture);
            }
        }
    });

    factory
}

/// Replace the store contents with `tasks`, populating each
/// `AtriumTask`'s `tag_names_csv` from `tag_map`. When
/// `sequential` is `true` (Phase 11 — viewing a sequential
/// project), every row past the first incomplete one gets
/// `queued = true` so the factory applies the `.queued` CSS class.
/// The caller is the only thing that knows whether the active list
/// is a sequential project view, so this is a parameter rather
/// than a global.
pub fn replace_store_with_tags_seq<F, G>(
    store: &gio::ListStore,
    tasks: &[Task],
    tag_pills: &TagPillMap,
    sequential: bool,
    context_for: F,
    area_color_for: G,
) where
    F: Fn(&Task) -> String,
    G: Fn(&Task) -> String,
{
    store.remove_all();
    let queued = compute_queued_state(tasks, sequential);
    let objects: Vec<glib::Object> = tasks
        .iter()
        .zip(queued.iter())
        .map(|(t, q)| {
            let pills = tag_pills.get(&t.id).cloned().unwrap_or_default();
            let obj = AtriumTask::from_task_with_tags(t, &pills);
            obj.set_context_label(context_for(t));
            obj.set_area_color(area_color_for(t));
            obj.set_queued(*q);
            obj.upcast()
        })
        .collect();
    store.extend_from_slice(&objects);
}

/// Compute per-task "queued" flags for a sequential project view.
/// Returns one bool per input task. When `sequential` is `false`,
/// all flags are `false` (no queueing). When `true`, the first
/// incomplete task is *not* queued; every subsequent open task is.
/// Completed tasks are never queued — they've already been done,
/// so dimming them on top of the completion fade is noise.
pub fn compute_queued_state(tasks: &[Task], sequential: bool) -> Vec<bool> {
    if !sequential {
        return vec![false; tasks.len()];
    }
    let mut seen_first_open = false;
    tasks
        .iter()
        .map(|t| {
            if t.completed_at.is_some() {
                return false;
            }
            if !seen_first_open {
                seen_first_open = true;
                false
            } else {
                true
            }
        })
        .collect()
}

/// Apply a `TaskChanges` delta to `store`, respecting `active`'s
/// membership filter so toggled-completed tasks leave Today, etc.
/// `tag_map` provides current tag-name strings for tasks added or
/// updated; pass an empty map if tags aren't known here. When
/// `sequential` is true (Phase 11 — sequential project view),
/// recomputes per-row queued state after the diff settles. The
/// first incomplete row is unqueued; the rest are queued. A
/// completion toggle on the head row demotes it to completed and
/// promotes the next row to "available" — the recompute makes the
/// dim/undim transition land in the same frame as the bound
/// `completed` flip.
// v0.5.0 (Slice B2) — eight parameters is past clippy's default
// threshold but each one is genuinely independent state the diff
// applier consults per-update; bundling them into a context struct
// trades a clippy warning for an indirection that doesn't make the
// call site clearer.
#[allow(clippy::too_many_arguments)]
pub fn apply_changes_seq<F, G>(
    store: &gio::ListStore,
    changes: &TaskChanges,
    active: ActiveList,
    today: NaiveDate,
    tag_pills: &TagPillMap,
    sequential: bool,
    context_for: F,
    area_color_for: G,
) where
    F: Fn(&Task) -> String,
    G: Fn(&Task) -> String,
{
    // Created — append rows that belong here.
    for task in &changes.created {
        if active.task_matches(task, today) && find_index(store, task.id).is_none() {
            let pills = tag_pills.get(&task.id).cloned().unwrap_or_default();
            let obj = AtriumTask::from_task_with_tags(task, &pills);
            obj.set_context_label(context_for(task));
            obj.set_area_color(area_color_for(task));
            store.append(&obj);
        }
    }

    // Updated — reconcile each row's presence and contents.
    for task in &changes.updated {
        let idx = find_index(store, task.id);
        let now_matches = active.task_matches(task, today);
        match (idx, now_matches) {
            (Some(i), true) => {
                if let Some(obj) = store.item(i).and_downcast::<AtriumTask>() {
                    obj.refresh_from(task);
                    // Sync tag pills with the latest tag map.
                    let pills = tag_pills.get(&task.id).cloned().unwrap_or_default();
                    obj.set_tag_names_csv(format_tag_names(&pills));
                    // Sync the area/project context chip — the
                    // task may have moved to a different project,
                    // which slides it under a different area.
                    obj.set_context_label(context_for(task));
                    // Sync the area-accent stripe — same reasoning as
                    // the context chip; a project move can change
                    // which area the row paints.
                    obj.set_area_color(area_color_for(task));
                }
            }
            (Some(i), false) => {
                store.remove(i);
            }
            (None, true) => {
                let pills = tag_pills.get(&task.id).cloned().unwrap_or_default();
                let obj = AtriumTask::from_task_with_tags(task, &pills);
                obj.set_context_label(context_for(task));
                obj.set_area_color(area_color_for(task));
                store.append(&obj);
            }
            (None, false) => {}
        }
    }

    // Deleted — remove by id if present.
    for id in &changes.deleted {
        if let Some(i) = find_index(store, *id) {
            store.remove(i);
        }
    }
    // status_changed: the affected tasks are also in `updated` (the
    // worker emits both), so the loop above covered them.

    // Re-sort by position so reorder updates land in the right slot.
    sort_by_position(store);

    // Phase 11 — recompute queued state if this is a sequential
    // project view. Walks the now-sorted store and updates each
    // AtriumTask's `queued` property. The factory has a property
    // notify on `queued` via `bind_property` — we'd love that, but
    // since the property → CSS class isn't bindable, the factory
    // mirrors `queued` to the row class on bind. So a recycled row
    // picks up the new state on its next bind. For rows that are
    // currently bound, we toggle the row class directly via the
    // notify hookup that connect_bind installed. (Implemented by
    // setting `queued` here — bound widgets observe via the
    // glib::Object property system.)
    if sequential {
        recompute_queued_state(store);
    } else {
        // If we left a sequential project (e.g., user toggled
        // sequential off and triggered a refresh), clear queued
        // flags so no row stays dimmed.
        for i in 0..store.n_items() {
            if let Some(obj) = store.item(i).and_downcast::<AtriumTask>() {
                obj.set_queued(false);
            }
        }
    }
}

/// Walk the store and recompute queued flags assuming the active
/// view is a sequential project. The first incomplete task is
/// unqueued; the rest are queued. Completed tasks are never
/// queued (the .completed dim takes precedence).
fn recompute_queued_state(store: &gio::ListStore) {
    let mut seen_first_open = false;
    for i in 0..store.n_items() {
        let Some(obj) = store.item(i).and_downcast::<AtriumTask>() else {
            continue;
        };
        let target = if obj.completed() {
            false
        } else if !seen_first_open {
            seen_first_open = true;
            false
        } else {
            true
        };
        if obj.queued() != target {
            obj.set_queued(target);
        }
    }
}

/// v0.3.0 — render tags as Pango markup. Coloured tags get a
/// `<span foreground="#hex">` wrapper; uncoloured tags render plain
/// (the row-level CSS class supplies the default accent palette).
/// The label widget bound to `tag-names-csv` was upgraded to
/// `use-markup=true` in `connect_setup` so the markup renders.
///
/// Pango markup requires `&` and `<` to be escaped. Tag names are
/// the only user-controlled input here; the color values come from
/// a fixed palette controlled by the swatch picker, so we only need
/// to escape names.
pub fn format_tag_names(pills: &[(String, Option<String>)]) -> String {
    pills
        .iter()
        .map(|(name, color)| {
            let escaped = pango_escape(name);
            match color.as_deref() {
                Some(hex) => format!("<span foreground=\"{hex}\">#{escaped}</span>"),
                None => format!("#{escaped}"),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Escape a string for Pango markup. Pango is XML-style; only `&`
/// and `<` need escaping in attribute-free spans (and `<` only when
/// it'd start a tag, but be safe).
fn pango_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;")
}

/// Sort an `AtriumTask`-bearing store by `position` (ascending).
pub fn sort_by_position(store: &gio::ListStore) {
    store.sort(|a, b| {
        let ta = a.downcast_ref::<AtriumTask>();
        let tb = b.downcast_ref::<AtriumTask>();
        match (ta, tb) {
            (Some(a), Some(b)) => a.position().total_cmp(&b.position()),
            _ => std::cmp::Ordering::Equal,
        }
    });
}

fn find_index(store: &gio::ListStore, id: i64) -> Option<u32> {
    for i in 0..store.n_items() {
        if let Some(obj) = store.item(i).and_downcast::<AtriumTask>()
            && obj.id() == id
        {
            return Some(i);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use atrium_core::test_support::dummy_task as dummy;
    use chrono::{NaiveDate, Utc};

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 5, 15).unwrap()
    }

    #[test]
    fn inbox_matches_unfiled_open_tasks() {
        let mut t = dummy(1);
        assert!(ActiveList::Inbox.task_matches(&t, today()));

        t.project_id = Some(7);
        assert!(!ActiveList::Inbox.task_matches(&t, today()));

        t.project_id = None;
        t.completed_at = Some(Utc::now());
        assert!(!ActiveList::Inbox.task_matches(&t, today()));
    }

    #[test]
    fn today_matches_scheduled_due_today() {
        let mut t = dummy(1);
        t.scheduled_for = Some(ScheduledFor::Date(today()));
        assert!(ActiveList::Today.task_matches(&t, today()));
    }

    #[test]
    fn today_excludes_someday() {
        let mut t = dummy(1);
        t.scheduled_for = Some(ScheduledFor::Someday);
        assert!(!ActiveList::Today.task_matches(&t, today()));
    }

    #[test]
    fn today_excludes_deferred_to_future() {
        let mut t = dummy(1);
        t.scheduled_for = Some(ScheduledFor::Date(today()));
        t.defer_until = Some(NaiveDate::from_ymd_opt(2026, 6, 1).unwrap());
        assert!(!ActiveList::Today.task_matches(&t, today()));
    }

    #[test]
    fn today_includes_deadline_within_window() {
        // Spec §4.2 — deadline 5 days out lands in Today.
        let mut t = dummy(1);
        t.deadline = Some(today() + chrono::Duration::days(5));
        assert!(ActiveList::Today.task_matches(&t, today()));
    }

    #[test]
    fn today_includes_deadline_at_window_edge() {
        let mut t = dummy(1);
        t.deadline = Some(today() + chrono::Duration::days(TODAY_DEADLINE_WINDOW_DAYS));
        assert!(ActiveList::Today.task_matches(&t, today()));
    }

    #[test]
    fn today_excludes_deadline_past_window() {
        let mut t = dummy(1);
        t.deadline = Some(today() + chrono::Duration::days(TODAY_DEADLINE_WINDOW_DAYS + 1));
        assert!(!ActiveList::Today.task_matches(&t, today()));
    }

    // Phase 10 / 12 / 13 / 14 — Builder Mode views. Forecast,
    // Review, and Perspective(id) render their own content; the
    // diff applier never matches a task into them.

    #[test]
    fn builder_views_match_no_tasks() {
        // The diff applier consults task_matches; Forecast/Review
        // own their own page so no task belongs to them, and
        // Perspective membership depends on the saved filter
        // expression which the diff applier doesn't have access to.
        let t = dummy(1);
        assert!(!ActiveList::Forecast.task_matches(&t, today()));
        assert!(!ActiveList::Review.task_matches(&t, today()));
        assert!(!ActiveList::Perspective(1).task_matches(&t, today()));
    }

    #[test]
    fn builder_stub_titles_render() {
        assert_eq!(ActiveList::Forecast.canonical_title(), "Forecast");
        assert_eq!(ActiveList::Review.canonical_title(), "Review");
        assert_eq!(ActiveList::Perspective(1).canonical_title(), "Perspective");
    }

    // Phase 11 — sequential rendering helper.

    #[test]
    fn queued_state_empty_when_not_sequential() {
        let tasks = vec![dummy(1), dummy(2), dummy(3)];
        let q = compute_queued_state(&tasks, false);
        assert_eq!(q, vec![false, false, false]);
    }

    #[test]
    fn queued_state_first_open_unqueued_rest_queued() {
        let tasks = vec![dummy(1), dummy(2), dummy(3)];
        let q = compute_queued_state(&tasks, true);
        assert_eq!(q, vec![false, true, true]);
    }

    #[test]
    fn queued_state_skips_completed_for_first_open() {
        // First task is done; second is the first open task — so
        // it's *not* queued. Third is queued.
        let mut t1 = dummy(1);
        t1.completed_at = Some(Utc::now());
        let tasks = vec![t1, dummy(2), dummy(3)];
        let q = compute_queued_state(&tasks, true);
        assert_eq!(q, vec![false, false, true]);
    }

    #[test]
    fn queued_state_all_completed_no_queue() {
        // Every task is done — nothing is queued (all `false`).
        let mut t1 = dummy(1);
        t1.completed_at = Some(Utc::now());
        let mut t2 = dummy(2);
        t2.completed_at = Some(Utc::now());
        let q = compute_queued_state(&[t1, t2], true);
        assert_eq!(q, vec![false, false]);
    }
}
