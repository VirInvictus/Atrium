// SPDX-License-Identifier: MIT
//! Dark/light resolution over `org.freedesktop.portal.Settings`.
//!
//! Phase 22's design bullet asked for "dark/light via a direct
//! `org.freedesktop.portal.Settings` read (gio D-Bus, no new dependency)"
//! in place of `adw::StyleManager`. C10 dropped the toolkit but never
//! landed the replacement read, so `apply_theme` was left setting nothing
//! but GtkSettings' prefer-dark hint from the stored string. The effect:
//! `AdwStyleManager`'s `Default` scheme used to track
//! `org.freedesktop.appearance.color-scheme` through the portal, and
//! without it the `"auto"` GSettings default stopped following the system
//! entirely and became a synonym for dark.
//!
//! That is cosmetically invisible today because Atrium ships only the dark
//! Kanagawa Dragon sheet (`theme.rs`), which is exactly why it was easy to
//! miss. It still matters: it is a documented intent that did not land, and
//! it gates the post-1.0 light Lotus palette, which cannot follow the
//! desktop until something is actually reading the desktop.
//!
//! Ported in shape from Viaduct's `theme.rs` (the sibling that solved this
//! first, Phase 20b), not depended on: same synchronous-first-read,
//! same `Read` fallback, same preference folding. Two deliberate
//! divergences, both because Atrium is a dark-only app today:
//!
//! * **The no-answer default is dark, not light.** Viaduct resolves light
//!   when no portal answers, matching what libadwaita did. Atrium has no
//!   light sheet to fall back to, so degrading to light would paint dark
//!   chrome with a light hint. The roadmap item says so outright: degrade
//!   to dark rather than failing.
//! * **No listener registry, and no public `is_dark`.** Viaduct has widgets
//!   to re-render on a flip (its article pane re-renders themed HTML) and an
//!   accessor they read. Atrium's only consumer is the GtkSettings hint, so
//!   the resolved value is applied directly and nothing needs to ask. Both
//!   the listener list and a read accessor are what the Lotus palette will
//!   want; they belong to that change, against real consumers, rather than
//!   sitting here unused.
//!
//! **`re_resolve` is the single funnel, and that is the point.** The
//! resolved value is the system preference *folded with* the user's `theme`
//! GSetting, so a user who has pinned Dark keeps it across a system flip to
//! light. Reading the portal alone would lose the pin; reading the setting
//! alone is the bug this module fixes.

use gtk::gio;
use gtk::glib;
use gtk::prelude::*;
use std::cell::{Cell, RefCell};

/// Matches what `AdwStyleManager` did with the portal's `color-scheme`:
/// 0 is "no preference", 1 is "prefer dark", 2 is "prefer light". Only 1
/// means dark, and an unknown future value must not read as dark.
fn portal_scheme_is_dark(scheme: u32) -> bool {
    scheme == 1
}

thread_local! {
    /// Whether this module has already wired itself up. The app can be
    /// activated more than once, and a second D-Bus subscription would
    /// double every flip.
    static INITIALIZED: Cell<bool> = const { Cell::new(false) };
    /// The desktop's own preference. Dark until the portal says otherwise:
    /// Atrium has only a dark sheet, so dark is the safe degrade.
    static SYSTEM_DARK: Cell<bool> = const { Cell::new(true) };
    /// The last value handed to GtkSettings, so a no-op flip does not
    /// churn the hint.
    static RESOLVED_DARK: Cell<bool> = const { Cell::new(true) };
    /// Held so the `theme` preference can be re-read whenever the portal
    /// flips.
    static SETTINGS: RefCell<Option<gio::Settings>> = const { RefCell::new(None) };
    /// Held so the `SettingChanged` subscription outlives `init`. Dropping
    /// the connection would silently stop live switching.
    static BUS: RefCell<Option<gio::DBusConnection>> = const { RefCell::new(None) };
}

/// Re-run resolution on demand. The Preferences dropdown calls this through
/// `preferences::apply_theme` after writing the key. The key subscription in
/// [`init`] would catch that write on its own; this exists so the dropdown's
/// call site still reads as "apply the theme now" rather than relying on a
/// side effect two modules away.
pub fn resolve_now() {
    re_resolve();
}

/// Fold the user's `theme` preference over the system value, and push the
/// result to GtkSettings if it changed.
///
/// Called from three places that all mean "the answer may be different
/// now": the portal flipped, the user picked a theme, or we just booted.
fn re_resolve() {
    let preference = SETTINGS.with(|s| {
        s.borrow()
            .as_ref()
            .map(|s| s.string("theme").to_string())
            .unwrap_or_else(|| "auto".to_string())
    });
    let dark = match preference.as_str() {
        "dark" => true,
        "light" => false,
        // "auto", and anything unrecognised, follows the desktop.
        _ => SYSTEM_DARK.with(Cell::get),
    };
    if RESOLVED_DARK.with(Cell::get) == dark && INITIALIZED.with(Cell::get) {
        return;
    }
    RESOLVED_DARK.with(|d| d.set(dark));
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(dark);
    }
}

/// Read the desktop's dark preference and keep following it. The first read
/// is synchronous and happens before the first frame, so the app never
/// paints one polarity and then corrects itself.
///
/// Every failure path leaves the dark default in place rather than
/// erroring: no session bus and no portal backend are both normal states on
/// a bare session, not faults.
///
/// `settings` is `None` only if a caller has no schema to hand; the module
/// then follows the desktop with no preference to fold.
pub fn init(settings: Option<gio::Settings>) {
    if INITIALIZED.with(Cell::get) {
        return;
    }

    if let Some(settings) = settings {
        // The preference is half of the resolved value, so a change to it
        // is as much a "dark changed" event as a system flip is. This also
        // covers the Preferences dropdown, which writes the key.
        settings.connect_changed(Some("theme"), |_, _| re_resolve());
        SETTINGS.with(|s| *s.borrow_mut() = Some(settings));
    }

    match gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE) {
        Ok(conn) => {
            if let Some(scheme) = read_portal_scheme(&conn) {
                SYSTEM_DARK.with(|d| d.set(portal_scheme_is_dark(scheme)));
            } else {
                tracing::debug!(
                    "settings portal did not answer; dark/light stays at the dark default"
                );
            }

            conn.signal_subscribe(
                Some("org.freedesktop.portal.Desktop"),
                Some("org.freedesktop.portal.Settings"),
                Some("SettingChanged"),
                Some("/org/freedesktop/portal/desktop"),
                None,
                gio::DBusSignalFlags::NONE,
                |_, _, _, _, _, params| {
                    let ns = params.child_value(0).get::<String>();
                    let key = params.child_value(1).get::<String>();
                    if ns.as_deref() != Some("org.freedesktop.appearance")
                        || key.as_deref() != Some("color-scheme")
                    {
                        return;
                    }
                    let Some(dark) = params
                        .child_value(2)
                        .as_variant()
                        .and_then(|v| v.get::<u32>())
                        .map(portal_scheme_is_dark)
                    else {
                        return;
                    };
                    SYSTEM_DARK.with(|d| d.set(dark));
                    // Through re_resolve, not straight to GtkSettings: a
                    // pinned theme must hold its polarity across a system
                    // flip.
                    re_resolve();
                },
            );
            BUS.with(|b| *b.borrow_mut() = Some(conn));
        }
        Err(_) => {
            tracing::debug!("no session bus; dark/light stays at the dark default");
        }
    }

    // Seed the hint from the first read, then mark initialised so the
    // dedupe in `re_resolve` starts biting.
    re_resolve();
    INITIALIZED.with(|i| i.set(true));
}

/// Ask the settings portal for the current colour scheme. `None` on any
/// failure, and the caller keeps its default.
fn read_portal_scheme(conn: &gio::DBusConnection) -> Option<u32> {
    let args = ("org.freedesktop.appearance", "color-scheme").to_variant();
    let reply_ty = glib::VariantTy::new("(v)").ok()?;
    let call = |method: &str| {
        conn.call_sync(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
            method,
            Some(&args),
            Some(reply_ty),
            gio::DBusCallFlags::NONE,
            1000,
            gio::Cancellable::NONE,
        )
    };
    match call("ReadOne") {
        Ok(reply) => reply.child_value(0).as_variant()?.get::<u32>(),
        // Portals predating ReadOne answer Read, which wraps the value in
        // a second layer of variant.
        Err(_) => call("Read")
            .ok()?
            .child_value(0)
            .as_variant()?
            .as_variant()?
            .get::<u32>(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_scheme_maps_like_adwaita() {
        // 0 = no preference, 1 = prefer dark, 2 = prefer light.
        assert!(portal_scheme_is_dark(1));
        assert!(!portal_scheme_is_dark(0));
        assert!(!portal_scheme_is_dark(2));
        // An unknown future value must not read as dark.
        assert!(!portal_scheme_is_dark(99));
    }

    #[test]
    fn degrades_to_dark_before_any_portal_answer() {
        // The roadmap item requires degrading to dark rather than failing
        // when no backend answers, because Atrium has no light sheet to
        // fall back to. This runs on a fresh thread_local set, i.e. exactly
        // the state a session with no portal leaves us in.
        assert!(SYSTEM_DARK.with(Cell::get));
        assert!(RESOLVED_DARK.with(Cell::get));
    }
}
