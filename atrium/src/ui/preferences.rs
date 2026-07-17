// SPDX-License-Identifier: MIT
//! v0.20.0 — Phase 19.5 AdwPreferencesWindow.
//!
//! First app-level preferences dialog. Closes a long-standing
//! gap (GSettings keys had no GUI surface; users had to edit
//! via `gsettings` or wait for a custom built-in editor).
//! Three pages: General (mode, theme, vault path),
//! Capture (Quick Entry shortcut binding), Notifications
//! (master on/off — wires into the v0.20.0 reminder service).
//!
//! All preferences write through to GSettings — no separate
//! state. The window is a thin presentation layer; the live
//! GSettings keys remain the source of truth.
//!
//! Wired to `app.preferences` action; the primary menu's
//! "Preferences…" entry triggers it. AdwPreferencesWindow
//! handles its own present/close lifecycle.
//!
//! Phase 20 adds a Backups page when the backup-restore UI
//! lands; the scaffolding here is set up so adding pages is
//! a one-method addition.

use adw::prelude::*;
use atrium_core::APP_ID;
use gtk::gio;
use gtk::glib;
use gtk::glib::clone;

use crate::i18n::{gettext, gettext_f};

/// Open the preferences dialog anchored to `parent`. Presents
/// itself and returns immediately. Uses `AdwPreferencesDialog`
/// (libadwaita 1.6+) — the predecessor `AdwPreferencesWindow`
/// is deprecated.
pub fn open(parent: &impl IsA<gtk::Widget>) {
    let settings = gio::Settings::new(APP_ID);

    let dialog = adw::PreferencesDialog::builder()
        .title(gettext("Preferences"))
        .content_width(620)
        .content_height(520)
        .build();

    dialog.add(&general_page(&settings));
    dialog.add(&capture_page(&settings));
    dialog.add(&notifications_page(&settings));
    dialog.add(&backups_page(&settings));

    dialog.present(Some(parent));
}

fn general_page(settings: &gio::Settings) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(gettext("General"))
        .icon_name("preferences-system-symbolic")
        .build();

    // ── Mode ──────────────────────────────────────────────────
    let appearance_group = adw::PreferencesGroup::builder()
        .title(gettext("Appearance"))
        .build();

    // Translators: the two Atrium UI modes.
    let mode_simple = gettext("Simple");
    let mode_builder = gettext("Builder");
    let mode_model = gtk::StringList::new(&[mode_simple.as_str(), mode_builder.as_str()]);
    let mode_row = adw::ComboRow::builder()
        .title(gettext("Default mode"))
        .subtitle(gettext(
            "Simple is the calm Things-style surface; Builder adds Inspector pane, Forecast, Review.",
        ))
        .model(&mode_model)
        .selected(if settings.string("mode") == "builder" { 1 } else { 0 })
        .build();
    {
        let settings = settings.clone();
        mode_row.connect_selected_notify(move |row| {
            let value = if row.selected() == 1 {
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
    let theme_model = gtk::StringList::new(&[
        theme_follow.as_str(),
        theme_light.as_str(),
        theme_dark.as_str(),
    ]);
    let theme_row = adw::ComboRow::builder()
        .title(gettext("Theme"))
        .subtitle(gettext(
            "Override the system colour scheme. Adwaita auto-tracks the system; pin one here if you want it constant.",
        ))
        .model(&theme_model)
        .selected(theme_to_index(&settings.string("theme")))
        .build();
    {
        let settings = settings.clone();
        theme_row.connect_selected_notify(move |row| {
            let value = match row.selected() {
                1 => "light",
                2 => "dark",
                _ => "auto",
            };
            let _ = settings.set_string("theme", value);
            apply_theme(value);
        });
    }
    appearance_group.add(&theme_row);

    let high_legibility = adw::SwitchRow::builder()
        .title(gettext("High-legibility font"))
        // Translators: "Atkinson Hyperlegible" is a typeface name — keep it as-is.
        .subtitle(gettext(
            "Atkinson Hyperlegible — designed by the Braille Institute for low-vision readers.",
        ))
        .active(settings.boolean("high-legibility-font"))
        .build();
    {
        let settings = settings.clone();
        high_legibility.connect_active_notify(move |row| {
            let _ = settings.set_boolean("high-legibility-font", row.is_active());
        });
    }
    appearance_group.add(&high_legibility);

    page.add(&appearance_group);

    // ── Vault ─────────────────────────────────────────────────
    let vault_group = adw::PreferencesGroup::builder()
        .title(gettext("Org vault"))
        // Translators: `.org` and `~/Tasks/` are literal file-system names — keep them untranslated.
        .description(gettext(
            "Path to a directory holding `.org` files Atrium projects its data into. \
             Empty = no vault (DB-only). Convention is ~/Tasks/.",
        ))
        .build();

    let vault_entry = adw::EntryRow::builder()
        .title(gettext("Vault path"))
        .build();
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
    vault_entry.add_suffix(&pick_button);
    vault_group.add(&vault_entry);

    page.add(&vault_group);

    page
}

fn capture_page(settings: &gio::Settings) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(gettext("Capture"))
        .icon_name("input-keyboard-symbolic")
        .build();

    let group = adw::PreferencesGroup::builder()
        .title(gettext("Quick Entry"))
        // Translators: `<Control><Alt>space` is a literal GTK accelerator string — keep it untranslated.
        .description(gettext(
            "Global shortcut that opens the Quick Entry modal. GTK accelerator syntax \
             (e.g. `<Control><Alt>space`).",
        ))
        .build();

    let shortcut_row = adw::EntryRow::builder()
        .title(gettext("Shortcut"))
        .text(settings.string("quick-entry-shortcut"))
        .build();
    {
        let settings = settings.clone();
        shortcut_row.connect_changed(move |entry| {
            let _ = settings.set_string("quick-entry-shortcut", &entry.text());
        });
    }
    group.add(&shortcut_row);

    page.add(&group);
    page
}

fn notifications_page(settings: &gio::Settings) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(gettext("Notifications"))
        .icon_name("preferences-system-notifications-symbolic")
        .build();

    let group = adw::PreferencesGroup::builder()
        .title(gettext("Reminders"))
        // Translators: `reminder_at` is a literal field name — keep it untranslated.
        .description(gettext(
            "Time-based reminders fire as system notifications when this is on. \
             Per-task `reminder_at` timestamps drive the schedule.",
        ))
        .build();

    let enabled_row = adw::SwitchRow::builder()
        .title(gettext("Enable system notifications"))
        .active(settings.boolean("notifications-enabled"))
        .build();
    {
        let settings = settings.clone();
        enabled_row.connect_active_notify(move |row| {
            let _ = settings.set_boolean("notifications-enabled", row.is_active());
        });
    }
    group.add(&enabled_row);

    page.add(&group);
    page
}

/// v0.32.0 — Backups page. "Back up now" writes a `VACUUM INTO`
/// snapshot and prunes to the newest 10; "Restore from backup…"
/// queues a snapshot to be copied over the live DB on next launch;
/// the switch toggles the opportunistic weekly auto-backup GSetting.
fn backups_page(settings: &gio::Settings) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title(gettext("Backups"))
        .icon_name("document-save-symbolic")
        .build();

    let group = adw::PreferencesGroup::builder()
        .title(gettext("Database backups"))
        // Translators: `backups/` is a literal folder name — keep it untranslated.
        .description(gettext(
            "Snapshots live in the Atrium data directory's `backups/` folder \
             (the newest ten are kept).",
        ))
        .build();

    // Back up now.
    let backup_row = adw::ActionRow::builder()
        .title(gettext("Back up now"))
        .subtitle(gettext("Write a snapshot of the current database."))
        .build();
    let backup_btn = gtk::Button::builder()
        .label(gettext("Back up"))
        .valign(gtk::Align::Center)
        .build();
    backup_btn.add_css_class("flat");
    backup_row.add_suffix(&backup_btn);
    backup_btn.connect_clicked(clone!(
        #[weak]
        backup_row,
        move |_| {
            let dir = atrium_core::paths::backups_dir();
            match atrium_core::backup::backup_now(&atrium_core::db_path(), &dir) {
                Ok(path) => {
                    let _ = atrium_core::backup::prune(&dir, 10);
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    backup_row.set_subtitle(&gettext_f("Backed up to {name}", &[("name", &name)]));
                }
                Err(e) => backup_row.set_subtitle(&gettext_f(
                    "Backup failed: {error}",
                    &[("error", &e.to_string())],
                )),
            }
        }
    ));
    group.add(&backup_row);

    // Restore from backup.
    let restore_row = adw::ActionRow::builder()
        .title(gettext("Restore from backup…"))
        .subtitle(gettext("Replace the current database on the next launch."))
        .build();
    let restore_btn = gtk::Button::builder()
        .label(gettext("Restore…"))
        .valign(gtk::Align::Center)
        .build();
    restore_btn.add_css_class("flat");
    restore_row.add_suffix(&restore_btn);
    restore_btn.connect_clicked(clone!(
        #[weak]
        restore_row,
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
            let restore_row = restore_row.clone();
            dialog.open(window.as_ref(), gio::Cancellable::NONE, move |res| {
                if let Ok(file) = res
                    && let Some(path) = file.path()
                {
                    match std::fs::write(
                        atrium_core::paths::restore_marker_path(),
                        path.to_string_lossy().as_bytes(),
                    ) {
                        Ok(()) => restore_row
                            .set_subtitle(&gettext("Restore queued — restart Atrium to apply.")),
                        Err(e) => restore_row.set_subtitle(&gettext_f(
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
    let weekly_row = adw::SwitchRow::builder()
        .title(gettext("Weekly automatic backup"))
        .subtitle(gettext(
            "On launch, snapshot if the newest backup is over a week old.",
        ))
        .active(settings.boolean("backup-weekly"))
        .build();
    {
        let settings = settings.clone();
        weekly_row.connect_active_notify(move |row| {
            let _ = settings.set_boolean("backup-weekly", row.is_active());
        });
    }
    group.add(&weekly_row);

    page.add(&group);
    page
}

fn theme_to_index(value: &str) -> u32 {
    match value {
        "light" => 1,
        "dark" => 2,
        _ => 0,
    }
}

/// Apply a theme override to the Adwaita StyleManager. Called
/// when the user picks a theme in preferences; the application
/// boot path also calls this once with the persisted value.
pub fn apply_theme(value: &str) {
    let manager = adw::StyleManager::default();
    let scheme = match value {
        "light" => adw::ColorScheme::ForceLight,
        "dark" => adw::ColorScheme::ForceDark,
        _ => adw::ColorScheme::Default,
    };
    manager.set_color_scheme(scheme);
}
