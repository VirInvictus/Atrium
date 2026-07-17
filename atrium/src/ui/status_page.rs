// SPDX-License-Identifier: MIT
//! Owned empty-state composite — the `adw::StatusPage` replacement
//! (Phase 22 C2). A centered icon / title / description column with an
//! optional call-to-action child below. The setters mirror the adwaita
//! names so call sites convert mechanically.
//!
//! Styling leans on the `.title-1` / `.dim-label` utility classes. While
//! libadwaita is still linked those come from its stylesheet; the owned
//! sheet provides them at C9. The exact spacing / icon weight need not
//! pixel-match adwaita — same copy, same behaviour is the C2 contract;
//! the look converges at the visual flip.

use gtk::prelude::*;

/// A built empty-state page. Cheap to clone (holds refcounted widgets);
/// keep a clone when the title/description are swapped at runtime, or
/// drop it and keep only [`StatusPage::widget`] for fire-and-forget use.
#[derive(Clone)]
pub struct StatusPage {
    root: gtk::Box,
    icon: gtk::Image,
    title: gtk::Label,
    description: gtk::Label,
}

/// Build a status page. `icon_name` and `description` are optional and
/// hide their widget when absent (matching adwaita, which omits them).
pub fn status_page(icon_name: Option<&str>, title: &str, description: Option<&str>) -> StatusPage {
    let icon = gtk::Image::builder()
        .pixel_size(96)
        .css_classes(["dim-label"])
        .build();
    icon.set_icon_name(icon_name);
    icon.set_visible(icon_name.is_some());

    let title_label = gtk::Label::builder()
        .label(title)
        .wrap(true)
        .justify(gtk::Justification::Center)
        .css_classes(["title-1"])
        .build();

    let description_label = gtk::Label::builder()
        .wrap(true)
        .justify(gtk::Justification::Center)
        .css_classes(["dim-label"])
        .build();
    description_label.set_label(description.unwrap_or_default());
    description_label.set_visible(description.is_some_and(|d| !d.is_empty()));

    let root = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .hexpand(true)
        .vexpand(true)
        .margin_top(24)
        .margin_bottom(24)
        .margin_start(24)
        .margin_end(24)
        .build();
    root.append(&icon);
    root.append(&title_label);
    root.append(&description_label);

    StatusPage {
        root,
        icon,
        title: title_label,
        description: description_label,
    }
}

impl StatusPage {
    /// The widget to place in a stack or container.
    pub fn widget(&self) -> &gtk::Widget {
        self.root.upcast_ref()
    }

    /// Append a call-to-action child below the description (the
    /// onboarding next-steps buttons are the current consumer).
    pub fn set_child(&self, child: &impl IsA<gtk::Widget>) {
        self.root.append(child);
    }

    pub fn set_icon_name(&self, icon_name: Option<&str>) {
        self.icon.set_icon_name(icon_name);
        self.icon.set_visible(icon_name.is_some());
    }

    pub fn set_title(&self, title: &str) {
        self.title.set_label(title);
    }

    pub fn set_description(&self, description: Option<&str>) {
        self.description.set_label(description.unwrap_or_default());
        self.description
            .set_visible(description.is_some_and(|d| !d.is_empty()));
    }
}
