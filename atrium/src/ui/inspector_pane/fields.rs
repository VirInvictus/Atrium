// SPDX-License-Identifier: MIT
//! Inspector pane field/widget builders, repeat-rule editor, and small
//! parsers. Extracted from inspector_pane/mod.rs in v0.22.0 split.

use super::*;

use crate::i18n::{gettext, gettext_f, ngettext_f, pgettext};

/// v0.16.0 — Phase 18.5 Tier-1. Read the vault's first
/// configured TODO sequence (if any). Returns `None` when no
/// vault is configured, the sidecar is missing / malformed, or
/// no `[[todo_sequences]]` block is present. Cheap call (one
/// GSettings read + one small file read); safe to invoke on
/// every Inspector rebuild.
pub(super) fn read_active_sequence() -> Option<atrium_org::sidecar::TodoSequenceEntry> {
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
pub(super) fn build_keyword_picker(
    sequence: &atrium_org::sidecar::TodoSequenceEntry,
    task: &Task,
    worker: WorkerHandle,
    task_id: i64,
) -> gtk::ListBoxRow {
    // Build the choice list. Two halves separated by a dash so
    // the user can tell open keywords from done at a glance.
    let mut choices: Vec<String> = Vec::new();
    choices.extend(sequence.workflow.iter().cloned());
    choices.extend(sequence.done.iter().cloned());

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

    let (row, dropdown) = crate::ui::rows::combo_row(
        &gettext("Keyword"),
        // Translators: "TODO" is an Org-mode keyword; keep it verbatim.
        Some(&gettext("From the vault's configured TODO sequence")),
        &choices.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    dropdown.set_selected(initial_index);

    let workflow_set: std::collections::HashSet<String> =
        sequence.workflow.iter().cloned().collect();
    let original_keyword = current_keyword;
    let initial_completed = task.completed_at;
    dropdown.connect_selected_notify(move |dropdown| {
        let idx = dropdown.selected() as usize;
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
pub(super) fn parse_time_input(raw: &str) -> Option<chrono::NaiveTime> {
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
pub(super) fn parse_reminder_input(raw: &str) -> Option<chrono::DateTime<chrono::Utc>> {
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
/// a flat button with the task's title; clicking inserts
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
pub(super) fn build_task_link_popover(
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
        .placeholder_text(gettext("Search tasks…"))
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
            list_for_show.append(&picker_message_row(&gettext("(database unavailable)")));
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
pub(super) fn populate_link_picker_rows(
    list: &gtk::ListBox,
    tasks: &[Task],
    buffer: &gtk::TextBuffer,
    popover: &gtk::Popover,
) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    if tasks.is_empty() {
        list.append(&picker_message_row(&gettext("(no matching tasks)")));
        return;
    }
    for task in tasks.iter().take(50) {
        // A flat button (not an owned row) so a task link can be inserted by
        // keyboard — Enter/Space on the focused button — as well as by mouse.
        // The button lives in the gtk::ListBox, left-aligned to read as a row.
        let button = gtk::Button::builder()
            .label(&task.title)
            .css_classes(["flat"])
            .build();
        if let Some(lbl) = button.child().and_downcast::<gtk::Label>() {
            lbl.set_xalign(0.0);
            lbl.set_ellipsize(pango::EllipsizeMode::End);
        }
        let uuid = task.uuid.clone();
        let title = task.title.clone();
        let buffer = buffer.clone();
        let popover = popover.clone();
        button.connect_clicked(move |_| {
            let link_text = format!("[[id:{uuid}][{title}]]");
            // Insert at the cursor's position.
            let mut iter = buffer.iter_at_mark(&buffer.get_insert());
            buffer.insert(&mut iter, &link_text);
            popover.popdown();
        });
        list.append(&button);
    }
    // Cap at 50 rows for the picker — typing a couple of letters
    // narrows things; the full list is rarely useful in a popover.
}

/// A non-interactive message row for the link picker's empty states.
fn picker_message_row(msg: &str) -> gtk::Label {
    let label = gtk::Label::builder()
        .label(msg)
        .xalign(0.0)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(12)
        .margin_end(12)
        .build();
    label.add_css_class("dim-label");
    label
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
pub(super) fn build_time_group(
    worker: &WorkerHandle,
    task_id: i64,
    entries: &[TaskClockEntry],
) -> crate::ui::rows::Group {
    let group = crate::ui::rows::group(Some(&gettext("Time")), None);

    let running = entries.iter().any(|e| e.is_running());
    let toggle_button = gtk::Button::builder()
        .label(if running {
            gettext("Stop")
        } else {
            gettext("Start")
        })
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
    let action_title = if running {
        gettext("Currently running")
    } else {
        gettext("Track time on this task")
    };
    group.add(&crate::ui::rows::row(
        &action_title,
        None,
        Some(toggle_button.upcast_ref()),
    ));

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
        group.add(&crate::ui::rows::row(
            &gettext("Total"),
            Some(&format!("{hours}:{mins:02}")),
            None,
        ));
    }

    for entry in entries {
        let started_local = entry.started_at.with_timezone(&chrono::Local);
        let started_label = started_local.format("%a %b %-d, %H:%M").to_string();
        let (title, mut subtitle, is_running) = match entry.duration_minutes() {
            Some(d) => {
                let h = d / 60;
                let m = d % 60;
                (format!("{h}:{m:02}"), started_label.clone(), false)
            }
            None => (
                gettext("Running"),
                // Translators: {time} is when the running session began,
                // e.g. "Mon Jul 6, 14:30".
                gettext_f("started {time}", &[("time", &started_label)]),
                true,
            ),
        };
        if !entry.note.is_empty() {
            // Translators: joins a clock session's start time and its
            // note; only the separator is yours to change.
            subtitle = gettext_f(
                "{subtitle} — {note}",
                &[("subtitle", subtitle.as_str()), ("note", &entry.note)],
            );
        }
        let row = crate::ui::rows::row(&title, Some(&subtitle), None);
        if is_running {
            row.add_css_class("atrium-clock-running");
        }
        group.add(&row);
    }

    group
}

pub(super) fn install_repeat_editor(
    group: &crate::ui::rows::Group,
    worker: &WorkerHandle,
    task: &Task,
) {
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
    let freq_choices = [
        // Translators: repeat-frequency choice meaning the task does
        // not repeat.
        pgettext("repeat frequency", "None"),
        gettext("Daily"),
        gettext("Weekly"),
        gettext("Monthly"),
        gettext("Yearly"),
        gettext("Custom"),
    ];
    let (freq_row, freq_dd) = crate::ui::rows::combo_row(
        &gettext("Repeat"),
        None,
        &freq_choices.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    freq_dd.set_selected(preset_index(initial_preset));

    // Translators: title of the interval spinner; reads as "Every N"
    // where N is the number of frequency units.
    let (interval_row, interval_spin) = crate::ui::rows::spin_row(
        &gettext("Every"),
        Some(&gettext("Number of frequency units between occurrences.")),
        1.0,
        365.0,
        1.0,
    );
    interval_spin.set_value(initial_interval as f64);

    let mode_choices = [
        gettext("After completion (Cumulative)"),
        gettext("From completion date (Next)"),
        gettext("Always shift by interval (Basic)"),
    ];
    let (mode_row, mode_dd) = crate::ui::rows::combo_row(
        &gettext("After completion"),
        None,
        &mode_choices.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    mode_dd.set_selected(mode_index(initial_mode));

    // Translators: "RRULE" is the RFC 5545 recurrence-rule keyword;
    // keep it verbatim.
    let (custom_row, custom_entry) =
        crate::ui::rows::entry_row(&gettext("Custom RRULE"), &initial_custom);

    let none_preset = matches!(initial_preset, RepeatPreset::None);
    interval_row.set_visible(!none_preset);
    mode_row.set_visible(!none_preset);
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
        let freq_dd = freq_dd.clone();
        let interval_spin = interval_spin.clone();
        let mode_dd = mode_dd.clone();
        let custom_entry = custom_entry.clone();
        Rc::new(move || {
            let preset = preset_from_index(freq_dd.selected());
            let interval = interval_spin.value().round().max(1.0) as u32;
            let mode = mode_from_index(mode_dd.selected());
            let custom_text = custom_entry.text().to_string();

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
        freq_dd.connect_selected_notify(move |dd| {
            let preset = preset_from_index(dd.selected());
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
        interval_spin.connect_value_changed(move |_| commit());
    }
    {
        let commit = commit.clone();
        mode_dd.connect_selected_notify(move |_| commit());
    }
    {
        // gtk::Entry has no adwaita "apply" button; Enter (activate) commits,
        // and the entry also commits on focus-out where it is wired by the
        // caller. Here the recurrence custom rule commits on Enter.
        let commit = commit.clone();
        custom_entry.connect_activate(move |_| commit());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RepeatPreset {
    None,
    Daily,
    Weekly,
    Monthly,
    Yearly,
    Custom,
}

pub(super) fn preset_index(p: RepeatPreset) -> u32 {
    match p {
        RepeatPreset::None => 0,
        RepeatPreset::Daily => 1,
        RepeatPreset::Weekly => 2,
        RepeatPreset::Monthly => 3,
        RepeatPreset::Yearly => 4,
        RepeatPreset::Custom => 5,
    }
}

pub(super) fn preset_from_index(i: u32) -> RepeatPreset {
    match i {
        1 => RepeatPreset::Daily,
        2 => RepeatPreset::Weekly,
        3 => RepeatPreset::Monthly,
        4 => RepeatPreset::Yearly,
        5 => RepeatPreset::Custom,
        _ => RepeatPreset::None,
    }
}

pub(super) fn mode_index(m: RepeatMode) -> u32 {
    match m {
        RepeatMode::Cumulative => 0,
        RepeatMode::Next => 1,
        RepeatMode::Basic => 2,
    }
}

pub(super) fn mode_from_index(i: u32) -> RepeatMode {
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
pub(super) fn preset_from_rule(rule: Option<&str>) -> RepeatPreset {
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

pub(super) fn interval_from_rule(rule: Option<&str>) -> Option<u32> {
    let rule = rule?;
    for token in rule.split(';') {
        let trimmed = token.trim();
        if let Some(rest) = trimmed.to_ascii_uppercase().strip_prefix("INTERVAL=") {
            return rest.trim().parse().ok();
        }
    }
    Some(1)
}

pub(super) fn rule_from_freq(freq: &str, interval: u32) -> String {
    if interval <= 1 {
        format!("FREQ={freq}")
    } else {
        format!("FREQ={freq};INTERVAL={interval}")
    }
}

pub(super) fn format_tag_count(n: usize) -> String {
    if n == 0 {
        gettext("No tags")
    } else {
        ngettext_f("{n} tag", "{n} tags", n as u32, &[("n", &n.to_string())])
    }
}

/// Wire a `gtk::Entry` to autosave on focus-out and on Enter (activate) — the
/// owned successor to adwaita's EntryRow "apply" signal. The closure gets both
/// the entry and the worker handle to dispatch updates with.
pub(super) fn wire_entry_autosave<F>(
    entry: &gtk::Entry,
    worker: WorkerHandle,
    _task_id: i64,
    save: F,
) where
    F: Fn(&gtk::Entry, &WorkerHandle) + Clone + 'static,
{
    let save_for_activate = save.clone();
    let worker_for_activate = worker.clone();
    entry.connect_activate(move |entry| {
        save_for_activate(entry, &worker_for_activate);
    });
    let save_for_focus = save.clone();
    let focus_ctrl = gtk::EventControllerFocus::new();
    let entry_weak = entry.downgrade();
    focus_ctrl.connect_leave(move |_| {
        if let Some(entry) = entry_weak.upgrade() {
            save_for_focus(&entry, &worker);
        }
    });
    entry.add_controller(focus_ctrl);
}

pub(super) fn build_schedule_button<F>(
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
        .label(gettext("Today"))
        .css_classes(["flat"])
        .build();
    let tomorrow_button = gtk::Button::builder()
        .label(gettext("Tomorrow"))
        .css_classes(["flat"])
        .build();
    let someday_button = gtk::Button::builder()
        .label(gettext("Someday"))
        .css_classes(["flat"])
        .build();
    let clear_button = gtk::Button::builder()
        .label(gettext("Clear"))
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

pub(super) fn build_date_button<F>(
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
        .label(gettext("Today"))
        .css_classes(["flat"])
        .build();
    let tomorrow_button = gtk::Button::builder()
        .label(gettext("Tomorrow"))
        .css_classes(["flat"])
        .build();
    let clear_button = gtk::Button::builder()
        .label(gettext("Clear"))
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

pub(super) fn build_project_combo_row(
    projects: &[Project],
    current: Option<i64>,
) -> (gtk::ListBoxRow, gtk::DropDown) {
    // Translators: first dropdown entry — the task belongs to no project.
    let inbox_label = gettext("Inbox (no project)");
    let mut items: Vec<&str> = vec![inbox_label.as_str()];
    for p in projects {
        items.push(p.title.as_str());
    }
    let (row, dropdown) = crate::ui::rows::combo_row(&gettext("Project"), None, &items);
    let pos: u32 = match current {
        None => 0,
        Some(id) => projects
            .iter()
            .position(|p| p.id == id)
            .map_or(0, |i| (i + 1) as u32),
    };
    dropdown.set_selected(pos);
    (row, dropdown)
}

pub(super) fn project_id_from_combo_row(
    dropdown: &gtk::DropDown,
    projects: &[Project],
) -> Option<i64> {
    let selected = dropdown.selected();
    if selected == 0 {
        return None;
    }
    let idx = (selected as usize).saturating_sub(1);
    projects.get(idx).map(|p| p.id)
}

pub(super) fn calendar_to_naive_date(cal: &gtk::Calendar) -> Option<NaiveDate> {
    let dt = cal.date();
    NaiveDate::from_ymd_opt(dt.year(), dt.month() as u32, dt.day_of_month() as u32)
}
