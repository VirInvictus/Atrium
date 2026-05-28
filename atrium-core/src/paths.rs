// SPDX-License-Identifier: MIT
//! XDG-compliant path helpers and the application identifier.
//!
//! Stdlib-only — no `directories` / `xdg` crate. `XDG_DATA_HOME` and
//! `XDG_CACHE_HOME` are honoured per the freedesktop.org Base Directory
//! Specification, falling back to `$HOME/.local/share` and `$HOME/.cache`.

use std::path::PathBuf;

/// Reverse-DNS application identifier. Must match the desktop entry,
/// GSettings schema, AppStream metainfo, and Flatpak manifest.
pub const APP_ID: &str = "io.github.virinvictus.atrium";

/// `$XDG_DATA_HOME/atrium/` — task database and durable user data.
pub fn data_dir() -> PathBuf {
    xdg_dir("XDG_DATA_HOME", ".local/share").join("atrium")
}

/// `$XDG_CACHE_HOME/atrium/` — disposable caches; safe to delete.
pub fn cache_dir() -> PathBuf {
    xdg_dir("XDG_CACHE_HOME", ".cache").join("atrium")
}

/// Canonical SQLite database path — `$XDG_DATA_HOME/atrium/atrium.db`.
pub fn db_path() -> PathBuf {
    data_dir().join("atrium.db")
}

/// `$XDG_DATA_HOME/atrium/backups/` — timestamped database snapshots
/// (v0.32.0). Durable user data, not a disposable cache.
pub fn backups_dir() -> PathBuf {
    data_dir().join("backups")
}

/// Marker file (`$XDG_DATA_HOME/atrium/.restore-pending`) holding the
/// path of a backup to restore on the next launch. The GUI writes it
/// from the restore picker; `boot_data_layer` consumes it before the
/// DB opens. v0.32.0.
pub fn restore_marker_path() -> PathBuf {
    data_dir().join(".restore-pending")
}

fn xdg_dir(env_var: &str, home_relative_fallback: &str) -> PathBuf {
    if let Ok(v) = std::env::var(env_var)
        && !v.is_empty()
    {
        return PathBuf::from(v);
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| String::from("/"));
    PathBuf::from(home).join(home_relative_fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_path_lives_under_atrium_data_dir() {
        // Race-safe structural check: the two test cases that mutate
        // XDG_DATA_HOME run in parallel with this one, so a direct
        // comparison against `data_dir()` is racy. Verify the shape
        // (`<…>/atrium/atrium.db`) instead — that's the contract.
        let db = db_path();
        assert_eq!(db.file_name().and_then(|s| s.to_str()), Some("atrium.db"));
        assert_eq!(
            db.parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str()),
            Some("atrium")
        );
    }

    #[test]
    fn app_id_is_reverse_dns() {
        assert!(APP_ID.contains('.'));
        assert_eq!(APP_ID, "io.github.virinvictus.atrium");
    }

    #[test]
    fn xdg_data_home_is_honoured() {
        // Regression guard: when XDG_DATA_HOME is set, data_dir() must
        // honour it rather than falling back to $HOME/.local/share.
        let original = std::env::var("XDG_DATA_HOME").ok();
        // SAFETY: tests in the same module run on the same thread by
        // default; std::env::set_var is safe here. If parallel test
        // execution becomes an issue, this guard isolates via a
        // sentinel path that's still distinguishable from the fallback.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", "/tmp/atrium-test-xdg");
        }
        let dir = data_dir();
        assert_eq!(dir, PathBuf::from("/tmp/atrium-test-xdg/atrium"));
        unsafe {
            match original {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
        }
    }
}
