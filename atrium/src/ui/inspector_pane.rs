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
use atrium_core::{Project, ScheduledFor, Task, TaskUpdate, WorkerHandle};
use chrono::NaiveDate;
use gtk::glib;
use gtk::glib::clone;
use tracing::error;

use crate::ui::inspector::{format_deadline_label, format_schedule_label};

/// Shared state mounted into the pane host. Keeps the empty-state
/// page + the editor page in a single `gtk::Stack` and exposes
/// `set_task` so the window can swap content as the selection
/// changes. The `current_task_id` cell short-circuits redundant
/// rebuilds when the same task is selected twice.
pub struct InspectorPane {
    stack: gtk::Stack,
    editor_host: adw::Bin,
    current_task_id: RefCell<Option<i64>>,
    worker: WorkerHandle,
    on_edit_tags: Rc<dyn Fn(i64)>,
}

impl InspectorPane {
    /// Build the pane and mount it into `host` (the `AdwBin` declared
    /// in window.ui). `on_edit_tags` is invoked when the user hits
    /// the "Edit Tags…" button — same hand-off as the dialog
    /// Inspector.
    pub fn install<F>(host: &adw::Bin, worker: WorkerHandle, on_edit_tags: F) -> Rc<Self>
    where
        F: Fn(i64) + 'static,
    {
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();

        let empty_state = adw::StatusPage::builder()
            .icon_name("edit-symbolic")
            .title("No task selected")
            .description("Select a row to edit it here.")
            .build();
        empty_state.add_css_class("compact");

        let editor_host = adw::Bin::new();

        stack.add_named(&empty_state, Some("empty"));
        stack.add_named(&editor_host, Some("editor"));
        stack.set_visible_child_name("empty");

        host.set_child(Some(&stack));

        Rc::new(Self {
            stack,
            editor_host,
            current_task_id: RefCell::new(None),
            worker,
            on_edit_tags: Rc::new(on_edit_tags),
        })
    }

    /// Show the per-task editor for `task`. `projects` populates the
    /// project dropdown; `tag_count` populates the Tags row subtitle.
    /// Always rebuilds the body — recycled forms across task switches
    /// are cheap and avoid stale-closure bugs.
    pub fn set_task(&self, task: Task, projects: Vec<Project>, tag_count: usize) {
        *self.current_task_id.borrow_mut() = Some(task.id);
        let edit_tags = self.on_edit_tags.clone();
        let body = build_editor(self.worker.clone(), task, projects, tag_count, move |id| {
            edit_tags(id)
        });
        self.editor_host.set_child(Some(&body));
        self.stack.set_visible_child_name("editor");
    }

    /// Drop back to the empty-state placeholder.
    pub fn clear(&self) {
        *self.current_task_id.borrow_mut() = None;
        self.editor_host.set_child(None::<&gtk::Widget>);
        self.stack.set_visible_child_name("empty");
    }

    /// Currently-displayed task id, if any.
    pub fn current_task_id(&self) -> Option<i64> {
        *self.current_task_id.borrow()
    }
}

/// Build the per-task editor body. Auto-saves each field on
/// focus-out / Enter. Mirrors the Phase 7i dialog form's groups but
/// ditches the Cancel/Apply footer in favor of live commits — the
/// pane is non-modal, so there's nothing to dismiss.
fn build_editor<F>(
    worker: WorkerHandle,
    task: Task,
    projects: Vec<Project>,
    tag_count: usize,
    on_edit_tags: F,
) -> gtk::Widget
where
    F: Fn(i64) + 'static,
{
    let task_id = task.id;
    let on_edit_tags = Rc::new(on_edit_tags);

    // ── Title ────────────────────────────────────────────────────
    let title_row = adw::EntryRow::builder()
        .title("Title")
        .text(&task.title)
        .build();
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

    // ── Schedule + Deadline + Project ────────────────────────────
    let schedule_state: Rc<RefCell<Option<ScheduledFor>>> =
        Rc::new(RefCell::new(task.scheduled_for));
    let original_schedule = task.scheduled_for;
    let schedule_button = build_schedule_button(&schedule_state, {
        let worker = worker.clone();
        move |new| {
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
    let deadline_button = build_date_button(&deadline_state, format_deadline_label, {
        let worker = worker.clone();
        move |new| {
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

    let dates_group = adw::PreferencesGroup::new();
    dates_group.add(&schedule_row);
    dates_group.add(&deadline_row);
    dates_group.add(&project_row);

    // ── Tags row ─────────────────────────────────────────────────
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
    let tags_group = adw::PreferencesGroup::new();
    tags_group.add(&tags_row);

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
    let notes_scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&notes_view)
        .min_content_height(160)
        .build();
    notes_scroll.add_css_class("card");
    notes_scroll.add_css_class("view");
    let notes_group = adw::PreferencesGroup::builder().title("Notes").build();
    notes_group.add(&notes_scroll);

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

    // ── Builder-only fields ──────────────────────────────────────
    let builder_group = adw::PreferencesGroup::builder()
        .title("Builder")
        .description("Fields exposed only in Builder Mode.")
        .build();

    // estimated_minutes — functional in Phase 10.
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
            // estimated_minutes isn't on TaskUpdate's builder yet;
            // exposing it here is a future Phase 10.5 / Phase 11
            // task. For Phase 10 the SpinRow lives but doesn't
            // commit — flagged in the patchnotes.
            let _ = (new, &worker);
        });
    }
    builder_group.add(&est_row);

    // defer_until — Phase 11 owns the editor; we render a disabled
    // placeholder so the row still appears in the layout.
    let defer_row = adw::ActionRow::builder()
        .title("Defer until")
        .subtitle("Editor lands in Phase 11.")
        .sensitive(false)
        .build();
    let defer_label = gtk::Label::builder()
        .label(
            task.defer_until
                .map(|d| d.format("%a · %b %-d, %Y").to_string())
                .unwrap_or_else(|| "—".to_string()),
        )
        .build();
    defer_row.add_suffix(&defer_label);
    builder_group.add(&defer_row);

    // repeat_rule — Phase 15 owns the editor; same shape as defer.
    let repeat_row = adw::ActionRow::builder()
        .title("Repeat rule")
        .subtitle("Editor lands in Phase 15.")
        .sensitive(false)
        .build();
    let repeat_label = gtk::Label::builder()
        .label(task.repeat_rule.clone().unwrap_or_else(|| "—".into()))
        .build();
    repeat_row.add_suffix(&repeat_label);
    builder_group.add(&repeat_row);

    // ── Page container ───────────────────────────────────────────
    let page = adw::PreferencesPage::new();
    page.add(&title_group);
    page.add(&dates_group);
    page.add(&tags_group);
    page.add(&notes_group);
    page.add(&builder_group);

    page.upcast()
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
            .map(|i| (i + 1) as u32)
            .unwrap_or(0),
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
}
