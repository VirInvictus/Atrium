// SPDX-License-Identifier: MIT
//! `AtriumWindow` free-function helpers: primary menu, sidebar row /
//! badge / section builders, search-help popover, history ring-buffer
//! helpers, and small predicates. Extracted from window/mod.rs in
//! v0.22.0 split (Pass 2).

use super::*;

/// Build the primary (hamburger) menu. `include_debug` adds the
/// fixture-generator submenu for `--debug` runs.
pub(crate) fn build_primary_menu(include_debug: bool) -> gio::Menu {
    let menu = gio::Menu::new();

    let new_section = gio::Menu::new();
    new_section.append(Some("New Task"), Some("app.new-task"));
    new_section.append(Some("Quick Entry"), Some("app.quick-entry"));
    new_section.append(Some("New Project"), Some("app.new-project"));
    new_section.append(Some("New from Template…"), Some("app.new-from-template"));
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

    // v0.34.0 — unified import dialog (Org / Todoist / VTODO /
    // Taskwarrior / todo.txt).
    let io_section = gio::Menu::new();
    io_section.append(Some("Import…"), Some("app.import"));
    menu.append_section(None, &io_section);

    let about_section = gio::Menu::new();
    // v0.20.0 — Phase 19.5 preferences entry. Above the
    // shortcuts/about line so it reads as part of the "primary"
    // app actions; standard GNOME convention puts Preferences
    // before About.
    about_section.append(Some("Preferences…"), Some("app.preferences"));
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
pub(super) const TAG_COLORS: &[(&str, Option<&str>)] = &[
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
pub(super) async fn prompt_for_named_color(
    parent: &impl IsA<gtk::Widget>,
    heading: &str,
    placeholder: &str,
    name_initial: &str,
    color_initial: Option<&str>,
    confirm_label: &str,
    review_initial: Option<i64>,
) -> Option<(String, Option<String>, Option<i64>)> {
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

    // v0.28.0 — optional Review-cadence row. Only the area dialogs opt
    // in (`Some`); tag dialogs pass `None` and keep the original
    // name + colour form. 0 means "no default" (cleared).
    let review_spin = review_initial.map(|initial| {
        let row = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(12)
            .build();
        let label = gtk::Label::builder()
            .label("Review every (days, 0 = off)")
            .halign(gtk::Align::Start)
            .hexpand(true)
            .xalign(0.0)
            .build();
        let spin = gtk::SpinButton::with_range(0.0, 365.0, 1.0);
        spin.set_value(initial.max(0) as f64);
        row.append(&label);
        row.append(&spin);
        body.append(&row);
        spin
    });

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
            let review = review_spin.map(|s| s.value().round() as i64);
            Some((text, selected_color.borrow().clone(), review))
        }
    } else {
        None
    }
}

pub(super) async fn prompt_for_text(
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

/// The board axis the user picked in a renderer dialog, or `None`
/// for the List renderer. Shared by the two renderer dialogs so the
/// radio → config translation lives in one place.
#[derive(Clone, Copy)]
enum RendererChoice {
    List,
    Board(atrium_core::BoardAxis),
}

/// Translate a renderer dialog's radio + columns-entry state into a
/// `(renderer, config_json)` pair. Tag boards split on commas; status
/// boards use the Org `#+TODO:` pipe convention. An empty column set
/// falls back to List, since the kanban can't render a config the
/// worker's `Renderer::from_columns` would reject.
fn renderer_pair_from_choice(
    choice: RendererChoice,
    columns_text: &str,
) -> (String, Option<String>) {
    let axis = match choice {
        RendererChoice::List => return ("list".into(), None),
        RendererChoice::Board(axis) => axis,
    };
    let (columns, done_columns) = match axis {
        atrium_core::BoardAxis::Tag => {
            let cols: Vec<String> = columns_text
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            (cols, Vec::new())
        }
        atrium_core::BoardAxis::Status => atrium_core::parse_status_columns(columns_text),
    };
    if columns.is_empty() {
        return ("list".into(), None);
    }
    let cfg = atrium_core::BoardConfig {
        axis,
        columns,
        done_columns,
    };
    ("board".into(), cfg.to_json().ok())
}

/// Inspect a perspective's stored renderer config for the existing
/// axis (if it's a board) and the entry text to prefill — columns
/// joined for a tag board, the pipe convention for a status board.
fn existing_board_state(
    perspective: &atrium_core::Perspective,
) -> (Option<atrium_core::BoardAxis>, String) {
    let cfg = perspective
        .renderer_config
        .as_deref()
        .and_then(|json| atrium_core::BoardConfig::from_json(json).ok());
    let is_board = perspective.renderer.eq_ignore_ascii_case("board");
    match cfg {
        Some(cfg) if is_board && cfg.axis == atrium_core::BoardAxis::Status => (
            Some(atrium_core::BoardAxis::Status),
            atrium_core::format_status_columns(&cfg),
        ),
        Some(cfg) if is_board => (Some(atrium_core::BoardAxis::Tag), cfg.columns.join(", ")),
        _ => (None, String::new()),
    }
}

/// The static hint text + placeholder for the columns entry under a
/// given board axis.
fn columns_hint(axis: atrium_core::BoardAxis) -> (&'static str, &'static str) {
    match axis {
        atrium_core::BoardAxis::Tag => {
            ("Columns (comma-separated tag names):", "todo, doing, done")
        }
        atrium_core::BoardAxis::Status => (
            "Columns (Org #+TODO: keywords; “|” before done states):",
            "TODO, NEXT, WAITING | DONE, CANCELLED",
        ),
    }
}

/// Build the List / Board-by-tag / Board-by-status radio group plus
/// the columns entry, wired so the entry's sensitivity + hint track
/// the active radio. Returns the three radios and the entry; the
/// caller reads their final state. Appends everything to `form`.
fn build_renderer_form(
    form: &gtk::Box,
    existing_axis: Option<atrium_core::BoardAxis>,
    existing_cols_text: &str,
) -> (
    gtk::CheckButton,
    gtk::CheckButton,
    gtk::CheckButton,
    gtk::Entry,
) {
    let is_tag = existing_axis == Some(atrium_core::BoardAxis::Tag);
    let is_status = existing_axis == Some(atrium_core::BoardAxis::Status);

    let list_radio = gtk::CheckButton::builder()
        .label("List \u{2014} flat task list (default)")
        .active(existing_axis.is_none())
        .build();
    let tag_radio = gtk::CheckButton::builder()
        .label("Board \u{2014} columns by tag")
        .active(is_tag)
        .build();
    let status_radio = gtk::CheckButton::builder()
        .label("Board \u{2014} columns by status (Org keywords)")
        .active(is_status)
        .build();
    tag_radio.set_group(Some(&list_radio));
    status_radio.set_group(Some(&list_radio));
    form.append(&list_radio);
    form.append(&tag_radio);
    form.append(&status_radio);

    // Initial hint follows the active axis (tag is the board default).
    let initial_axis = existing_axis.unwrap_or(atrium_core::BoardAxis::Tag);
    let (initial_label, initial_placeholder) = columns_hint(initial_axis);

    let columns_label = gtk::Label::builder()
        .label(initial_label)
        .halign(gtk::Align::Start)
        .build();
    columns_label.add_css_class("dim-label");
    form.append(&columns_label);

    let columns_entry = gtk::Entry::builder()
        .placeholder_text(initial_placeholder)
        .text(existing_cols_text)
        .activates_default(true)
        .build();
    columns_entry.set_sensitive(existing_axis.is_some());
    form.append(&columns_entry);

    // Each board radio, on becoming active, enables the entry and
    // swaps in its axis-specific hint; the List radio disables it.
    for (radio, axis) in [
        (&tag_radio, atrium_core::BoardAxis::Tag),
        (&status_radio, atrium_core::BoardAxis::Status),
    ] {
        let entry = columns_entry.clone();
        let label = columns_label.clone();
        radio.connect_active_notify(move |btn| {
            if btn.is_active() {
                let (hint, placeholder) = columns_hint(axis);
                entry.set_sensitive(true);
                label.set_text(hint);
                entry.set_placeholder_text(Some(placeholder));
            }
        });
    }
    let entry_for_list = columns_entry.clone();
    list_radio.connect_active_notify(move |btn| {
        if btn.is_active() {
            entry_for_list.set_sensitive(false);
        }
    });

    (list_radio, tag_radio, status_radio, columns_entry)
}

/// Read the active board axis out of the three renderer radios.
fn choice_from_radios(
    tag_radio: &gtk::CheckButton,
    status_radio: &gtk::CheckButton,
) -> RendererChoice {
    if status_radio.is_active() {
        RendererChoice::Board(atrium_core::BoardAxis::Status)
    } else if tag_radio.is_active() {
        RendererChoice::Board(atrium_core::BoardAxis::Tag)
    } else {
        RendererChoice::List
    }
}

/// v0.6.2 — perspective renderer configuration dialog. Pick `List`,
/// `Board` by tag, or `Board` by status; for a board, edit the
/// columns in a single text entry (comma-separated tags, or Org
/// `#+TODO:` keywords for the status axis). Returns `(renderer,
/// config_json)` on confirm, `None` on cancel. The config_json is
/// `None` for List or an empty board; `Some(json)` otherwise.
pub(super) async fn prompt_configure_renderer_dialog(
    parent: &impl IsA<gtk::Widget>,
    perspective: &atrium_core::Perspective,
) -> Option<(String, Option<String>)> {
    let (existing_axis, existing_cols_text) = existing_board_state(perspective);

    let form = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();
    let (_list_radio, tag_radio, status_radio, columns_entry) =
        build_renderer_form(&form, existing_axis, &existing_cols_text);

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
    let choice = choice_from_radios(&tag_radio, &status_radio);
    Some(renderer_pair_from_choice(choice, &columns_entry.text()))
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
pub(super) async fn prompt_edit_perspective(
    parent: &impl IsA<gtk::Widget>,
    existing: Option<&atrium_core::Perspective>,
) -> Option<EditedPerspectiveFields> {
    let (existing_name, existing_filter, existing_axis, existing_cols_text) = match existing {
        Some(p) => {
            let (axis, cols_text) = existing_board_state(p);
            (p.name.clone(), p.filter_expr.clone(), axis, cols_text)
        }
        None => (String::new(), String::new(), None, String::new()),
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

    let (_list_radio, tag_radio, status_radio, columns_entry) =
        build_renderer_form(&form, existing_axis, &existing_cols_text);

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

    let choice = choice_from_radios(&tag_radio, &status_radio);
    let (renderer, renderer_config) = renderer_pair_from_choice(choice, &columns_entry.text());

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
pub(super) fn tag_lists_equal_case_insensitive(a: &[String], b: &[String]) -> bool {
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
pub(super) async fn prompt_confirm_destructive(
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

pub(super) fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let head: String = s.chars().take(max_chars).collect();
        format!("{head}…")
    }
}

pub(super) fn build_canonical_row(active: &ActiveList) -> (gtk::ListBoxRow, gtk::Label) {
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
pub(super) const SEARCH_HISTORY_MAX: usize = 20;

/// v0.4.1 — build the operator-reference popover for the `?` menu
/// button on the search bar. Compact quick-reference, organised by
/// section, with monospace operator examples paired against
/// short descriptions. Sections cover the boolean / field /
/// modifier / comparison / date / state / sort layers of the
/// expression language; spec.md §4.3 is the authoritative deeper
/// reference.
pub(super) fn build_search_help_popover() -> gtk::Popover {
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
pub(super) fn build_help_section(title: &str, rows: &[(&str, &str)]) -> gtk::Box {
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
pub(super) enum HistoryDirection {
    /// ↑ — toward older entries (lower indices in our newest-last vec).
    Older,
    /// ↓ — toward newer / "current" entry.
    Newer,
}

/// Append `entry` to the history buffer, deduplicating against the
/// most-recent entry (so repeatedly running the same query doesn't
/// flood the buffer) and capping at `max` entries (drops from the
/// front when full). Empty / whitespace-only entries are ignored.
pub(super) fn push_history_entry(history: &mut Vec<String>, entry: String, max: usize) {
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
pub(super) fn cycle_history_cursor(
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
pub(super) fn canonical_accent_class(active: &ActiveList) -> Option<&'static str> {
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
pub(super) fn top_tier_extras(builder: bool) -> Vec<(ActiveList, &'static str)> {
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

pub(super) fn build_area_row(area: &Area) -> (gtk::ListBoxRow, gtk::Label) {
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

pub(super) fn build_project_row(
    project: &Project,
    indented: bool,
) -> (gtk::ListBoxRow, gtk::Label) {
    let margin = if indented { 24 } else { 8 };
    sidebar_row(
        icon_for(&ActiveList::Project(project.id)),
        &project.title,
        margin,
    )
}

pub(super) fn build_tag_row(tag: &Tag) -> (gtk::ListBoxRow, gtk::Label) {
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
pub(super) fn swatch_class_for_hex(hex: &str) -> Option<&'static str> {
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

pub(super) fn build_section_header(label: &str) -> gtk::ListBoxRow {
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

pub(super) fn sidebar_row(
    icon: &str,
    label: &str,
    margin_start: i32,
) -> (gtk::ListBoxRow, gtk::Label) {
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
pub(super) fn available_count(open: i64, sequential: bool) -> i64 {
    if sequential && open > 0 { 1 } else { open }
}

/// Set a badge label's text from a count, hiding when zero.
pub(super) fn apply_badge_label(badge: &gtk::Label, count: i64) {
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
pub(super) fn find_task_row(start: &gtk::Widget) -> Option<gtk::Widget> {
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
pub(super) fn compute_sidebar_visibility(
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
