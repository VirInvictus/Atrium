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
use atrium_core::{Project, RepeatMode, RepeatRule, ScheduledFor, Task, TaskUpdate, WorkerHandle};
use chrono::NaiveDate;
use gtk::glib;
use gtk::glib::clone;
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
            current_title_row: RefCell::new(None),
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
        let (body, title_row) =
            build_editor(self.worker.clone(), task, projects, tag_count, move |id| {
                edit_tags(id)
            });
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
fn build_editor<F>(
    worker: WorkerHandle,
    task: Task,
    projects: Vec<Project>,
    tag_count: usize,
    on_edit_tags: F,
) -> (gtk::Widget, adw::EntryRow)
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
    let page = adw::PreferencesPage::new();
    page.add_css_class("atrium-inspector-pane");
    page.add(&title_group);
    page.add(&dates_group);
    page.add(&tags_group);
    page.add(&notes_group);
    page.add(&builder_group);

    (page.upcast(), title_row)
}

/// Phase 15 — install the repeat-rule editor into a Builder
/// preferences group. Three preset frequencies (Daily / Weekly /
/// Monthly / Yearly) plus a Custom escape hatch for the full RFC
/// 5545 grammar. Autosaves on every interaction; validation
/// failures from the worker land as a tracing::error (the entry
/// is restored to whatever the worker last accepted on the next
/// `set_task` call so the user isn't stranded with bad text).
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
