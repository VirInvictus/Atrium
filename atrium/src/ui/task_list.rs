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
/// Simple-Mode lists plus the `Project(id)` / `Area(id)` variants
/// added in Phase 5a for clicking through the sidebar hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ActiveList {
    Inbox,
    /// First-run lands on Today per spec — derived `Default` reflects
    /// that, so `Cell<ActiveList>::default()` does the right thing.
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
}

impl ActiveList {
    /// Static label for the canonical lists. Project/Area return a
    /// generic placeholder; the window resolves the real title from
    /// its cache because that requires DB-side data.
    pub fn canonical_title(self) -> &'static str {
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
    /// refresh picks them up. Acceptable for Phase 5a; Phase 5c will
    /// revisit with a smarter applier when drag-to-project lands.
    pub fn task_matches(self, task: &Task, today: NaiveDate) -> bool {
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
                let deadline_match = task.deadline.is_some_and(|d| d <= today);
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
            Self::Project(id) => task.completed_at.is_none() && task.project_id == Some(id),
            // Area aggregates depend on project→area mapping that
            // isn't on the Task. Fall through to refresh-on-update.
            Self::Area(_) => false,
            // Tag membership lives on task_tag, not on Task. Same.
            Self::Tag(_) => false,
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
            .build();
        let title = gtk::EditableLabel::builder()
            .hexpand(true)
            .xalign(0.0)
            .build();
        title.add_css_class("atrium-task-title");

        let tags = gtk::Label::builder().visible(false).build();
        tags.add_css_class("atrium-task-tags");
        tags.add_css_class("dim-label");
        tags.set_ellipsize(pango::EllipsizeMode::End);

        let schedule = gtk::Label::builder().visible(false).build();
        schedule.add_css_class("atrium-task-schedule");
        schedule.add_css_class("dim-label");
        schedule.set_ellipsize(pango::EllipsizeMode::End);

        let deadline = gtk::Label::builder().visible(false).build();
        deadline.add_css_class("atrium-task-deadline");
        deadline.add_css_class("dim-label");

        row.append(&check);
        row.append(&title);
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
        let title = check
            .next_sibling()
            .and_downcast::<gtk::EditableLabel>()
            .expect("title");
        let tags = title
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

        // Bidirectional: GObject property ↔ widget property.
        let bindings = vec![
            task.bind_property("title", &title, "text")
                .sync_create()
                .bidirectional()
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

        // .completed CSS class for the fade transition.
        if task.completed() {
            row.add_css_class("completed");
        } else {
            row.remove_css_class("completed");
        }

        // Wire the user-input handlers. `connect_*_notify` fires on
        // *any* property change including programmatic ones, so we
        // gate by comparing against the model — only fire the worker
        // call when the change came from the widget.
        let task_id = task.id();
        let on_toggle = on_toggle.clone();
        let toggle_handler = check.connect_active_notify(move |b| {
            on_toggle(task_id, b.is_active());
        });

        let on_rename = on_rename.clone();
        let task_for_rename = task.clone();
        let rename_handler = title.connect_changed(move |label| {
            // EditableLabel's `text` property fires on every keystroke
            // in edit mode AND on commit. We only want commits — gate
            // on `editing` being false when the change fires.
            if label.is_editing() {
                return;
            }
            let new = label.text().to_string();
            let old = task_for_rename.title();
            if new != old {
                on_rename(task_id, new);
            }
        });

        // Stash handler IDs so unbind can disconnect — prevents the
        // factory's recycling pool from firing handlers on stale
        // model objects.
        unsafe {
            row.set_data("atrium-toggle-handler", toggle_handler);
            row.set_data("atrium-rename-handler", rename_handler);
            row.set_data("atrium-check", check.clone());
            row.set_data("atrium-title", title.clone());
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
            if let (Some(title), Some(handler)) = (
                row.steal_data::<gtk::EditableLabel>("atrium-title"),
                row.steal_data::<glib::SignalHandlerId>("atrium-rename-handler"),
            ) {
                title.disconnect(handler);
            }
            // Tear down the drag controllers; the next bind installs
            // fresh ones with the new task id captured.
            if let Some(drag_source) = row.steal_data::<gtk::DragSource>("atrium-drag-source") {
                row.remove_controller(&drag_source);
            }
            if let Some(drop_target) = row.steal_data::<gtk::DropTarget>("atrium-drop-target") {
                row.remove_controller(&drop_target);
            }
        }
    });

    factory
}

/// Replace the store contents with `tasks`, populating each
/// `AtriumTask`'s `tag_names_csv` from `tag_map`. Used on list
/// switches and after mutations whose ordering implications are
/// easier to refresh than to compute (e.g., after a `create_task`).
pub fn replace_store_with_tags(store: &gio::ListStore, tasks: &[Task], tag_map: &TagMap) {
    store.remove_all();
    let objects: Vec<glib::Object> = tasks
        .iter()
        .map(|t| {
            let names = tag_map.get(&t.id).cloned().unwrap_or_default();
            AtriumTask::from_task_with_tags(t, &names).upcast()
        })
        .collect();
    store.extend_from_slice(&objects);
}

/// Apply a `TaskChanges` delta to `store`, respecting `active`'s
/// membership filter so toggled-completed tasks leave Today, etc.
/// `tag_map` provides current tag-name strings for tasks added or
/// updated; pass an empty map if tags aren't known here (the row will
/// render with no pills until the next full refresh).
pub fn apply_changes(
    store: &gio::ListStore,
    changes: &TaskChanges,
    active: ActiveList,
    today: NaiveDate,
    tag_map: &TagMap,
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
}
