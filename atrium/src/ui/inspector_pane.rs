// SPDX-License-Identifier: MIT
//! Builder Mode Inspector side pane (Phase 10).
//!
//! Companion pane that lives on the right of `AdwOverlaySplitView`
//! and renders the full task editor whenever a row is selected. The
//! Phase 7i modal Inspector dialog is still the path Simple Mode
//! uses (Ctrl+I, double-click); this pane is the Builder analogue
//! and stays visible as long as Builder Mode is on.
//!
//! Differences vs the dialog:
//!
//! - **Always-visible.** No present/close lifecycle. The pane
//!   swaps between an empty-state placeholder and a per-task
//!   editor depending on the active selection.
//! - **Auto-save.** Each field commits on focus-out / Enter via
//!   the same worker calls the dialog's Apply button uses. Things-3
//!   semantics; Ctrl+Z still reverses any commit.
//! - **Builder-only fields exposed.** `estimated_minutes` is a live
//!   `gtk::SpinButton`; `defer_until` and `repeat_rule` ship as
//!   disabled placeholder rows that name the phase that finishes
//!   them (11 and 15). "No new logic — just exposure" per the
//!   roadmap Phase 10 tagline.
//!
//! The pane host (`AdwBin id="inspector_pane_host"`) is declared in
//! `data/window.ui`; `install` mounts the body widget into it on
//! window startup. `set_task` swaps the contents.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use atrium_core::db::read_pool::ReadPool;
use atrium_core::{
    Project, RepeatMode, RepeatRule, ScheduledFor, Task, TaskClockEntry, TaskUpdate, WorkerHandle,
    parse_body_checkboxes, parse_body_links, toggle_body_checkbox,
};
use chrono::{NaiveDate, TimeZone};
use gtk::gio;
use gtk::glib;
use gtk::glib::clone;
use gtk::pango;
use tracing::error;

use crate::ui::inspector::{format_deadline_label, format_defer_label, format_schedule_label};

/// Shared state mounted into the pane host. Keeps the empty-state
/// page + the editor page in a single `gtk::Stack` and exposes
/// `set_task` so the window can swap content as the selection
/// changes. The `current_task_id` cell short-circuits redundant
/// rebuilds when the same task is selected twice.
pub struct InspectorPane {
    stack: gtk::Stack,
    editor_host: adw::Bin,
    current_task_id: RefCell<Option<i64>>,
    /// The current editor's title `EntryRow`, stashed so
    /// `focus_title()` can hand keyboard focus to it without
    /// walking the widget tree. Cleared on `clear()` and replaced
    /// on every `set_task` rebuild.
    current_title_row: RefCell<Option<adw::EntryRow>>,
    worker: WorkerHandle,
    on_edit_tags: Rc<dyn Fn(i64)>,
    /// v0.19.0 — Phase 18.5 Tier-2 Org-link navigation. The
    /// click handler on the notes TextView resolves a clicked
    /// `[[id:UUID][label]]` span to its UUID and invokes this
    /// callback. The window wires it to the existing
    /// `open_inspector_for(task_id)` after a uuid → id lookup;
    /// the callback receives the UUID rather than the resolved
    /// id so the inspector pane stays read-pool-agnostic.
    on_navigate_uuid: Rc<dyn Fn(String)>,
    /// v0.19.0 — Phase 18.5 Tier-2 Link… picker source. Lazily
    /// resolves the read pool when the picker popover opens.
    /// Returning `None` disables the picker.
    pool_source: Rc<dyn Fn() -> Option<ReadPool>>,
}

impl InspectorPane {
    /// Build the pane and mount it into `host` (the `AdwBin` declared
    /// in window.ui). `on_edit_tags` is invoked when the user hits
    /// the "Edit Tags…" button — same hand-off as the dialog
    /// Inspector.
    pub fn install<F, N, P>(
        host: &adw::Bin,
        worker: WorkerHandle,
        on_edit_tags: F,
        on_navigate_uuid: N,
        pool_source: P,
    ) -> Rc<Self>
    where
        F: Fn(i64) + 'static,
        N: Fn(String) + 'static,
        P: Fn() -> Option<ReadPool> + 'static,
    {
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();

        // v0.7.5 — empty state pared down. The previous
        // AdwStatusPage with a giant edit-symbolic icon dominated
        // the pane during navigation (the inspector is empty more
        // often than full). A small centred caption near the top
        // of the pane reads as "ready and waiting" without
        // claiming visual weight. The atmospheric tint of the
        // pane itself signals that this is the inspector's home.
        let empty_label = gtk::Label::builder()
            .label("Select a task to edit it here.")
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Start)
            .margin_top(28)
            .margin_start(24)
            .margin_end(24)
            .wrap(true)
            .justify(gtk::Justification::Center)
            .build();
        empty_label.add_css_class("dim-label");
        empty_label.add_css_class("caption");

        let editor_host = adw::Bin::new();

        stack.add_named(&empty_label, Some("empty"));
        stack.add_named(&editor_host, Some("editor"));
        stack.set_visible_child_name("empty");

        host.set_child(Some(&stack));

        Rc::new(Self {
            stack,
            editor_host,
            current_task_id: RefCell::new(None),
            current_title_row: RefCell::new(None),
            worker,
            on_edit_tags: Rc::new(on_edit_tags),
            on_navigate_uuid: Rc::new(on_navigate_uuid),
            pool_source: Rc::new(pool_source),
        })
    }

    /// Show the per-task editor for `task`. `projects` populates the
    /// project dropdown; `tag_count` populates the Tags row subtitle;
    /// `clock_entries` (v0.17.0) populates the Time group's
    /// running-state, total, and per-session log. Always rebuilds
    /// the body — recycled forms across task switches are cheap
    /// and avoid stale-closure bugs.
    pub fn set_task(
        &self,
        task: Task,
        projects: Vec<Project>,
        tag_count: usize,
        clock_entries: Vec<TaskClockEntry>,
    ) {
        *self.current_task_id.borrow_mut() = Some(task.id);
        let edit_tags = self.on_edit_tags.clone();
        let navigate = self.on_navigate_uuid.clone();
        let pool_source = self.pool_source.clone();
        let (body, title_row) = build_editor(
            self.worker.clone(),
            task,
            projects,
            tag_count,
            clock_entries,
            move |id| edit_tags(id),
            move |uuid| navigate(uuid),
            move || pool_source(),
        );
        self.editor_host.set_child(Some(&body));
        *self.current_title_row.borrow_mut() = Some(title_row);
        self.stack.set_visible_child_name("editor");
    }

    /// Drop back to the empty-state placeholder.
    pub fn clear(&self) {
        *self.current_task_id.borrow_mut() = None;
        *self.current_title_row.borrow_mut() = None;
        self.editor_host.set_child(None::<&gtk::Widget>);
        self.stack.set_visible_child_name("empty");
    }

    /// Currently-displayed task id, if any.
    pub fn current_task_id(&self) -> Option<i64> {
        *self.current_task_id.borrow()
    }

    /// Hand keyboard focus to the editor's title row and select all
    /// the existing text. Routed to from `Ctrl+I`, double-click, and
    /// right-click → Edit Details… in Builder Mode — the Simple-Mode
    /// modal Inspector's analogue is the `title_row.grab_focus()`
    /// call at the bottom of `inspector::open`. No-ops when no
    /// task is currently displayed (e.g., empty state).
    pub fn focus_title(&self) {
        if let Some(row) = self.current_title_row.borrow().as_ref() {
            row.grab_focus();
            // EntryRow exposes the inner editable through
            // `delegate()`; selecting all on it puts the cursor in
            // a state where typing replaces the title outright,
            // matching the modal Inspector's grab_focus + select-all
            // shape.
            if let Some(delegate) = row.delegate() {
                delegate.select_region(0, -1);
            }
        }
    }
}

/// Build the per-task editor body. Auto-saves each field on
/// focus-out / Enter. Mirrors the Phase 7i dialog form's groups but
/// ditches the Cancel/Apply footer in favor of live commits — the
/// pane is non-modal, so there's nothing to dismiss.
///
/// Returns `(body, title_row)` so the caller can stash the title
/// row for `InspectorPane::focus_title()` (`Ctrl+I` and the
/// double-click / right-click activate paths in Builder Mode).
// 8 parameters is past clippy's default threshold but each
// one is genuinely independent state the inspector body needs;
// bundling them into a struct trades a clippy warning for an
// indirection that doesn't help readers.
#[allow(clippy::too_many_arguments)]
fn build_editor<F, N, P>(
    worker: WorkerHandle,
    task: Task,
    projects: Vec<Project>,
    tag_count: usize,
    clock_entries: Vec<TaskClockEntry>,
    on_edit_tags: F,
    on_navigate_uuid: N,
    pool_source: P,
) -> (gtk::Widget, adw::EntryRow)
where
    F: Fn(i64) + 'static,
    N: Fn(String) + 'static,
    P: Fn() -> Option<ReadPool> + 'static,
{
    let task_id = task.id;
    let on_edit_tags = Rc::new(on_edit_tags);
    let on_navigate_uuid = Rc::new(on_navigate_uuid);
    let pool_source = Rc::new(pool_source);

    // ── Title ────────────────────────────────────────────────────
    let title_row = adw::EntryRow::builder()
        .title("Title")
        .text(&task.title)
        .build();

    // v0.7.3 — completion checkbox as the row's leading prefix.
    // Mirror of the row-checkbox in the task list (same .selection-
    // mode class for the circular look, same toggle path through
    // the worker). Brandon's call after spotting the gap: "the
    // inspector doesn't have a way to check off the task." A user
    // viewing a task in the inspector can now mark it done in
    // place without bouncing back to the row.
    let complete_check = gtk::CheckButton::builder()
        .css_classes(["selection-mode"])
        .tooltip_text("Toggle complete")
        .valign(gtk::Align::Center)
        .active(task.completed_at.is_some())
        .build();
    complete_check.update_property(&[gtk::accessible::Property::Label("Task complete")]);
    {
        let worker = worker.clone();
        // `toggled` fires both for user clicks and for our own
        // `set_active` after the worker round-trips the actual
        // state. Latch on the persisted state so the second call
        // is a no-op; without this, opening an already-completed
        // task would untoggle on first click of any field.
        let persisted = std::cell::Cell::new(task.completed_at.is_some());
        complete_check.connect_toggled(move |btn| {
            if btn.is_active() == persisted.get() {
                return;
            }
            persisted.set(btn.is_active());
            let worker = worker.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker.toggle_complete(task_id).await {
                    error!(?e, task_id, "inspector pane: toggle_complete failed");
                }
            });
        });
    }
    title_row.add_prefix(&complete_check);

    let title_initial = task.title.clone();
    wire_entry_autosave(&title_row, worker.clone(), task_id, move |row, worker| {
        let new = row.text().to_string();
        let trimmed = new.trim().to_string();
        if trimmed.is_empty() {
            // Empty title — bounce back to the previous value.
            row.set_text(&title_initial);
            return;
        }
        if trimmed == title_initial {
            return;
        }
        let value = trimmed.clone();
        let worker = worker.clone();
        glib::MainContext::default().spawn_local(async move {
            if let Err(e) = worker
                .update_task(TaskUpdate::new(task_id).title(value))
                .await
            {
                error!(?e, task_id, "inspector pane: title autosave failed");
            }
        });
    });
    let title_group = adw::PreferencesGroup::new();
    title_group.add(&title_row);

    // v0.16.0 — Phase 18.5 Tier-1 keyword picker. Hidden when
    // the vault has no `[[todo_sequences]]` configured (the
    // canonical TODO/DONE is the binary the title-row checkbox
    // already toggles). When configured, the picker exposes the
    // workflow + done sets so the user can pick NEXT / WAITING /
    // etc. without typing into the title field. Selection writes
    // through to `task.orig_keyword` + `completed_at` together.
    if let Some(sequence) = read_active_sequence()
        && (!sequence.workflow.is_empty() || !sequence.done.is_empty())
    {
        let keyword_row = build_keyword_picker(&sequence, &task, worker.clone(), task_id);
        title_group.add(&keyword_row);
    }

    // ── Schedule + Deadline + Project ────────────────────────────
    let schedule_state: Rc<RefCell<Option<ScheduledFor>>> =
        Rc::new(RefCell::new(task.scheduled_for));
    let original_schedule = task.scheduled_for;

    // v0.19.0 — Phase 18.5 Tier-2 time-of-day on schedule. The
    // entry sits below the schedule picker and is only visible
    // when scheduled_for is a Date (Someday + None can't carry
    // a meaningful time). Entry text is `HH:MM`; commit on
    // focus-leave parses + dispatches the worker update.
    let time_entry = gtk::Entry::builder()
        .placeholder_text("HH:MM")
        .max_length(5)
        .width_chars(6)
        .build();
    if let Some(t) = task.scheduled_time {
        time_entry.set_text(&t.format("%H:%M").to_string());
    }
    let time_row = adw::ActionRow::builder()
        .title("Time")
        .activatable_widget(&time_entry)
        .build();
    time_row.add_suffix(&time_entry);
    let scheduled_is_date = matches!(task.scheduled_for, Some(ScheduledFor::Date(_)));
    time_row.set_visible(scheduled_is_date);

    let original_time = task.scheduled_time;
    {
        let worker = worker.clone();
        let entry = time_entry.clone();
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(move |_| {
            let raw = entry.text().to_string();
            let parsed = parse_time_input(&raw);
            if parsed == original_time {
                return;
            }
            let worker = worker.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker
                    .update_task(TaskUpdate::new(task_id).scheduled_time_value(parsed))
                    .await
                {
                    error!(
                        ?e,
                        task_id, "inspector pane: scheduled time autosave failed"
                    );
                }
            });
        });
        time_entry.add_controller(focus);
    }

    let time_row_for_schedule = time_row.clone();
    let schedule_button = build_schedule_button(&schedule_state, {
        let worker = worker.clone();
        move |new| {
            // Toggle the time row's visibility in lockstep with
            // the schedule. Someday or None can't carry a time.
            let is_date = matches!(new, Some(ScheduledFor::Date(_)));
            time_row_for_schedule.set_visible(is_date);
            if new == original_schedule {
                return;
            }
            let worker = worker.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker
                    .update_task(TaskUpdate::new(task_id).schedule(new))
                    .await
                {
                    error!(?e, task_id, "inspector pane: schedule autosave failed");
                }
            });
        }
    });
    schedule_button.add_css_class("flat");
    let schedule_row = adw::ActionRow::builder()
        .title("Schedule")
        .activatable_widget(&schedule_button)
        .build();
    schedule_row.add_suffix(&schedule_button);

    let deadline_state: Rc<RefCell<Option<NaiveDate>>> = Rc::new(RefCell::new(task.deadline));
    let original_deadline = task.deadline;

    // v0.14.0 — DEADLINE warning window (Phase 18.5 Tier-1). The
    // SpinRow is built up-front so the deadline-button callback can
    // toggle its visibility when the user clears or sets the
    // deadline. 0 in the SpinRow means "use the global default";
    // any positive value sets a per-task override that surfaces
    // the task in Today that many days early.
    let warn_row = adw::SpinRow::with_range(0.0, 60.0, 1.0);
    warn_row.set_title("Heads-up window");
    warn_row.set_subtitle(
        "Days before the deadline this task surfaces in Today. 0 uses the default (7).",
    );
    warn_row.set_value(task.deadline_warn_days.unwrap_or(0) as f64);
    warn_row.set_visible(task.deadline.is_some());
    let original_warn = task.deadline_warn_days;
    {
        let worker = worker.clone();
        warn_row.connect_changed(move |row| {
            let raw = row.value().round() as i64;
            let new = if raw == 0 { None } else { Some(raw) };
            if new == original_warn {
                return;
            }
            let worker = worker.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker
                    .update_task(TaskUpdate::new(task_id).deadline_warn_days_value(new))
                    .await
                {
                    error!(?e, task_id, "inspector pane: deadline-warn autosave failed");
                }
            });
        });
    }

    let deadline_button = build_date_button(&deadline_state, format_deadline_label, {
        let worker = worker.clone();
        let warn_row = warn_row.clone();
        move |new| {
            // Toggle the warning row's visibility in lockstep with
            // the deadline. A task without a deadline can't have a
            // meaningful per-task heads-up window.
            warn_row.set_visible(new.is_some());
            if new == original_deadline {
                return;
            }
            let worker = worker.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker
                    .update_task(TaskUpdate::new(task_id).deadline_value(new))
                    .await
                {
                    error!(?e, task_id, "inspector pane: deadline autosave failed");
                }
            });
        }
    });
    deadline_button.add_css_class("flat");
    let deadline_row = adw::ActionRow::builder()
        .title("Deadline")
        .activatable_widget(&deadline_button)
        .build();
    deadline_row.add_suffix(&deadline_button);

    let project_row = build_project_combo_row(&projects, task.project_id);
    let original_project = task.project_id;
    {
        let projects_for_handler = projects.clone();
        let worker = worker.clone();
        project_row.connect_selected_notify(move |row| {
            let new_project = project_id_from_combo_row(row, &projects_for_handler);
            if new_project == original_project {
                return;
            }
            let worker = worker.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker
                    .update_task(TaskUpdate::new(task_id).project(new_project))
                    .await
                {
                    error!(?e, task_id, "inspector pane: project autosave failed");
                }
            });
        });
    }

    // v0.7.0 — inspector clustering pass. Was: Schedule + Deadline +
    // Project in dates_group, Tags alone in its own one-row group
    // (an orphan card the eye couldn't justify). Now: dates_group
    // carries only the date fields, and Project + Tags collapse
    // into a new "Classify" cluster — both fields answer the
    // question "where does this task live?" so the eye groups them
    // naturally.
    // v0.20.0 — Phase 19.5 reminder picker. Independent of
    // scheduled_for / deadline (a reminder fires on a task
    // regardless of those). EntryRow accepts `YYYY-MM-DD HH:MM`
    // text; commits on focus-leave. Empty clears.
    let reminder_row = adw::EntryRow::builder().title("Reminder").build();
    if let Some(when) = task.reminder_at {
        let local = when.with_timezone(&chrono::Local);
        reminder_row.set_text(&local.format("%Y-%m-%d %H:%M").to_string());
    }
    let original_reminder = task.reminder_at;
    {
        let worker = worker.clone();
        let entry = reminder_row.clone();
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(move |_| {
            let raw = entry.text().to_string();
            let parsed = parse_reminder_input(&raw);
            if parsed == original_reminder {
                return;
            }
            let worker = worker.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker
                    .update_task(TaskUpdate::new(task_id).reminder_at_value(parsed))
                    .await
                {
                    error!(?e, task_id, "inspector pane: reminder autosave failed");
                }
            });
        });
        reminder_row.add_controller(focus);
    }

    let dates_group = adw::PreferencesGroup::new();
    dates_group.add(&schedule_row);
    dates_group.add(&time_row);
    dates_group.add(&deadline_row);
    dates_group.add(&warn_row);
    dates_group.add(&reminder_row);

    // ── Classify cluster: Project + Tags ─────────────────────────
    let tag_count_text = format_tag_count(tag_count);
    let edit_tags_button = gtk::Button::builder()
        .label("Edit Tags…")
        .css_classes(["flat"])
        .valign(gtk::Align::Center)
        .build();
    let tags_row = adw::ActionRow::builder()
        .title("Tags")
        .subtitle(&tag_count_text)
        .activatable_widget(&edit_tags_button)
        .build();
    tags_row.add_suffix(&edit_tags_button);
    edit_tags_button.connect_clicked({
        let on_edit_tags = on_edit_tags.clone();
        move |_| {
            on_edit_tags(task_id);
        }
    });
    let classify_group = adw::PreferencesGroup::new();
    classify_group.add(&project_row);
    classify_group.add(&tags_row);

    // ── Notes ────────────────────────────────────────────────────
    let notes_buffer = gtk::TextBuffer::builder().text(&task.note).build();
    let notes_view = gtk::TextView::builder()
        .buffer(&notes_buffer)
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(10)
        .bottom_margin(10)
        .left_margin(10)
        .right_margin(10)
        .build();
    notes_view.add_css_class("atrium-note-body");

    // v0.6.13 (Patch C) — placeholder text for the Notes TextView.
    // GtkTextView doesn't have a native placeholder property, so we
    // overlay a Label that's visible only when the buffer is empty.
    // `set_can_target(false)` keeps the label transparent to clicks
    // — the underlying TextView still gets focus when the user
    // clicks anywhere on the surface.
    let notes_placeholder = gtk::Label::builder()
        .label("What / why / next step — autosaves on focus-out")
        .halign(gtk::Align::Start)
        .valign(gtk::Align::Start)
        .margin_top(14)
        .margin_start(14)
        .build();
    notes_placeholder.add_css_class("dim-label");
    notes_placeholder.set_can_target(false);
    notes_placeholder.set_visible(task.note.is_empty());

    let notes_overlay = gtk::Overlay::builder().child(&notes_view).build();
    notes_overlay.add_overlay(&notes_placeholder);

    // Hide the placeholder the moment the buffer has any text.
    let placeholder_for_changed = notes_placeholder.clone();
    notes_buffer.connect_changed(move |b| {
        placeholder_for_changed.set_visible(b.char_count() == 0);
    });

    let notes_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&notes_overlay)
        .min_content_height(160)
        .build();
    notes_scroll.add_css_class("card");
    notes_scroll.add_css_class("view");
    let notes_group = adw::PreferencesGroup::builder().title("Notes").build();
    notes_group.add(&notes_scroll);

    // v0.19.0 — Phase 18.5 Tier-2 Link… picker. Lives as the
    // notes_group's header suffix so it sits next to the "Notes"
    // title without competing with the body. Click opens a
    // popover with a search field + filtered task list; picking
    // a task inserts `[[id:UUID][title]]` at the cursor.
    let link_button = gtk::Button::builder()
        .icon_name("insert-link-symbolic")
        .tooltip_text("Link to another task…")
        .css_classes(["flat"])
        .build();
    let link_popover = build_task_link_popover(&notes_buffer, pool_source.clone(), task_id);
    link_popover.set_parent(&link_button);
    link_button.connect_clicked(move |_| {
        link_popover.popup();
    });
    notes_group.set_header_suffix(Some(&link_button));

    let notes_initial = task.note.clone();
    let notes_focus = gtk::EventControllerFocus::new();
    notes_focus.connect_leave({
        let buffer = notes_buffer.clone();
        let worker = worker.clone();
        let initial = notes_initial.clone();
        move |_| {
            let new = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .to_string();
            if new == initial {
                return;
            }
            let value = new.clone();
            let worker = worker.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker
                    .update_task(TaskUpdate::new(task_id).note(value))
                    .await
                {
                    error!(?e, task_id, "inspector pane: note autosave failed");
                }
            });
        }
    });
    notes_view.add_controller(notes_focus);

    // ── v0.19.0 — Phase 18.5 Tier-2 Org-link rendering. The
    // notes_buffer carries `[[id:UUID][label]]` constructs that
    // we want to render as clickable spans. Strategy:
    //
    // 1. Register a single `link` text tag with a foreground
    //    accent + underline. Apply it to every link range
    //    parsed from the current body text.
    // 2. Re-apply on every buffer change so live edits keep
    //    links highlighted (cheap — body parsing is linear and
    //    typical notes are short).
    // 3. A click gesture on the textview walks to the iter at
    //    the click position and looks up the buffer's
    //    char-offset against the parsed link ranges. If a
    //    match exists, invoke `on_navigate_uuid` with the
    //    target UUID.
    let link_tag = notes_buffer
        .create_tag(Some("link"), &[("underline", &pango::Underline::Single)])
        .expect("link tag created exactly once per buffer");
    link_tag.set_foreground(Some("@accent_color"));

    let apply_link_tags = {
        let buffer = notes_buffer.clone();
        let tag = link_tag.clone();
        Rc::new(move || {
            let body = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .to_string();
            // Clear existing link-tag spans before re-applying.
            // Bodies are short; this is fine.
            buffer.remove_tag(&tag, &buffer.start_iter(), &buffer.end_iter());
            for link in parse_body_links(&body) {
                // The `BodyLink.range` is a byte range; convert
                // to char offsets for `iter_at_offset`. Bytes →
                // chars: count chars in the byte slice up to
                // `range.start` and `range.end`. ASCII bodies
                // (the common case) have byte == char, so this
                // is a no-cost walk for them.
                let start_char = body[..link.range.start].chars().count() as i32;
                let end_char = body[..link.range.end].chars().count() as i32;
                let start_iter = buffer.iter_at_offset(start_char);
                let end_iter = buffer.iter_at_offset(end_char);
                buffer.apply_tag(&tag, &start_iter, &end_iter);
            }
        })
    };

    // Initial application + re-apply on every buffer change.
    apply_link_tags();
    {
        let apply = apply_link_tags.clone();
        notes_buffer.connect_changed(move |_| apply());
    }

    // Click gesture on the textview. `gtk::GestureClick` fires
    // for single-click + double-click; we trigger on single
    // primary release at the link iter.
    let click_gesture = gtk::GestureClick::builder().button(1).build();
    let view_for_click = notes_view.clone();
    let buffer_for_click = notes_buffer.clone();
    let navigate_for_click = on_navigate_uuid.clone();
    click_gesture.connect_released(move |_, _, x, y| {
        // Convert widget coords → buffer coords → iter.
        let (bx, by) =
            view_for_click.window_to_buffer_coords(gtk::TextWindowType::Widget, x as i32, y as i32);
        let Some(iter) = view_for_click.iter_at_location(bx, by) else {
            return;
        };
        // Resolve the link by walking the parsed list against
        // the click's char-offset. Re-parses on every click —
        // cheap, and avoids cache-invalidation bugs.
        let body = buffer_for_click
            .text(
                &buffer_for_click.start_iter(),
                &buffer_for_click.end_iter(),
                false,
            )
            .to_string();
        let click_char = iter.offset() as usize;
        for link in parse_body_links(&body) {
            // BodyLink.range is byte-indexed; the iter offset
            // is char-indexed. Convert and compare.
            let start_char = body[..link.range.start].chars().count();
            let end_char = body[..link.range.end].chars().count();
            if click_char >= start_char && click_char < end_char {
                navigate_for_click(link.target_uuid);
                return;
            }
        }
    });
    notes_view.add_controller(click_gesture);

    // ── v0.15.0 — Body checkboxes (Phase 18.5 Tier-2) ────────────
    // Subtasks group lives above the Notes textview and reflects
    // any `- [ ]` / `- [X]` / `- [-]` lines as interactive
    // CheckButtons. Clicking a checkbox toggles the line in the
    // buffer (which triggers our notes_buffer change handler) and
    // dispatches the worker update directly so the change isn't
    // gated on focus-out — the click *is* the commit. The
    // notes_view stays the source of truth; this section is a
    // projection rendered on every buffer edit.
    let subtasks_group = adw::PreferencesGroup::builder().title("Subtasks").build();
    let subtasks_list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    subtasks_list.add_css_class("boxed-list");
    subtasks_group.add(&subtasks_list);

    let rebuild_subtasks = std::rc::Rc::new({
        let buffer = notes_buffer.clone();
        let list = subtasks_list.clone();
        let group = subtasks_group.clone();
        let worker = worker.clone();
        move || {
            // Drain existing children.
            while let Some(child) = list.first_child() {
                list.remove(&child);
            }
            let body = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), false)
                .to_string();
            let checkboxes = parse_body_checkboxes(&body);
            if checkboxes.is_empty() {
                group.set_visible(false);
                return;
            }
            group.set_visible(true);
            for cb in checkboxes {
                let row = adw::ActionRow::builder().title(&cb.label).build();
                let check = gtk::CheckButton::builder()
                    .active(cb.state.is_done())
                    .valign(gtk::Align::Center)
                    .build();
                check.set_inconsistent(matches!(
                    cb.state,
                    atrium_core::CheckboxState::Indeterminate
                ));
                let line_index = cb.line_index;
                let buffer_for_click = buffer.clone();
                let worker_for_click = worker.clone();
                check.connect_toggled(move |_| {
                    let current = buffer_for_click
                        .text(
                            &buffer_for_click.start_iter(),
                            &buffer_for_click.end_iter(),
                            false,
                        )
                        .to_string();
                    let updated = toggle_body_checkbox(&current, line_index);
                    if updated == current {
                        return;
                    }
                    // Replace the buffer text in one shot so the
                    // user's cursor doesn't drift mid-edit. Setting
                    // .text fires the buffer's `changed` signal,
                    // which triggers the rebuild closure below.
                    buffer_for_click.set_text(&updated);
                    let value = updated;
                    let worker = worker_for_click.clone();
                    glib::MainContext::default().spawn_local(async move {
                        if let Err(e) = worker
                            .update_task(TaskUpdate::new(task_id).note(value))
                            .await
                        {
                            error!(?e, task_id, "inspector pane: checkbox toggle failed");
                        }
                    });
                });
                row.add_prefix(&check);
                row.set_activatable_widget(Some(&check));
                list.append(&row);
            }
        }
    });

    // Initial population.
    rebuild_subtasks();

    // Rebuild on every buffer change (text edits, toggles, paste,
    // …). This is what keeps the checklist in lockstep with the
    // raw body text the user sees in the textview.
    let rebuild_for_changed = rebuild_subtasks.clone();
    notes_buffer.connect_changed(move |_| {
        rebuild_for_changed();
    });

    // ── Builder-only fields ──────────────────────────────────────
    // The pane only renders in Builder Mode, so an "exposed only in
    // Builder" subtitle reads as redundant noise. v0.6.11 dropped
    // the subtitle entirely and renamed the section to a verb
    // phrase that describes what the fields do.
    let builder_group = adw::PreferencesGroup::builder()
        .title("Schedule depth")
        .build();

    // estimated_minutes — Phase 11 wires the dispatch. SpinRow
    // commits on value-changed via `worker.update_task(
    // TaskUpdate::estimated_minutes_value(_))`. 0 clears the field.
    let est_row = adw::SpinRow::with_range(0.0, 24.0 * 60.0, 5.0);
    est_row.set_title("Estimated minutes");
    est_row.set_subtitle("0 leaves the field unset.");
    est_row.set_value(task.estimated_minutes.unwrap_or(0) as f64);
    let original_estimated = task.estimated_minutes;
    {
        let worker = worker.clone();
        est_row.connect_changed(move |row| {
            let raw = row.value().round() as i64;
            let new = if raw == 0 { None } else { Some(raw) };
            if new == original_estimated {
                return;
            }
            let worker = worker.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker
                    .update_task(TaskUpdate::new(task_id).estimated_minutes_value(new))
                    .await
                {
                    error!(?e, task_id, "inspector pane: estimated autosave failed");
                }
            });
        });
    }
    builder_group.add(&est_row);

    // Phase 11 — defer_until is a functional date popover that
    // mirrors the Schedule / Deadline pickers. A future date
    // excludes the task from Today and Anytime per spec §4.2.
    let defer_state: Rc<RefCell<Option<NaiveDate>>> = Rc::new(RefCell::new(task.defer_until));
    let original_defer = task.defer_until;
    let defer_button = build_date_button(&defer_state, format_defer_label, {
        let worker = worker.clone();
        move |new| {
            if new == original_defer {
                return;
            }
            let worker = worker.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker
                    .update_task(TaskUpdate::new(task_id).defer_value(new))
                    .await
                {
                    error!(?e, task_id, "inspector pane: defer autosave failed");
                }
            });
        }
    });
    defer_button.add_css_class("flat");
    let defer_row = adw::ActionRow::builder()
        .title("Defer until")
        .activatable_widget(&defer_button)
        .build();
    defer_row.add_suffix(&defer_button);
    builder_group.add(&defer_row);

    // Phase 15 — repeat rule editor. Three rows working together:
    //
    //   1. Frequency dropdown (None / Daily / Weekly / Monthly /
    //      Yearly / Custom). "None" clears the rule.
    //   2. Interval spin ("Every N"). Hidden for None / Custom.
    //   3. Mode dropdown (After completion: Cumulative / Next /
    //      Basic). Hidden for None.
    //   4. Custom RRULE entry. Shown only for Custom; the user
    //      types raw RFC 5545 text. Validation happens at the
    //      worker; bad rules surface a toast.
    install_repeat_editor(&builder_group, &worker, &task);

    // ── Page container ───────────────────────────────────────────
    // v0.3.0 — `atrium-inspector-pane` styling lifts the surface
    // visually so the side pane reads as a sheet rather than a
    // continuation of the main list. Padding + a subtle left
    // border distinguishes it; the page itself stays the standard
    // AdwPreferencesPage so library theming flows through.
    // ── v0.17.0 — Phase 18.5 Tier-1 CLOCK time tracking. The
    // Time group sits between Notes and Builder fields: actual
    // time spent (clock entries) reads naturally next to
    // estimated_minutes (intent) on the Builder side. Hidden
    // for tasks with no entries yet, *unless* the user can
    // start one — which is always — so we render the Start
    // button regardless and lazily reveal the log + total when
    // entries exist.
    let time_group = build_time_group(&worker, task_id, &clock_entries);

    let page = adw::PreferencesPage::new();
    page.add_css_class("atrium-inspector-pane");
    page.add(&title_group);
    page.add(&dates_group);
    page.add(&classify_group);
    page.add(&subtasks_group);
    page.add(&notes_group);
    page.add(&time_group);
    page.add(&builder_group);

    (page.upcast(), title_row)
}

/// v0.16.0 — Phase 18.5 Tier-1. Read the vault's first
/// configured TODO sequence (if any). Returns `None` when no
/// vault is configured, the sidecar is missing / malformed, or
/// no `[[todo_sequences]]` block is present. Cheap call (one
/// GSettings read + one small file read); safe to invoke on
/// every Inspector rebuild.
fn read_active_sequence() -> Option<atrium_org::sidecar::TodoSequenceEntry> {
    let settings = gio::Settings::new(atrium_core::APP_ID);
    let raw: String = settings.string("vault-path").into();
    let path = raw.trim();
    if path.is_empty() {
        return None;
    }
    let root = std::path::PathBuf::from(path);
    let sidecar = atrium_org::sidecar::read_sidecar(&root).ok()?;
    sidecar.todo_sequences.into_iter().next()
}

/// v0.16.0 — build the keyword-picker row. ComboRow lists
/// workflow keywords first, then done keywords, in user-defined
/// order. Selection writes through to `task.orig_keyword` (the
/// canonical round-trip column for non-canonical keywords) +
/// `completed_at` (set to `now()` when the user picks a done
/// keyword on an open task; cleared when picking a workflow
/// keyword on a done task). Builder-only — Simple Mode keeps
/// the title-row checkbox as the binary toggle.
fn build_keyword_picker(
    sequence: &atrium_org::sidecar::TodoSequenceEntry,
    task: &Task,
    worker: WorkerHandle,
    task_id: i64,
) -> adw::ComboRow {
    // Build the choice list. Two halves separated by a dash so
    // the user can tell open keywords from done at a glance.
    let mut choices: Vec<String> = Vec::new();
    choices.extend(sequence.workflow.iter().cloned());
    choices.extend(sequence.done.iter().cloned());
    let str_refs: Vec<&str> = choices.iter().map(String::as_str).collect();
    let model = gtk::StringList::new(&str_refs);

    // Resolve the task's current keyword. Priority order:
    //   1. orig_keyword (carries non-canonical labels verbatim)
    //   2. canonical from completed_at (DONE / TODO)
    let current_keyword = task.orig_keyword.clone().unwrap_or_else(|| {
        if task.completed_at.is_some() {
            "DONE".to_string()
        } else {
            "TODO".to_string()
        }
    });
    let initial_index = choices
        .iter()
        .position(|c| c == &current_keyword)
        .unwrap_or(0) as u32;

    let row = adw::ComboRow::builder()
        .title("Keyword")
        .subtitle("From the vault's configured TODO sequence")
        .model(&model)
        .selected(initial_index)
        .build();

    let workflow_set: std::collections::HashSet<String> =
        sequence.workflow.iter().cloned().collect();
    let original_keyword = current_keyword;
    let initial_completed = task.completed_at;
    row.connect_selected_notify(move |row| {
        let idx = row.selected() as usize;
        let Some(picked) = choices.get(idx).cloned() else {
            return;
        };
        if picked == original_keyword {
            return;
        }
        let is_workflow = workflow_set.contains(&picked);
        // The orig_keyword column carries the literal label.
        // Canonical TODO/DONE map to None (column default); any
        // other keyword stashes verbatim. Matches the watcher's
        // org_keyword_to_orig logic.
        let new_orig = match picked.as_str() {
            "TODO" | "DONE" => None,
            other => Some(other.to_string()),
        };
        let new_completed = if is_workflow {
            None
        } else {
            // Done state. If the task was already done preserve
            // the existing timestamp; otherwise stamp now().
            initial_completed.or_else(|| Some(chrono::Utc::now()))
        };
        let worker = worker.clone();
        glib::MainContext::default().spawn_local(async move {
            let mut update = TaskUpdate::new(task_id).orig_keyword(new_orig);
            update = update.completed_at(new_completed);
            if let Err(e) = worker.update_task(update).await {
                error!(
                    ?e,
                    task_id, "inspector pane: keyword picker autosave failed"
                );
            }
        });
    });

    row
}

/// Phase 15 — install the repeat-rule editor into a Builder
/// preferences group. Three preset frequencies (Daily / Weekly /
/// Monthly / Yearly) plus a Custom escape hatch for the full RFC
/// 5545 grammar. Autosaves on every interaction; validation
/// failures from the worker land as a tracing::error (the entry
/// is restored to whatever the worker last accepted on the next
/// `set_task` call so the user isn't stranded with bad text).
/// v0.19.0 — Phase 18.5 Tier-2 time-of-day input parser.
/// Accepts `HH:MM` (24-hour) or empty string (clear).
/// Tolerant: leading/trailing whitespace stripped; single-digit
/// hours accepted (`9:00`); minutes must be two digits. Returns
/// `None` for empty input or unparseable text — the worker
/// treats `None` as "clear the column."
fn parse_time_input(raw: &str) -> Option<chrono::NaiveTime> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut parts = trimmed.splitn(2, ':');
    let h: u32 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    chrono::NaiveTime::from_hms_opt(h, m, 0)
}

/// v0.20.0 — Phase 19.5 reminder input parser. Accepts
/// `YYYY-MM-DD HH:MM` (treated as local time, converted to
/// UTC for storage) or empty (clear). Returns `None` for
/// empty / unparseable input — the worker treats `None` as
/// "clear the column."
fn parse_reminder_input(raw: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let naive = chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M").ok()?;
    let local = chrono::Local.from_local_datetime(&naive).single()?;
    Some(local.with_timezone(&chrono::Utc))
}

/// v0.19.0 — Phase 18.5 Tier-2 Link… picker popover. Builds a
/// search-field + scrolled list combo. Each row in the list is
/// an `adw::ActionRow` with the task's title; clicking inserts
/// `[[id:UUID][title]]` into `buffer` at the cursor and dismisses
/// the popover.
///
/// Filter strategy: the popover loads every task once via the
/// pool when it opens (typical DBs have thousands at most; the
/// load is cheap), then filters in-memory by case-insensitive
/// substring against the title as the user types. Avoids the
/// FTS5 expression-grammar complexity for v0.19.0; if real users
/// hit performance ceilings we can swap in `bm25_for_terms` here.
///
/// `current_task_id` is excluded from the result list — linking
/// a task to itself isn't useful.
fn build_task_link_popover(
    buffer: &gtk::TextBuffer,
    pool_source: Rc<dyn Fn() -> Option<ReadPool>>,
    current_task_id: i64,
) -> gtk::Popover {
    let popover = gtk::Popover::builder()
        .position(gtk::PositionType::Bottom)
        .has_arrow(true)
        .build();
    popover.add_css_class("atrium-link-picker");

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .width_request(360)
        .height_request(320)
        .build();

    let search = gtk::SearchEntry::builder()
        .placeholder_text("Search tasks…")
        .build();
    body.append(&search);

    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .build();
    list.add_css_class("boxed-list");
    let list_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&list)
        .build();
    body.append(&list_scroll);

    popover.set_child(Some(&body));

    // Cached task list — populated on every popover-show so a
    // recently-created task surfaces. Held inside an Rc<RefCell>
    // so the search-changed handler can re-filter without
    // re-querying the DB on every keystroke.
    let cached_tasks: Rc<RefCell<Vec<Task>>> = Rc::new(RefCell::new(Vec::new()));
    let pool_source_for_show = pool_source.clone();
    let cached_for_show = cached_tasks.clone();
    let list_for_show = list.clone();
    let buffer_for_show = buffer.clone();
    let popover_for_show = popover.clone();
    popover.connect_show(move |_| {
        let Some(pool) = pool_source_for_show() else {
            // No pool available — render an empty-state row
            // and bail.
            while let Some(child) = list_for_show.first_child() {
                list_for_show.remove(&child);
            }
            let row = adw::ActionRow::builder()
                .title("(database unavailable)")
                .build();
            list_for_show.append(&row);
            return;
        };
        let tasks = pool
            .with(atrium_core::db::read::list_all_tasks)
            .unwrap_or_default()
            .into_iter()
            .filter(|t| t.id != current_task_id)
            .collect::<Vec<_>>();
        *cached_for_show.borrow_mut() = tasks.clone();
        populate_link_picker_rows(&list_for_show, &tasks, &buffer_for_show, &popover_for_show);
    });

    let search_for_changed = search.clone();
    let cached_for_search = cached_tasks.clone();
    let list_for_search = list.clone();
    let buffer_for_search = buffer.clone();
    let popover_for_search = popover.clone();
    search.connect_search_changed(move |_| {
        let needle = search_for_changed.text().to_string().to_ascii_lowercase();
        let cached = cached_for_search.borrow();
        let filtered: Vec<Task> = if needle.is_empty() {
            cached.clone()
        } else {
            cached
                .iter()
                .filter(|t| t.title.to_ascii_lowercase().contains(&needle))
                .cloned()
                .collect()
        };
        populate_link_picker_rows(
            &list_for_search,
            &filtered,
            &buffer_for_search,
            &popover_for_search,
        );
    });

    popover
}

/// Replace the link-picker list's children with one ActionRow
/// per task. Click handler inserts the link at the buffer's
/// cursor and dismisses the popover.
fn populate_link_picker_rows(
    list: &gtk::ListBox,
    tasks: &[Task],
    buffer: &gtk::TextBuffer,
    popover: &gtk::Popover,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    if tasks.is_empty() {
        let row = adw::ActionRow::builder()
            .title("(no matching tasks)")
            .build();
        list.append(&row);
        return;
    }
    for task in tasks.iter().take(50) {
        let row = adw::ActionRow::builder().title(&task.title).build();
        let uuid = task.uuid.clone();
        let title = task.title.clone();
        let buffer = buffer.clone();
        let popover = popover.clone();
        let click = gtk::GestureClick::new();
        click.connect_released(move |_, _, _, _| {
            let link_text = format!("[[id:{uuid}][{title}]]");
            // Insert at the cursor's position.
            let mut iter = buffer.iter_at_mark(&buffer.get_insert());
            buffer.insert(&mut iter, &link_text);
            popover.popdown();
        });
        row.add_controller(click);
        row.set_activatable(true);
        list.append(&row);
    }
    // Cap at 50 rows for the picker — typing a couple of letters
    // narrows things; the full list is rarely useful in a popover.
}

/// v0.17.0 — Phase 18.5 Tier-1 CLOCK time tracking Time group.
/// Renders three things:
///
/// 1. Start/Stop button (label flips based on whether this task
///    has an open clock).
/// 2. "Total" row — sum of closed-entry minutes formatted
///    HH:MM, hidden when zero so an empty group doesn't look
///    accusatory.
/// 3. Per-session log — one ActionRow per closed entry showing
///    the duration + start time. Open entries surface as a
///    "Running since HH:MM" row. Hidden when there are no
///    entries.
///
/// Builder-only (caller controls visibility — Simple Mode
/// dialog doesn't include this group at all). Auto-refreshes
/// because `set_task` re-runs on every TaskChanges that touches
/// this task; clock_in/clock_out emit the right TaskChanges via
/// the worker's `emit_task_refresh` helper.
fn build_time_group(
    worker: &WorkerHandle,
    task_id: i64,
    entries: &[TaskClockEntry],
) -> adw::PreferencesGroup {
    let group = adw::PreferencesGroup::builder().title("Time").build();

    let running = entries.iter().any(|e| e.is_running());
    let action_row = adw::ActionRow::builder()
        .title(if running {
            "Currently running"
        } else {
            "Track time on this task"
        })
        .build();
    let toggle_button = gtk::Button::builder()
        .label(if running { "Stop" } else { "Start" })
        .valign(gtk::Align::Center)
        .build();
    if running {
        toggle_button.add_css_class("destructive-action");
    } else {
        toggle_button.add_css_class("suggested-action");
    }
    {
        let worker = worker.clone();
        toggle_button.connect_clicked(move |_| {
            let worker = worker.clone();
            glib::MainContext::default().spawn_local(async move {
                let result = if running {
                    worker.clock_out(task_id).await.map(|_| ())
                } else {
                    worker.clock_in(task_id, String::new()).await.map(|_| ())
                };
                if let Err(e) = result {
                    error!(?e, task_id, "inspector pane: clock toggle failed");
                }
                // The worker's emit_task_refresh fires a
                // TaskChanges with this task in `updated`, which
                // triggers the window's refresh path → set_task
                // re-runs → this group rebuilds with the new
                // running state. No manual UI poke needed here.
            });
        });
    }
    action_row.add_suffix(&toggle_button);
    group.add(&action_row);

    // Total row + log only when entries exist. A first-time
    // user clocking in should see Stop + nothing else; once
    // they've stopped, the closed entry surfaces in the log
    // and the total appears.
    let total_minutes: i64 = entries
        .iter()
        .filter_map(TaskClockEntry::duration_minutes)
        .sum();
    if total_minutes > 0 {
        let hours = total_minutes / 60;
        let mins = total_minutes % 60;
        let total_row = adw::ActionRow::builder()
            .title("Total")
            .subtitle(format!("{hours}:{mins:02}"))
            .build();
        group.add(&total_row);
    }

    for entry in entries {
        let row = adw::ActionRow::builder().build();
        let started_local = entry.started_at.with_timezone(&chrono::Local);
        let started_label = started_local.format("%a %b %-d, %H:%M").to_string();
        match entry.duration_minutes() {
            Some(d) => {
                let h = d / 60;
                let m = d % 60;
                row.set_title(&format!("{h}:{m:02}"));
                row.set_subtitle(&started_label);
            }
            None => {
                // Open entry — surface "Running since…".
                row.set_title("Running");
                row.set_subtitle(&format!("started {started_label}"));
                row.add_css_class("atrium-clock-running");
            }
        }
        if !entry.note.is_empty() {
            // Append the note in the subtitle so the user can
            // see what the session was for.
            let combined = format!("{} — {}", row.subtitle().unwrap_or_default(), entry.note);
            row.set_subtitle(&combined);
        }
        group.add(&row);
    }

    group
}

fn install_repeat_editor(group: &adw::PreferencesGroup, worker: &WorkerHandle, task: &Task) {
    let task_id = task.id;
    let initial_preset = preset_from_rule(task.repeat_rule.as_deref());
    let initial_interval = interval_from_rule(task.repeat_rule.as_deref()).unwrap_or(1);
    let initial_mode = RepeatMode::from_column(task.repeat_mode.as_deref());
    let initial_custom = if matches!(initial_preset, RepeatPreset::Custom) {
        task.repeat_rule.clone().unwrap_or_default()
    } else {
        String::new()
    };

    // Frequency dropdown. "None" lives at index 0 so a brand-new
    // task without a repeat lands there by default.
    let freq_model =
        gtk::StringList::new(&["None", "Daily", "Weekly", "Monthly", "Yearly", "Custom"]);
    let freq_row = adw::ComboRow::builder()
        .title("Repeat")
        .model(&freq_model)
        .selected(preset_index(initial_preset))
        .build();

    let interval_row = adw::SpinRow::with_range(1.0, 365.0, 1.0);
    interval_row.set_title("Every");
    interval_row.set_subtitle("Number of frequency units between occurrences.");
    interval_row.set_value(initial_interval as f64);

    let mode_model = gtk::StringList::new(&[
        "After completion (Cumulative)",
        "From completion date (Next)",
        "Always shift by interval (Basic)",
    ]);
    let mode_row = adw::ComboRow::builder()
        .title("After completion")
        .model(&mode_model)
        .selected(mode_index(initial_mode))
        .build();

    let custom_row = adw::EntryRow::builder()
        .title("Custom RRULE")
        .text(&initial_custom)
        .build();

    let visible = matches!(initial_preset, RepeatPreset::None);
    interval_row.set_visible(!visible);
    mode_row.set_visible(!visible);
    custom_row.set_visible(matches!(initial_preset, RepeatPreset::Custom));
    if matches!(initial_preset, RepeatPreset::Custom) {
        interval_row.set_visible(false);
    }

    group.add(&freq_row);
    group.add(&interval_row);
    group.add(&mode_row);
    group.add(&custom_row);

    // Shared commit closure — reads the current state of all three
    // rows, builds the RRULE text, and dispatches an update to the
    // worker. Mode is always sent (even when no rule is set, to
    // clear stale state); rule is sent as Some(text) / None.
    let commit = {
        let worker = worker.clone();
        let freq_row = freq_row.clone();
        let interval_row = interval_row.clone();
        let mode_row = mode_row.clone();
        let custom_row = custom_row.clone();
        Rc::new(move || {
            let preset = preset_from_index(freq_row.selected());
            let interval = interval_row.value().round().max(1.0) as u32;
            let mode = mode_from_index(mode_row.selected());
            let custom_text = custom_row.text().to_string();

            let new_rule = match preset {
                RepeatPreset::None => None,
                RepeatPreset::Daily => Some(rule_from_freq("DAILY", interval)),
                RepeatPreset::Weekly => Some(rule_from_freq("WEEKLY", interval)),
                RepeatPreset::Monthly => Some(rule_from_freq("MONTHLY", interval)),
                RepeatPreset::Yearly => Some(rule_from_freq("YEARLY", interval)),
                RepeatPreset::Custom => {
                    let trimmed = custom_text.trim().to_string();
                    if trimmed.is_empty() {
                        None
                    } else {
                        // Validate locally so we can avoid a
                        // worker round-trip on obvious garbage.
                        if RepeatRule::parse(&trimmed, mode).is_err() {
                            // Don't dispatch; the user will see the
                            // entry sit unstyled until they fix it.
                            return;
                        }
                        Some(trimmed)
                    }
                }
            };

            let new_mode = if new_rule.is_some() {
                Some(mode.as_column().to_string())
            } else {
                None
            };

            let worker = worker.clone();
            glib::MainContext::default().spawn_local(async move {
                if let Err(e) = worker
                    .update_task(
                        TaskUpdate::new(task_id)
                            .repeat_rule_value(new_rule)
                            .repeat_mode_value(new_mode),
                    )
                    .await
                {
                    error!(?e, task_id, "inspector pane: repeat autosave failed");
                }
            });
        })
    };

    // Toggle row visibility when the preset changes.
    {
        let interval_row = interval_row.clone();
        let mode_row = mode_row.clone();
        let custom_row = custom_row.clone();
        let commit = commit.clone();
        freq_row.connect_selected_notify(move |row| {
            let preset = preset_from_index(row.selected());
            let none = matches!(preset, RepeatPreset::None);
            let custom = matches!(preset, RepeatPreset::Custom);
            interval_row.set_visible(!none && !custom);
            mode_row.set_visible(!none);
            custom_row.set_visible(custom);
            commit();
        });
    }

    {
        let commit = commit.clone();
        interval_row.connect_changed(move |_| commit());
    }
    {
        let commit = commit.clone();
        mode_row.connect_selected_notify(move |_| commit());
    }
    {
        let commit = commit.clone();
        custom_row.connect_apply(move |_| commit());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RepeatPreset {
    None,
    Daily,
    Weekly,
    Monthly,
    Yearly,
    Custom,
}

fn preset_index(p: RepeatPreset) -> u32 {
    match p {
        RepeatPreset::None => 0,
        RepeatPreset::Daily => 1,
        RepeatPreset::Weekly => 2,
        RepeatPreset::Monthly => 3,
        RepeatPreset::Yearly => 4,
        RepeatPreset::Custom => 5,
    }
}

fn preset_from_index(i: u32) -> RepeatPreset {
    match i {
        1 => RepeatPreset::Daily,
        2 => RepeatPreset::Weekly,
        3 => RepeatPreset::Monthly,
        4 => RepeatPreset::Yearly,
        5 => RepeatPreset::Custom,
        _ => RepeatPreset::None,
    }
}

fn mode_index(m: RepeatMode) -> u32 {
    match m {
        RepeatMode::Cumulative => 0,
        RepeatMode::Next => 1,
        RepeatMode::Basic => 2,
    }
}

fn mode_from_index(i: u32) -> RepeatMode {
    match i {
        1 => RepeatMode::Next,
        2 => RepeatMode::Basic,
        _ => RepeatMode::Cumulative,
    }
}

/// Best-effort recognise the simple-preset shape of a stored rule.
/// `FREQ=DAILY[;INTERVAL=N]` (in either order, possibly with extra
/// whitespace) maps to Daily; anything outside the simple presets
/// (BYDAY, COUNT, UNTIL, etc.) maps to Custom so the user keeps
/// editorial control over the raw RRULE text.
fn preset_from_rule(rule: Option<&str>) -> RepeatPreset {
    let Some(rule) = rule else {
        return RepeatPreset::None;
    };
    let mut freq: Option<&str> = None;
    let mut has_interval = false;
    let mut has_other = false;
    for token in rule.split(';') {
        let trimmed = token.trim();
        let upper = trimmed.to_ascii_uppercase();
        if let Some(rest) = upper.strip_prefix("FREQ=") {
            freq = match rest {
                "DAILY" => Some("DAILY"),
                "WEEKLY" => Some("WEEKLY"),
                "MONTHLY" => Some("MONTHLY"),
                "YEARLY" => Some("YEARLY"),
                _ => return RepeatPreset::Custom,
            };
        } else if upper.starts_with("INTERVAL=") {
            has_interval = true;
        } else if !trimmed.is_empty() {
            has_other = true;
        }
    }
    if has_other {
        return RepeatPreset::Custom;
    }
    let _ = has_interval; // INTERVAL alone keeps the preset simple
    match freq {
        Some("DAILY") => RepeatPreset::Daily,
        Some("WEEKLY") => RepeatPreset::Weekly,
        Some("MONTHLY") => RepeatPreset::Monthly,
        Some("YEARLY") => RepeatPreset::Yearly,
        _ => RepeatPreset::Custom,
    }
}

fn interval_from_rule(rule: Option<&str>) -> Option<u32> {
    let rule = rule?;
    for token in rule.split(';') {
        let trimmed = token.trim();
        if let Some(rest) = trimmed.to_ascii_uppercase().strip_prefix("INTERVAL=") {
            return rest.trim().parse().ok();
        }
    }
    Some(1)
}

fn rule_from_freq(freq: &str, interval: u32) -> String {
    if interval <= 1 {
        format!("FREQ={freq}")
    } else {
        format!("FREQ={freq};INTERVAL={interval}")
    }
}

fn format_tag_count(n: usize) -> String {
    match n {
        0 => "No tags".to_string(),
        1 => "1 tag".to_string(),
        n => format!("{n} tags"),
    }
}

/// Wire an `AdwEntryRow` to autosave on focus-out and on Enter
/// (Adwaita's "apply" signal — the enter-key activation). The
/// closure gets both the row and the worker handle to dispatch
/// updates with.
fn wire_entry_autosave<F>(row: &adw::EntryRow, worker: WorkerHandle, _task_id: i64, save: F)
where
    F: Fn(&adw::EntryRow, &WorkerHandle) + Clone + 'static,
{
    let save_for_apply = save.clone();
    let worker_for_apply = worker.clone();
    row.connect_apply(move |row| {
        save_for_apply(row, &worker_for_apply);
    });
    let save_for_focus = save.clone();
    let focus_ctrl = gtk::EventControllerFocus::new();
    let row_weak = row.downgrade();
    focus_ctrl.connect_leave(move |_| {
        if let Some(row) = row_weak.upgrade() {
            save_for_focus(&row, &worker);
        }
    });
    row.add_controller(focus_ctrl);
}

fn build_schedule_button<F>(
    state: &Rc<RefCell<Option<ScheduledFor>>>,
    on_change: F,
) -> gtk::MenuButton
where
    F: Fn(Option<ScheduledFor>) + Clone + 'static,
{
    let label_widget = gtk::Label::builder()
        .label(format_schedule_label(state.borrow().as_ref()))
        .build();
    let button = gtk::MenuButton::builder().child(&label_widget).build();
    let popover = gtk::Popover::new();
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(12)
        .build();

    let today_button = gtk::Button::builder()
        .label("Today")
        .css_classes(["flat"])
        .build();
    let tomorrow_button = gtk::Button::builder()
        .label("Tomorrow")
        .css_classes(["flat"])
        .build();
    let someday_button = gtk::Button::builder()
        .label("Someday")
        .css_classes(["flat"])
        .build();
    let clear_button = gtk::Button::builder()
        .label("Clear")
        .css_classes(["flat"])
        .build();
    let calendar = gtk::Calendar::new();

    body.append(&today_button);
    body.append(&tomorrow_button);
    body.append(&someday_button);
    body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    body.append(&calendar);
    body.append(&clear_button);
    popover.set_child(Some(&body));
    button.set_popover(Some(&popover));

    let today = chrono::Local::now().date_naive();
    let tomorrow = today + chrono::Duration::days(1);

    let commit = clone!(
        #[strong]
        state,
        #[weak]
        label_widget,
        #[weak]
        popover,
        #[strong]
        on_change,
        move |new: Option<ScheduledFor>| {
            *state.borrow_mut() = new;
            label_widget.set_label(&format_schedule_label(state.borrow().as_ref()));
            popover.popdown();
            on_change(new);
        }
    );

    today_button.connect_clicked({
        let commit = commit.clone();
        move |_| commit(Some(ScheduledFor::Date(today)))
    });
    tomorrow_button.connect_clicked({
        let commit = commit.clone();
        move |_| commit(Some(ScheduledFor::Date(tomorrow)))
    });
    someday_button.connect_clicked({
        let commit = commit.clone();
        move |_| commit(Some(ScheduledFor::Someday))
    });
    clear_button.connect_clicked({
        let commit = commit.clone();
        move |_| commit(None)
    });
    calendar.connect_day_selected({
        let commit = commit.clone();
        move |cal| {
            if let Some(d) = calendar_to_naive_date(cal) {
                commit(Some(ScheduledFor::Date(d)));
            }
        }
    });

    button
}

fn build_date_button<F>(
    state: &Rc<RefCell<Option<NaiveDate>>>,
    formatter: fn(Option<&NaiveDate>) -> String,
    on_change: F,
) -> gtk::MenuButton
where
    F: Fn(Option<NaiveDate>) + Clone + 'static,
{
    let label_widget = gtk::Label::builder()
        .label(formatter(state.borrow().as_ref()))
        .build();
    let button = gtk::MenuButton::builder().child(&label_widget).build();
    let popover = gtk::Popover::new();
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(8)
        .margin_start(12)
        .margin_end(12)
        .margin_top(12)
        .margin_bottom(12)
        .build();

    let today_button = gtk::Button::builder()
        .label("Today")
        .css_classes(["flat"])
        .build();
    let tomorrow_button = gtk::Button::builder()
        .label("Tomorrow")
        .css_classes(["flat"])
        .build();
    let clear_button = gtk::Button::builder()
        .label("Clear")
        .css_classes(["flat"])
        .build();
    let calendar = gtk::Calendar::new();

    body.append(&today_button);
    body.append(&tomorrow_button);
    body.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
    body.append(&calendar);
    body.append(&clear_button);
    popover.set_child(Some(&body));
    button.set_popover(Some(&popover));

    let today = chrono::Local::now().date_naive();
    let tomorrow = today + chrono::Duration::days(1);

    let commit = clone!(
        #[strong]
        state,
        #[weak]
        label_widget,
        #[weak]
        popover,
        #[strong]
        on_change,
        move |new: Option<NaiveDate>| {
            *state.borrow_mut() = new;
            label_widget.set_label(&formatter(state.borrow().as_ref()));
            popover.popdown();
            on_change(new);
        }
    );

    today_button.connect_clicked({
        let commit = commit.clone();
        move |_| commit(Some(today))
    });
    tomorrow_button.connect_clicked({
        let commit = commit.clone();
        move |_| commit(Some(tomorrow))
    });
    clear_button.connect_clicked({
        let commit = commit.clone();
        move |_| commit(None)
    });
    calendar.connect_day_selected({
        let commit = commit.clone();
        move |cal| {
            if let Some(d) = calendar_to_naive_date(cal) {
                commit(Some(d));
            }
        }
    });

    button
}

fn build_project_combo_row(projects: &[Project], current: Option<i64>) -> adw::ComboRow {
    let model = gtk::StringList::new(&["Inbox (no project)"]);
    for p in projects {
        model.append(&p.title);
    }
    let row = adw::ComboRow::builder()
        .title("Project")
        .model(&model)
        .build();
    let pos: u32 = match current {
        None => 0,
        Some(id) => projects
            .iter()
            .position(|p| p.id == id)
            .map_or(0, |i| (i + 1) as u32),
    };
    row.set_selected(pos);
    row
}

fn project_id_from_combo_row(row: &adw::ComboRow, projects: &[Project]) -> Option<i64> {
    let selected = row.selected();
    if selected == 0 {
        return None;
    }
    let idx = (selected as usize).saturating_sub(1);
    projects.get(idx).map(|p| p.id)
}

fn calendar_to_naive_date(cal: &gtk::Calendar) -> Option<NaiveDate> {
    let dt = cal.date();
    NaiveDate::from_ymd_opt(dt.year(), dt.month() as u32, dt.day_of_month() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_count_formatter() {
        assert_eq!(format_tag_count(0), "No tags");
        assert_eq!(format_tag_count(1), "1 tag");
        assert_eq!(format_tag_count(5), "5 tags");
    }

    // Phase 15 — preset / interval round-trip helpers.

    #[test]
    fn preset_recognition() {
        assert_eq!(preset_from_rule(None), RepeatPreset::None);
        assert_eq!(preset_from_rule(Some("FREQ=DAILY")), RepeatPreset::Daily);
        assert_eq!(preset_from_rule(Some("FREQ=WEEKLY")), RepeatPreset::Weekly);
        assert_eq!(
            preset_from_rule(Some("FREQ=MONTHLY")),
            RepeatPreset::Monthly
        );
        assert_eq!(preset_from_rule(Some("FREQ=YEARLY")), RepeatPreset::Yearly);
        // INTERVAL keeps the preset simple.
        assert_eq!(
            preset_from_rule(Some("FREQ=WEEKLY;INTERVAL=2")),
            RepeatPreset::Weekly
        );
        // BYDAY / COUNT / UNTIL fall through to Custom.
        assert_eq!(
            preset_from_rule(Some("FREQ=WEEKLY;BYDAY=MO,WE")),
            RepeatPreset::Custom
        );
        assert_eq!(
            preset_from_rule(Some("FREQ=DAILY;COUNT=5")),
            RepeatPreset::Custom
        );
    }

    #[test]
    fn interval_round_trip() {
        assert_eq!(interval_from_rule(Some("FREQ=DAILY")), Some(1));
        assert_eq!(interval_from_rule(Some("FREQ=WEEKLY;INTERVAL=3")), Some(3));
        assert_eq!(interval_from_rule(None), None);
    }

    #[test]
    fn rule_emit() {
        assert_eq!(rule_from_freq("DAILY", 1), "FREQ=DAILY");
        assert_eq!(rule_from_freq("WEEKLY", 2), "FREQ=WEEKLY;INTERVAL=2");
    }

    #[test]
    fn mode_index_round_trip() {
        for m in [RepeatMode::Cumulative, RepeatMode::Next, RepeatMode::Basic] {
            assert_eq!(mode_from_index(mode_index(m)), m);
        }
    }
}
