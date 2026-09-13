// SPDX-License-Identifier: MIT
//! Shared plain-GTK list rows and groups — the `adw::ActionRow` /
//! `PreferencesGroup` family replacement. The row builders and `Group`
//! moved into vir-gtk's widget kit (1.4.0) and are re-exported here under
//! the historical paths; what stays app-side is Atrium's own [`Page`],
//! [`Bin`], and [`set_box_child`] helpers (the slice rule: only the
//! verbatim mass moved).
//!
//! Styling leans on the `.heading` / `.caption` / `.dim-label` / `.boxed-list`
//! utility classes from the shared base sheet. The exact metrics need not
//! pixel-match adwaita — same structure and behaviour is the contract.

pub use vir_gtk::widgets::{
    Group, action_row, combo_row, entry_row, group, row, spin_row, switch_row,
};

use gtk::prelude::*;

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
        set_box_child(&self.root, child);
    }
}

/// Replace the single child of a `gtk::Box` used as a bin-style host (the
/// `adw::Bin::set_child` analogue for the `.ui`-template stack-page hosts,
/// which must be real GObjects and so are `GtkBox`, not [`Bin`]). `None`
/// clears it.
pub fn set_box_child(host: &gtk::Box, child: Option<&impl IsA<gtk::Widget>>) {
    while let Some(existing) = host.first_child() {
        host.remove(&existing);
    }
    if let Some(child) = child {
        host.append(child);
    }
}
