// SPDX-License-Identifier: MIT
//! Shared plain-GTK list rows and groups — the `adw::ActionRow` /
//! `PreferencesGroup` / `PreferencesPage` family replacement (Phase 22 C5).
//!
//! Free-function builders return a `gtk::ListBoxRow` plus, where the row
//! carries an interactive control, the control itself, so call sites keep the
//! exact `set_active` / `selected` / `text` surface they wired against. Ported
//! from Conservatory's Phase 26 module (itself from the Colophon pilot); the
//! `Page` successor is Atrium's addition.
//!
//! Styling leans on the `.heading` / `.caption` / `.dim-label` / `.boxed-list`
//! utility classes (from adwaita while it's linked; from the owned sheet at
//! C9). The exact metrics need not pixel-match adwaita — same structure and
//! behaviour is the contract; the look converges at the visual flip.

use gtk::pango;
use gtk::prelude::*;

/// The shared row body: title over an optional dim subtitle on the left, an
/// optional trailing suffix on the right. Returns the subtitle label so
/// [`action_row`] can hand it out for later mutation; it starts hidden when the
/// subtitle is absent or empty.
fn build_row(
    title: &str,
    subtitle: Option<&str>,
    suffix: Option<&gtk::Widget>,
) -> (gtk::ListBoxRow, gtk::Label) {
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    let title_label = gtk::Label::builder()
        .label(title)
        .xalign(0.0)
        .ellipsize(pango::EllipsizeMode::End)
        .build();
    text.append(&title_label);
    let subtitle_label = gtk::Label::builder()
        .label(subtitle.unwrap_or_default())
        .xalign(0.0)
        .ellipsize(pango::EllipsizeMode::End)
        .css_classes(["caption", "dim-label"])
        .build();
    subtitle_label.set_tooltip_text(subtitle);
    subtitle_label.set_visible(subtitle.is_some_and(|s| !s.is_empty()));
    text.append(&subtitle_label);

    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(&text);
    if let Some(suffix) = suffix {
        suffix.set_valign(gtk::Align::Center);
        content.append(suffix);
    }
    let row = gtk::ListBoxRow::builder()
        .activatable(false)
        .child(&content)
        .build();
    (row, subtitle_label)
}

/// A non-activatable list row. Long titles and subtitles ellipsize; the
/// subtitle carries itself as a tooltip so nothing is lost to the cut.
pub fn row(title: &str, subtitle: Option<&str>, suffix: Option<&gtk::Widget>) -> gtk::ListBoxRow {
    build_row(title, subtitle, suffix).0
}

/// An `adw::ActionRow` successor for rows whose subtitle changes at runtime:
/// the returned label is the subtitle (kept visible so updates always show).
pub fn action_row(
    title: &str,
    subtitle: Option<&str>,
    suffix: Option<&gtk::Widget>,
) -> (gtk::ListBoxRow, gtk::Label) {
    let (row, label) = build_row(title, subtitle, suffix);
    label.set_visible(true);
    (row, label)
}

/// An `adw::SwitchRow` successor; the returned `gtk::Switch` keeps the exact
/// `set_active` / `is_active` / `connect_active_notify` surface.
pub fn switch_row(title: &str, subtitle: Option<&str>) -> (gtk::ListBoxRow, gtk::Switch) {
    let switch = gtk::Switch::new();
    let row = row(title, subtitle, Some(switch.upcast_ref()));
    (row, switch)
}

/// An `adw::SpinRow` successor; the returned `gtk::SpinButton` carries the
/// `set_digits` / `set_value` / `value` / `adjustment` surface.
pub fn spin_row(
    title: &str,
    subtitle: Option<&str>,
    min: f64,
    max: f64,
    step: f64,
) -> (gtk::ListBoxRow, gtk::SpinButton) {
    let spin = gtk::SpinButton::with_range(min, max, step);
    let row = row(title, subtitle, Some(spin.upcast_ref()));
    (row, spin)
}

/// An `adw::ComboRow` successor; the returned `gtk::DropDown` carries the
/// `set_selected` / `selected` / `connect_selected_notify` surface.
pub fn combo_row(
    title: &str,
    subtitle: Option<&str>,
    items: &[&str],
) -> (gtk::ListBoxRow, gtk::DropDown) {
    let dropdown = gtk::DropDown::from_strings(items);
    let row = row(title, subtitle, Some(dropdown.upcast_ref()));
    (row, dropdown)
}

/// An `adw::EntryRow` successor; the returned `gtk::Entry` carries the
/// `set_text` / `text` / `connect_changed` / `connect_activate` surface. (The
/// adwaita apply button has no analogue — callers that used `connect_apply`
/// switch to `connect_activate` / a nearby confirm button.)
pub fn entry_row(title: &str, text: &str) -> (gtk::ListBoxRow, gtk::Entry) {
    let entry = gtk::Entry::builder().text(text).hexpand(true).build();
    let row = row(title, None, Some(entry.upcast_ref()));
    (row, entry)
}

/// An `adw::PreferencesGroup` successor: an optional heading and dim
/// description (with room for a trailing header suffix) over a `.boxed-list`
/// of rows.
pub struct Group {
    root: gtk::Box,
    header: gtk::Box,
    list: gtk::ListBox,
}

pub fn group(title: Option<&str>, description: Option<&str>) -> Group {
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    if let Some(title) = title.filter(|t| !t.is_empty()) {
        text.append(
            &gtk::Label::builder()
                .label(title)
                .xalign(0.0)
                .css_classes(["heading"])
                .build(),
        );
    }
    if let Some(description) = description.filter(|d| !d.is_empty()) {
        text.append(
            &gtk::Label::builder()
                .label(description)
                .xalign(0.0)
                .wrap(true)
                .css_classes(["caption", "dim-label"])
                .build(),
        );
    }
    let header = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .visible(title.is_some() || description.is_some())
        .build();
    header.append(&text);
    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .build();
    root.append(&header);
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    root.append(&list);
    Group { root, header, list }
}

impl Group {
    /// The widget to place (a dialog extra child, a page section).
    pub fn widget(&self) -> &gtk::Widget {
        self.root.upcast_ref()
    }

    /// A trailing widget in the header line (the way
    /// `adw::PreferencesGroup::set_header_suffix` placed one). Reveals the
    /// header even when the group has no title/description.
    pub fn set_header_suffix(&self, suffix: &impl IsA<gtk::Widget>) {
        suffix.as_ref().set_valign(gtk::Align::Center);
        self.header.append(suffix);
        self.header.set_visible(true);
    }

    /// Append a row; any non-row widget is wrapped in a non-activatable row,
    /// the way `adw::PreferencesGroup::add` did.
    pub fn add(&self, child: &impl IsA<gtk::Widget>) {
        if let Some(row) = child.as_ref().downcast_ref::<gtk::ListBoxRow>() {
            self.list.append(row);
        } else {
            let wrapper = gtk::ListBoxRow::builder()
                .activatable(false)
                .child(child)
                .build();
            self.list.append(&wrapper);
        }
    }

    /// Remove every row (for groups whose contents rebuild at runtime, e.g.
    /// the inspector checklist).
    pub fn clear(&self) {
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
    }
}

/// An `adw::PreferencesPage` successor: a vertically-scrolling column of
/// [`Group`]s, clamped to a comfortable reading width.
pub struct Page {
    scroller: gtk::ScrolledWindow,
    list: gtk::Box,
}

pub fn page() -> Page {
    let list = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(18)
        .margin_top(18)
        .margin_bottom(18)
        .margin_start(18)
        .margin_end(18)
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&list)
        .build();
    Page { scroller, list }
}

impl Page {
    /// The widget to place (a dialog child, a window content pane).
    pub fn widget(&self) -> &gtk::Widget {
        self.scroller.upcast_ref()
    }

    pub fn add(&self, group: &Group) {
        self.list.append(group.widget());
    }

    /// Add an arbitrary widget as a page section (a bespoke section that
    /// isn't a plain [`Group`] — e.g. a heading over a dynamic list).
    pub fn add_widget(&self, child: &impl IsA<gtk::Widget>) {
        self.list.append(child);
    }
}

/// An `adw::Bin` successor: a single-child host whose child is swapped at
/// runtime via [`Bin::set_child`]. Used where content is rebuilt in place
/// (the inspector pane's editor host, the stack-page view hosts).
pub struct Bin {
    root: gtk::Box,
}

pub fn bin() -> Bin {
    let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
    root.set_hexpand(true);
    root.set_vexpand(true);
    Bin { root }
}

impl Bin {
    /// The widget to place.
    pub fn widget(&self) -> &gtk::Widget {
        self.root.upcast_ref()
    }

    /// Replace the (single) child. `None` clears it. Mirrors
    /// `adw::Bin::set_child`.
    pub fn set_child(&self, child: Option<&impl IsA<gtk::Widget>>) {
        while let Some(existing) = self.root.first_child() {
            self.root.remove(&existing);
        }
        if let Some(child) = child {
            self.root.append(child);
        }
    }
}
