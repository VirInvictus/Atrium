// SPDX-License-Identifier: MIT
//! Typography foundation (spec §3 / roadmap Phase 3).
//!
//! Bundles three font families — Inter Variable (UI), Source Serif 4
//! Variable (note bodies), JetBrains Mono Variable (debug pane and
//! monospace surfaces) — all SIL OFL 1.1, all shipped from
//! `data/fonts/`.
//!
//! Installation strategy mirrors the proven pattern from sibling
//! Viaduct: copy the TTFs to `$XDG_DATA_HOME/fonts/atrium/` on first
//! run and refresh `fc-cache`. Fontconfig handles registration after
//! that, so the typography is identical across native and Flatpak
//! installs without per-process Pango plumbing. Idempotent — files
//! aren't recopied if they're already present.
//!
//! If a source file is missing at runtime (e.g., a development clone
//! where `data/fonts/` hasn't been populated), a warning is logged and
//! the application falls back to whatever the system has installed.

use std::path::{Path, PathBuf};
use std::process::Command;

use gtk::gdk;
use tracing::{debug, info, warn};

const BUNDLED_FONT_FILES: &[&str] = &[
    "InterVariable.ttf",
    "InterVariable-Italic.ttf",
    "SourceSerif4Variable-Roman.ttf",
    "SourceSerif4Variable-Italic.ttf",
    "JetBrainsMono-Variable.ttf",
    "JetBrainsMono-Variable-Italic.ttf",
];

/// Copy the bundled TTFs into the user fonts directory and refresh
/// fontconfig. No-op if every font is already installed.
pub fn install_bundled_fonts() -> usize {
    let Some(source_dir) = font_source_dir() else {
        warn!("no font source directory resolved; skipping font install");
        return 0;
    };
    let Some(target_dir) = user_fonts_dir() else {
        warn!("could not determine user fonts directory; skipping font install");
        return 0;
    };

    if let Err(e) = std::fs::create_dir_all(&target_dir) {
        warn!(error = %e, dir = %target_dir.display(), "could not create user fonts dir");
        return 0;
    }

    let mut copied = 0;
    let mut already = 0;
    let mut missing = 0;

    for filename in BUNDLED_FONT_FILES {
        let src = source_dir.join(filename);
        let dst = target_dir.join(filename);
        if !src.exists() {
            warn!(path = %src.display(), "bundled font missing in data/fonts/");
            missing += 1;
            continue;
        }
        if dst.exists() {
            already += 1;
            continue;
        }
        match std::fs::copy(&src, &dst) {
            Ok(_) => {
                copied += 1;
                debug!(path = %dst.display(), "installed bundled font");
            }
            Err(e) => warn!(error = %e, src = %src.display(), "failed to copy font"),
        }
    }

    if copied > 0 {
        info!(
            dir = %target_dir.display(),
            copied,
            already_present = already,
            missing,
            "refreshing font cache"
        );
        let result = Command::new("fc-cache").arg("-f").arg(&target_dir).status();
        match result {
            Ok(s) if s.success() => debug!("fc-cache OK"),
            Ok(s) => warn!(status = ?s, "fc-cache exited non-zero"),
            Err(e) => warn!(error = %e, "fc-cache failed to run"),
        }
    } else {
        info!(
            dir = %target_dir.display(),
            already_present = already,
            missing,
            "bundled fonts already installed"
        );
    }

    copied + already
}

/// `$XDG_DATA_HOME/fonts/atrium/` per the Linux user-fonts convention.
fn user_fonts_dir() -> Option<PathBuf> {
    let base = if let Ok(v) = std::env::var("XDG_DATA_HOME") {
        if !v.is_empty() {
            PathBuf::from(v)
        } else {
            home_dir()?.join(".local/share")
        }
    } else {
        home_dir()?.join(".local/share")
    };
    Some(base.join("fonts").join("atrium"))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// Resolve the directory holding the bundled TTFs at runtime.
fn font_source_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("ATRIUM_FONT_DIR") {
        return Some(PathBuf::from(d));
    }
    if let Some(d) = compile_time_datadir() {
        let p = d.join("fonts");
        if p.exists() {
            return Some(p);
        }
    }
    if let Some(d) = exe_relative_share_fonts()
        && d.exists()
    {
        return Some(d);
    }
    None
}

fn compile_time_datadir() -> Option<PathBuf> {
    option_env!("ATRIUM_DATADIR").map(PathBuf::from)
}

fn exe_relative_share_fonts() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let parent = exe.parent()?;
    Some(parent.join("..").join("share").join("atrium").join("fonts"))
}

/// Load and apply the bundled stylesheet (`data/style.css`).
pub fn apply_bundled_stylesheet() {
    let Some(path) = stylesheet_path() else {
        warn!("could not resolve stylesheet path; using default GTK theme");
        return;
    };
    if !path.exists() {
        warn!(path = %path.display(), "bundled stylesheet missing; using default GTK theme");
        return;
    }

    let provider = gtk::CssProvider::new();
    provider.load_from_path(&path);

    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        info!(path = %path.display(), "stylesheet applied");
    } else {
        warn!("no GDK display available; stylesheet not applied");
    }
}

fn stylesheet_path() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("ATRIUM_DATADIR") {
        return Some(Path::new(&d).join("style.css"));
    }
    if let Some(d) = compile_time_datadir() {
        return Some(d.join("style.css"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_filenames_are_six() {
        assert_eq!(BUNDLED_FONT_FILES.len(), 6);
    }

    #[test]
    fn font_source_dir_resolves_when_env_set() {
        // SAFETY: this is the only test mutating ATRIUM_FONT_DIR.
        unsafe {
            std::env::set_var("ATRIUM_FONT_DIR", "/tmp/atrium-fonts-test");
        }
        let dir = font_source_dir();
        assert_eq!(dir, Some(PathBuf::from("/tmp/atrium-fonts-test")));
        unsafe {
            std::env::remove_var("ATRIUM_FONT_DIR");
        }
    }

    #[test]
    fn user_fonts_dir_lands_under_xdg_data_home() {
        let dir = user_fonts_dir().expect("HOME or XDG_DATA_HOME should resolve");
        assert!(dir.ends_with("fonts/atrium"));
    }
}
