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
    /// Phase 10: Builder-only sidebar entries. Each renders an
    /// `AdwStatusPage` placeholder pointing at the phase that owns
    /// the actual content; selecting one is non-destructive (no
    /// task list query runs).
    Forecast,
    Review,
    Perspectives,
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
            Self::Perspectives => "Perspectives",
        }
    }

    /// Builder-only Phase 10 stub views — no task list, just a
    /// placeholder page. The window dispatches these to a status
    /// page instead of running a list query.
    pub fn is_builder_stub(&self) -> bool {
        matches!(self, Self::Forecast | Self::Review | Self::Perspectives)
    }

    /// Does `task` belong in this list right now? Used by the diff
    /// applier to decide whether to add / remove / update an updated
    /// row in place.
    ///
    /// For `Project(id)` and `Area(id)`, membership depends on data
    /// not carried on the `Task` struct (the area's project membership
    /// in particular). Returning `false` here means the diff applier
    /// won't add a newly-arriving task to those views — the next list
    /// refresh picks them up. Acceptable for Phase 5a; Phase 5c will
    /// revisit with a smarter applier when drag-to-project lands.
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
            // Phase 10 Builder stubs render a static placeholder —
            // no task list, so no task can match.
            Self::Forecast | Self::Review | Self::Perspectives => false,
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
            .margin_top(6)
            .margin_bottom(6)
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

        let tags = gtk::Label::builder().visible(false).build();
        tags.add_css_class("atrium-task-tags");
        tags.add_css_class("dim-label");
        tags.set_ellipsize(pango::EllipsizeMode::End);

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

        row.append(&check);
        row.append(&title_stack);
        row.append(&tags);
        row.append(&schedule);
        row.append(&deadline);

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
        let schedule = tags
            .next_sibling()
            .and_downcast::<gtk::Label>()
            .expect("schedule");
        let deadline = schedule
            .next_sibling()
            .and_downcast::<gtk::Label>()
            .expect("deadline");

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

        // Empty schedule/deadline label hides the widget so the row
        // stays clean.
        schedule.set_visible(!task.schedule_label().is_empty());
        deadline.set_visible(!task.deadline_label().is_empty());

        // Always start a freshly-bound row in display mode so a
        // recycled row doesn't carry a previous task's edit state.
        title_stack.set_visible_child_name("display");

        // .completed CSS class for the fade transition.
        if task.completed() {
            row.add_css_class("completed");
        } else {
            row.remove_css_class("completed");
        }

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
                // Only fire when we're actually in edit mode —
                // recycled rows traverse the controller during bind
                // even though the entry isn't on screen.
                if stack.visible_child_name().as_deref() != Some("edit") {
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
        activate_gesture.connect_released(move |gesture, n_press, _, _| {
            if n_press == 2
                && let Some(widget) = gesture.widget()
            {
                crate::ui::window::start_edit_on_row(&widget);
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
            // Phase 11 — disconnect the queued-notify handler too.
            if let (Some(task_obj), Some(handler)) = (
                row.steal_data::<AtriumTask>("atrium-task-obj"),
                row.steal_data::<glib::SignalHandlerId>("atrium-queued-handler"),
            ) {
                task_obj.disconnect(handler);
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
pub fn replace_store_with_tags_seq(
    store: &gio::ListStore,
    tasks: &[Task],
    tag_map: &TagMap,
    sequential: bool,
) {
    store.remove_all();
    let queued = compute_queued_state(tasks, sequential);
    let objects: Vec<glib::Object> = tasks
        .iter()
        .zip(queued.iter())
        .map(|(t, q)| {
            let names = tag_map.get(&t.id).cloned().unwrap_or_default();
            let obj = AtriumTask::from_task_with_tags(t, &names);
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
pub fn apply_changes_seq(
    store: &gio::ListStore,
    changes: &TaskChanges,
    active: ActiveList,
    today: NaiveDate,
    tag_map: &TagMap,
    sequential: bool,
) {
    // Created — append rows that belong here.
    for task in &changes.created {
        if active.task_matches(task, today) && find_index(store, task.id).is_none() {
            let names = tag_map.get(&task.id).cloned().unwrap_or_default();
            store.append(&AtriumTask::from_task_with_tags(task, &names));
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
                    let names = tag_map.get(&task.id).cloned().unwrap_or_default();
                    obj.set_tag_names_csv(format_tag_names(&names));
                }
            }
            (Some(i), false) => {
                store.remove(i);
            }
            (None, true) => {
                let names = tag_map.get(&task.id).cloned().unwrap_or_default();
                store.append(&AtriumTask::from_task_with_tags(task, &names));
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

fn format_tag_names(names: &[String]) -> String {
    names
        .iter()
        .map(|n| format!("#{n}"))
        .collect::<Vec<_>>()
        .join(" ")
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
    use chrono::{NaiveDate, Utc};

    fn dummy(id: i64) -> Task {
        Task {
            id,
            uuid: format!("uuid-{id}"),
            title: format!("Task {id}"),
            note: String::new(),
            project_id: None,
            parent_id: None,
            scheduled_for: None,
            deadline: None,
            defer_until: None,
            estimated_minutes: None,
            completed_at: None,
            repeat_rule: None,
            position: id as f64,
            created_at: Utc::now(),
            modified_at: Utc::now(),
        }
    }

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

    // Phase 10 — Builder Mode stub views.

    #[test]
    fn builder_stubs_report_themselves() {
        assert!(ActiveList::Forecast.is_builder_stub());
        assert!(ActiveList::Review.is_builder_stub());
        assert!(ActiveList::Perspectives.is_builder_stub());
    }

    #[test]
    fn non_builder_variants_are_not_stubs() {
        assert!(!ActiveList::Inbox.is_builder_stub());
        assert!(!ActiveList::Today.is_builder_stub());
        assert!(!ActiveList::Project(7).is_builder_stub());
        assert!(!ActiveList::Tag(3).is_builder_stub());
        assert!(!ActiveList::SearchResults("milk".into()).is_builder_stub());
    }

    #[test]
    fn builder_stubs_match_no_tasks() {
        // Phase 10 stubs render an AdwStatusPage placeholder; no
        // task can belong to them. The diff applier consults
        // task_matches so it doesn't accidentally append rows to
        // the empty content stack.
        let t = dummy(1);
        assert!(!ActiveList::Forecast.task_matches(&t, today()));
        assert!(!ActiveList::Review.task_matches(&t, today()));
        assert!(!ActiveList::Perspectives.task_matches(&t, today()));
    }

    #[test]
    fn builder_stub_titles_render() {
        assert_eq!(ActiveList::Forecast.canonical_title(), "Forecast");
        assert_eq!(ActiveList::Review.canonical_title(), "Review");
        assert_eq!(ActiveList::Perspectives.canonical_title(), "Perspectives");
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
