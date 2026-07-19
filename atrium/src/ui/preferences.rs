// SPDX-License-Identifier: MIT
//! Preferences window (Phase 19.5; plain GTK since Phase 22 C7).
//!
//! First app-level preferences surface. Closes a long-standing gap
//! (GSettings keys had no GUI; users had to edit via `gsettings`).
//! Four pages: General (mode, theme, vault path), Capture (Quick Entry
//! shortcut), Notifications (master on/off), Backups.
//!
//! All preferences write through to GSettings — no separate state. The
//! window is a thin presentation layer; the live GSettings keys remain
//! the source of truth.
//!
//! Wired to `app.preferences`. Was `AdwPreferencesDialog`; now a plain
//! modal `gtk::Window` with a `GtkStackSidebar` over the owned rows.
//! `apply_theme` sets GtkSettings' prefer-dark hint (was `adw::StyleManager`
//! before the C10 toolkit cut).

use atrium_core::APP_ID;
use gtk::gio;
use gtk::glib;
use gtk::glib::clone;
use gtk::prelude::*;

use crate::i18n::{gettext, gettext_f};
use crate::ui::rows;

/// Open the preferences window anchored to `parent`. Presents itself
/// and returns immediately.
pub fn open(parent: &impl IsA<gtk::Widget>) {
    let settings = gio::Settings::new(APP_ID);

    let stack = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .hexpand(true)
        .vexpand(true)
        .build();
    stack.add_titled(
        &general_page(&settings),
        Some("general"),
        &gettext("General"),
    );
    stack.add_titled(
        &capture_page(&settings),
        Some("capture"),
        &gettext("Capture"),
    );
    stack.add_titled(
        &notifications_page(&settings),
        Some("notifications"),
        &gettext("Notifications"),
    );
    stack.add_titled(
        &backups_page(&settings),
        Some("backups"),
        &gettext("Backups"),
    );

    let sidebar = gtk::StackSidebar::builder().stack(&stack).build();
    sidebar.set_size_request(170, -1);

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .build();
    body.append(&sidebar);
    body.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    body.append(&stack);

    let window = gtk::Window::builder()
        .title(gettext("Preferences"))
        .modal(true)
        .default_width(660)
        .default_height(540)
        .child(&body)
        .build();
    window.set_titlebar(Some(&gtk::HeaderBar::new()));

    if let Some(root) = parent
        .upcast_ref::<gtk::Widget>()
        .root()
        .and_downcast::<gtk::Window>()
    {
        window.set_transient_for(Some(&root));
    }

    // Escape closes (plain GTK has no adwaita auto-close).
    let key = gtk::EventControllerKey::new();
    let weak = window.downgrade();
    key.connect_key_pressed(move |_, keyval, _, _| {
        if keyval == gtk::gdk::Key::Escape {
            if let Some(w) = weak.upgrade() {
                w.close();
            }
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });
    window.add_controller(key);

    window.present();
}

fn general_page(settings: &gio::Settings) -> gtk::Widget {
    let page = rows::page();

    // ── Appearance ────────────────────────────────────────────────
    let appearance_group = rows::group(Some(&gettext("Appearance")), None);

    // Translators: the two Atrium UI modes.
    let mode_simple = gettext("Simple");
    let mode_builder = gettext("Builder");
    let (mode_row, mode_dd) = rows::combo_row(
        &gettext("Default mode"),
        Some(&gettext(
            "Simple is the calm Things-style surface; Builder adds Inspector pane, Forecast, Review.",
        )),
        &[mode_simple.as_str(), mode_builder.as_str()],
    );
    mode_dd.set_selected(if settings.string("mode") == "builder" {
        1
    } else {
        0
    });
    {
        let settings = settings.clone();
        mode_dd.connect_selected_notify(move |dd| {
            let value = if dd.selected() == 1 {
                "builder"
            } else {
                "simple"
            };
            let _ = settings.set_string("mode", value);
        });
    }
    appearance_group.add(&mode_row);

    let theme_follow = gettext("Follow system");
    let theme_light = gettext("Light");
    let theme_dark = gettext("Dark");
    let (theme_row, theme_dd) = rows::combo_row(
        &gettext("Theme"),
        Some(&gettext(
            "Override the system colour scheme. Adwaita auto-tracks the system; pin one here if you want it constant.",
        )),
        &[
            theme_follow.as_str(),
            theme_light.as_str(),
            theme_dark.as_str(),
        ],
    );
    theme_dd.set_selected(theme_to_index(&settings.string("theme")));
    {
        let settings = settings.clone();
        theme_dd.connect_selected_notify(move |dd| {
            let value = match dd.selected() {
                1 => "light",
                2 => "dark",
                _ => "auto",
            };
            let _ = settings.set_string("theme", value);
            apply_theme(value);
        });
    }
    appearance_group.add(&theme_row);

    // Translators: "Atkinson Hyperlegible" is a typeface name — keep it as-is.
    let (high_leg_row, high_leg_switch) = rows::switch_row(
        &gettext("High-legibility font"),
        Some(&gettext(
            "Atkinson Hyperlegible — designed by the Braille Institute for low-vision readers.",
        )),
    );
    high_leg_switch.set_active(settings.boolean("high-legibility-font"));
    {
        let settings = settings.clone();
        high_leg_switch.connect_active_notify(move |sw| {
            let _ = settings.set_boolean("high-legibility-font", sw.is_active());
        });
    }
    appearance_group.add(&high_leg_row);

    page.add(&appearance_group);

    // ── Vault ─────────────────────────────────────────────────────
    let vault_group = rows::group(
        Some(&gettext("Org vault")),
        // Translators: `.org` and `~/Tasks/` are literal file-system names — keep them untranslated.
        Some(&gettext(
            "Path to a directory holding `.org` files Atrium projects its data into. \
             Empty = no vault (DB-only). Convention is ~/Tasks/.",
        )),
    );

    let vault_label = gtk::Label::builder()
        .label(gettext("Vault path"))
        .xalign(0.0)
        .build();
    let vault_entry = gtk::Entry::builder().hexpand(true).build();
    vault_entry.set_text(&settings.string("vault-path"));
    {
        let settings = settings.clone();
        vault_entry.connect_changed(move |entry| {
            let _ = settings.set_string("vault-path", &entry.text());
        });
    }
    let pick_button = gtk::Button::builder()
        .icon_name("folder-open-symbolic")
        .tooltip_text(gettext("Choose folder…"))
        .css_classes(["flat"])
        .valign(gtk::Align::Center)
        .build();
    pick_button.update_property(&[gtk::accessible::Property::Label(&gettext(
        "Choose vault folder",
    ))]);
    {
        let entry = vault_entry.clone();
        pick_button.connect_clicked(clone!(
            #[weak]
            entry,
            move |btn| {
                let parent = btn.root().and_downcast::<gtk::Window>();
                let dialog = gtk::FileDialog::builder()
                    .title(gettext("Choose vault folder"))
                    .modal(true)
                    .build();
                dialog.select_folder(parent.as_ref(), gio::Cancellable::NONE, move |result| {
                    if let Ok(folder) = result
                        && let Some(path) = folder.path()
                    {
                        entry.set_text(&path.display().to_string());
                    }
                });
            }
        ));
    }
    let vault_hbox = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .margin_top(8)
        .margin_bottom(8)
        .margin_start(12)
        .margin_end(12)
        .build();
    vault_hbox.append(&vault_label);
    vault_hbox.append(&vault_entry);
    vault_hbox.append(&pick_button);
    let vault_row = gtk::ListBoxRow::builder()
        .activatable(false)
        .child(&vault_hbox)
        .build();
    vault_group.add(&vault_row);

    page.add(&vault_group);

    page.widget().clone()
}

fn capture_page(settings: &gio::Settings) -> gtk::Widget {
    let page = rows::page();
    let group = rows::group(
        Some(&gettext("Quick Entry")),
        // Translators: `<Control><Alt>space` is a literal GTK accelerator string — keep it untranslated.
        Some(&gettext(
            "Global shortcut that opens the Quick Entry modal. GTK accelerator syntax \
             (e.g. `<Control><Alt>space`).",
        )),
    );

    let (shortcut_row, shortcut_entry) = rows::entry_row(
        &gettext("Shortcut"),
        &settings.string("quick-entry-shortcut"),
    );
    {
        let settings = settings.clone();
        shortcut_entry.connect_changed(move |entry| {
            let _ = settings.set_string("quick-entry-shortcut", &entry.text());
        });
    }
    group.add(&shortcut_row);

    page.add(&group);
    page.widget().clone()
}

fn notifications_page(settings: &gio::Settings) -> gtk::Widget {
    let page = rows::page();
    let group = rows::group(
        Some(&gettext("Reminders")),
        // Translators: `reminder_at` is a literal field name — keep it untranslated.
        Some(&gettext(
            "Time-based reminders fire as system notifications when this is on. \
             Per-task `reminder_at` timestamps drive the schedule.",
        )),
    );

    let (enabled_row, enabled_switch) =
        rows::switch_row(&gettext("Enable system notifications"), None);
    enabled_switch.set_active(settings.boolean("notifications-enabled"));
    {
        let settings = settings.clone();
        enabled_switch.connect_active_notify(move |sw| {
            let _ = settings.set_boolean("notifications-enabled", sw.is_active());
        });
    }
    group.add(&enabled_row);

    page.add(&group);
    page.widget().clone()
}

/// v0.32.0 — Backups page. "Back up now" writes a `VACUUM INTO`
/// snapshot and prunes to the newest 10; "Restore from backup…"
/// queues a snapshot to be copied over the live DB on next launch;
/// the switch toggles the opportunistic weekly auto-backup GSetting.
fn backups_page(settings: &gio::Settings) -> gtk::Widget {
    let page = rows::page();
    let group = rows::group(
        Some(&gettext("Database backups")),
        // Translators: `backups/` is a literal folder name — keep it untranslated.
        Some(&gettext(
            "Snapshots live in the Atrium data directory's `backups/` folder \
             (the newest ten are kept).",
        )),
    );

    // Back up now.
    let backup_btn = gtk::Button::builder()
        .label(gettext("Back up"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    let (backup_row, backup_subtitle) = rows::action_row(
        &gettext("Back up now"),
        Some(&gettext("Write a snapshot of the current database.")),
        Some(backup_btn.upcast_ref()),
    );
    backup_btn.connect_clicked(clone!(
        #[weak]
        backup_subtitle,
        move |_| {
            let dir = atrium_core::paths::backups_dir();
            match atrium_core::backup::backup_now(&atrium_core::db_path(), &dir) {
                Ok(path) => {
                    let _ = atrium_core::backup::prune(&dir, 10);
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    backup_subtitle
                        .set_label(&gettext_f("Backed up to {name}", &[("name", &name)]));
                }
                Err(e) => backup_subtitle.set_label(&gettext_f(
                    "Backup failed: {error}",
                    &[("error", &e.to_string())],
                )),
            }
        }
    ));
    group.add(&backup_row);

    // Restore from backup.
    let restore_btn = gtk::Button::builder()
        .label(gettext("Restore…"))
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    let (restore_row, restore_subtitle) = rows::action_row(
        &gettext("Restore from backup…"),
        Some(&gettext("Replace the current database on the next launch.")),
        Some(restore_btn.upcast_ref()),
    );
    restore_btn.connect_clicked(clone!(
        #[weak]
        restore_subtitle,
        move |btn| {
            let window = btn.root().and_downcast::<gtk::Window>();
            let filter = gtk::FileFilter::new();
            filter.set_name(Some(&gettext("Atrium backups")));
            filter.add_pattern("atrium.*.db");
            filter.add_suffix("db");
            let filters = gio::ListStore::new::<gtk::FileFilter>();
            filters.append(&filter);
            let dialog = gtk::FileDialog::builder()
                .title(gettext("Restore from backup"))
                .filters(&filters)
                .build();
            if let Some(dir) = atrium_core::paths::backups_dir().to_str() {
                dialog.set_initial_folder(Some(&gio::File::for_path(dir)));
            }
            let restore_subtitle = restore_subtitle.clone();
            dialog.open(window.as_ref(), gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res
                    && let Some(path) = file.path()
                {
                    match std::fs::write(
                        atrium_core::paths::restore_marker_path(),
                        path.to_string_lossy().as_bytes(),
                    ) {
                        Ok(()) => restore_subtitle
                            .set_label(&gettext("Restore queued — restart Atrium to apply.")),
                        Err(e) => restore_subtitle.set_label(&gettext_f(
                            "Could not queue restore: {error}",
                            &[("error", &e.to_string())],
                        )),
                    }
                }
            });
        }
    ));
    group.add(&restore_row);

    // Weekly auto-backup toggle.
    let (weekly_row, weekly_switch) = rows::switch_row(
        &gettext("Weekly automatic backup"),
        Some(&gettext(
            "On launch, snapshot if the newest backup is over a week old.",
        )),
    );
    weekly_switch.set_active(settings.boolean("backup-weekly"));
    {
        let settings = settings.clone();
        weekly_switch.connect_active_notify(move |sw| {
            let _ = settings.set_boolean("backup-weekly", sw.is_active());
        });
    }
    group.add(&weekly_row);

    page.add(&group);
    page.widget().clone()
}

fn theme_to_index(value: &str) -> u32 {
    match value {
        "light" => 1,
        "dark" => 2,
        _ => 0,
    }
}

/// Apply the persisted theme preference. Called when the user picks a
/// theme in Preferences; the boot path also calls it once with the stored
/// value. Was `adw::StyleManager` / `adw::ColorScheme` before the C10
/// toolkit cut; now it sets GtkSettings' prefer-dark hint.
///
/// Atrium ships the dark Kanagawa Dragon sheet (`theme.rs`), so "dark" and
/// the "auto" default both render dark; "light" only nudges the prefer-dark
/// hint for any GTK-default-drawn bits the owned sheet doesn't cover. A true
/// light (Lotus) palette is post-1.0 work.
pub fn apply_theme(value: &str) {
    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    settings.set_gtk_application_prefer_dark_theme(!matches!(value, "light"));
}
