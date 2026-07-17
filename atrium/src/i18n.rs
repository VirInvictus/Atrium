// SPDX-License-Identifier: MIT
//! Localisation runtime (Phase 20, v0.47.0).
//!
//! Thin wiring over glibc's gettext via the `gettextrs` crate. The
//! binary binds the `atrium` text domain at startup; every user-facing
//! string in the GTK layer routes through the re-exports or the `_f`
//! helpers below. Library crates and `atrium-cli` stay untranslated by
//! design — their output is script-parseable contract surface.
//!
//! With no `.mo` installed for the current locale, `gettext` returns
//! the msgid unchanged, so untranslated locales (and `cargo test`,
//! which never calls [`init`]) see the English source strings.

use std::path::PathBuf;

use tracing::warn;

/// The gettext text domain. Must match the meson `gettext_package`
/// (`i18n.gettext('atrium', ...)`) and the `domain="atrium"` attribute
/// on the GtkBuilder `.ui` interfaces.
pub const GETTEXT_DOMAIN: &str = "atrium";

pub use gettextrs::{gettext, ngettext, pgettext};

/// Bind the text domain. Call once at startup, before any GTK type is
/// created — GtkBuilder resolves `translatable="yes"` strings through
/// this domain when it inflates templates.
pub fn init() {
    if gettextrs::setlocale(gettextrs::LocaleCategory::LcAll, "").is_none() {
        warn!("setlocale rejected the environment's locale; staying on C locale");
    }
    if let Err(e) = gettextrs::bindtextdomain(GETTEXT_DOMAIN, localedir()) {
        warn!(error = %e, "bindtextdomain failed; UI stays untranslated");
    }
    if let Err(e) = gettextrs::bind_textdomain_codeset(GETTEXT_DOMAIN, "UTF-8") {
        warn!(error = %e, "bind_textdomain_codeset failed");
    }
    if let Err(e) = gettextrs::textdomain(GETTEXT_DOMAIN) {
        warn!(error = %e, "textdomain failed; UI stays untranslated");
    }
}

/// Locale directory resolution, mirroring the `typography.rs` chain:
/// runtime env override → compile-time default baked by `build.rs`
/// (meson exports the install-time path; plain cargo builds bake the
/// system location) → system location.
fn localedir() -> PathBuf {
    if let Ok(d) = std::env::var("ATRIUM_LOCALEDIR") {
        return PathBuf::from(d);
    }
    if let Some(d) = option_env!("ATRIUM_LOCALEDIR") {
        return PathBuf::from(d);
    }
    PathBuf::from("/usr/share/locale")
}

/// Translate `msgid`, then substitute `{name}` placeholders.
///
/// Translated strings can't go through `format!` (it needs a literal
/// format string), so interpolated messages use named placeholders the
/// translator can reorder: `gettext_f("Moved to {name}", &[("name",
/// project_name)])`.
pub fn gettext_f(msgid: &str, args: &[(&str, &str)]) -> String {
    freplace(gettext(msgid), args)
}

/// Plural-aware [`gettext_f`]. `n` picks the plural form; placeholders
/// (conventionally including `{n}` itself) substitute afterwards.
pub fn ngettext_f(msgid: &str, msgid_plural: &str, n: u32, args: &[(&str, &str)]) -> String {
    freplace(ngettext(msgid, msgid_plural, n), args)
}

fn freplace(mut s: String, args: &[(&str, &str)]) -> String {
    for (key, value) in args {
        s = s.replace(&format!("{{{key}}}"), value);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::freplace;

    #[test]
    fn freplace_substitutes_named_placeholders() {
        let out = freplace("Moved to {name}".to_string(), &[("name", "Errands")]);
        assert_eq!(out, "Moved to Errands");
    }

    #[test]
    fn freplace_handles_multiple_and_repeated_keys() {
        let out = freplace("{a} and {b} and {a}".to_string(), &[("a", "1"), ("b", "2")]);
        assert_eq!(out, "1 and 2 and 1");
    }

    #[test]
    fn freplace_leaves_unknown_placeholders_verbatim() {
        let out = freplace("{n} tasks".to_string(), &[("m", "3")]);
        assert_eq!(out, "{n} tasks");
    }

    #[test]
    fn freplace_no_args_is_identity() {
        let out = freplace("Plain title".to_string(), &[]);
        assert_eq!(out, "Plain title");
    }
}
