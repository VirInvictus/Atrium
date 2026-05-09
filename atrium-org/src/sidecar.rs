// SPDX-License-Identifier: MIT
//! `<vault>/.atrium/config.toml` — the Atrium-only sidecar.
//!
//! Spec §7.3.1: tag colors and mode preference live in a sidecar
//! file under a hidden `.atrium/` directory at the vault root.
//! Other Org tools ignore the directory; Atrium regenerates the
//! file from DB state, so manual edits are overwritten.
//!
//! v0.10.1 ships **tag colors + mode** — the two fields that have
//! a defined home in the data layer today. Saved Perspectives are
//! reserved for a follow-up; perspective definitions cross more
//! boundaries (renderer config, column lists) and there's no
//! existing import path to round-trip them through. The schema
//! reserves `[perspectives]` as an empty section so Emacs-side
//! tools see the slot.
//!
//! ## Format
//!
//! ```toml
//! # Atrium vault sidecar.
//! # Regenerated automatically by Atrium — manual edits are
//! # overwritten on the next sync.
//!
//! mode = "simple"
//!
//! [tags]
//! errand = "#26a269"
//! work = "#3584e4"
//!
//! [perspectives]
//! # Reserved for future use.
//! ```
//!
//! ## Why hand-rolled
//!
//! No `toml` crate dependency — the dep ledger in CLAUDE.md +
//! `Cargo.toml` keeps the surface tight, and the schema we need
//! is small (top-level scalars + one level of `[section]` with
//! string-string entries). Same ethos as the hand-rolled Org
//! parser. If the schema grows past arrays / nested tables this
//! decision earns a re-discussion.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use atrium_core::error::DbError;
use rusqlite::Connection;

/// Parsed sidecar contents. `BTreeMap` so the emit order is
/// deterministic — round-tripping the file produces byte-stable
/// output, which keeps git diffs honest if a user commits their
/// vault.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sidecar {
    /// `"simple"` or `"builder"`. `None` lets the GUI's local
    /// GSettings value win — the sidecar doesn't override on
    /// import, just records.
    pub mode: Option<String>,
    /// Tag name → hex colour string (`"#RRGGBB"`).
    pub tag_colors: BTreeMap<String, String>,
}

impl Sidecar {
    /// Render to canonical TOML text. Empty maps emit the
    /// section header so external tools can see the placeholder.
    pub fn emit_text(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push_str("# Atrium vault sidecar.\n");
        out.push_str(
            "# Regenerated automatically by Atrium — manual edits are overwritten on the next sync.\n",
        );
        out.push('\n');

        if let Some(mode) = &self.mode {
            out.push_str(&format!("mode = {}\n\n", quote_string(mode)));
        }

        out.push_str("[tags]\n");
        for (name, color) in &self.tag_colors {
            out.push_str(&format!("{} = {}\n", quote_key(name), quote_string(color)));
        }
        out.push('\n');

        out.push_str("[perspectives]\n");
        out.push_str("# Reserved for future use.\n");
        out
    }

    /// Parse from TOML text. Tolerant: unknown sections / unknown
    /// top-level keys are dropped silently. Returns the default
    /// for genuinely malformed input (we'd rather surface a fresh
    /// sidecar than fail boot on a hand-edited typo).
    pub fn parse_text(text: &str) -> Self {
        let mut out = Sidecar::default();
        let mut current_section: Option<String> = None;

        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                current_section = Some(rest.trim().to_string());
                continue;
            }
            // key = value
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let key = unquote_key(k.trim());
            let value = unquote_string(v.trim());
            match current_section.as_deref() {
                None if key == "mode" => {
                    out.mode = Some(value);
                }
                Some("tags") => {
                    out.tag_colors.insert(key, value);
                }
                _ => {}
            }
        }
        out
    }
}

/// Path of the sidecar file inside a vault. The parent
/// `.atrium/` directory is created on first write.
pub fn sidecar_path(vault_root: &Path) -> PathBuf {
    vault_root.join(".atrium").join("config.toml")
}

/// Build the sidecar from a read-only DB connection. The mode
/// field is left `None` here — mode lives in GSettings, not the
/// SQL schema, and only the GTK binary knows it. The `tag_colors`
/// map is populated from `tag.color` for every tag with a colour
/// set.
pub fn build_from_db(conn: &Connection) -> Result<Sidecar, DbError> {
    let tags = atrium_core::db::read::list_tags(conn)?;
    let mut tag_colors = BTreeMap::new();
    for tag in tags {
        if let Some(color) = tag.color {
            tag_colors.insert(tag.name, color);
        }
    }
    Ok(Sidecar {
        mode: None,
        tag_colors,
    })
}

/// Read the sidecar from disk. Returns `Sidecar::default()` when
/// the file is absent — most users won't have provisioned one
/// before the first vault write.
pub fn read_sidecar(vault_root: &Path) -> io::Result<Sidecar> {
    let path = sidecar_path(vault_root);
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(Sidecar::parse_text(&text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Sidecar::default()),
        Err(e) => Err(e),
    }
}

/// Write the sidecar atomically. Creates `<vault>/.atrium/` if
/// absent. Goes through [`atrium_core::sync::atomic::write_atomic`]
/// so a crash mid-write leaves the previous file intact (or no
/// file at all — never a half-written one).
pub fn write_sidecar(vault_root: &Path, sidecar: &Sidecar) -> io::Result<()> {
    let path = sidecar_path(vault_root);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = sidecar.emit_text();
    atrium_core::sync::atomic::write_atomic(&path, text.as_bytes())
}

// ── TOML quoting helpers (minimal subset) ────────────────────

/// Always emit values as basic strings (`"…"`). Escapes backslash
/// and double-quote per TOML basic-string rules; no need for
/// multiline or literal forms with our schema.
fn quote_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Bare keys (alphanumeric + `_`/`-`) emit unquoted; everything
/// else gets quoted like a value.
fn quote_key(k: &str) -> String {
    if !k.is_empty()
        && k.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        k.to_string()
    } else {
        quote_string(k)
    }
}

fn unquote_string(s: &str) -> String {
    if let Some(stripped) = s.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        let mut out = String::with_capacity(stripped.len());
        let mut chars = stripped.chars();
        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some(other) => out.push(other),
                    None => break,
                }
            } else {
                out.push(ch);
            }
        }
        out
    } else {
        s.to_string()
    }
}

fn unquote_key(k: &str) -> String {
    if let Some(stripped) = k.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        unquote_string(&format!("\"{stripped}\""))
    } else {
        k.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn populated() -> Sidecar {
        let mut tag_colors = BTreeMap::new();
        tag_colors.insert("errand".into(), "#26a269".into());
        tag_colors.insert("work".into(), "#3584e4".into());
        Sidecar {
            mode: Some("builder".to_string()),
            tag_colors,
        }
    }

    #[test]
    fn round_trip_through_text() {
        let original = populated();
        let text = original.emit_text();
        let parsed = Sidecar::parse_text(&text);
        assert_eq!(parsed, original);
    }

    #[test]
    fn empty_sidecar_emits_section_headers() {
        let s = Sidecar::default();
        let text = s.emit_text();
        assert!(text.contains("[tags]"));
        assert!(text.contains("[perspectives]"));
        // Mode is absent → no `mode = ` line.
        assert!(!text.contains("mode ="));
    }

    #[test]
    fn parse_ignores_unknown_sections() {
        let text = "\
[unknown]
something = \"ignored\"

[tags]
work = \"#3584e4\"
";
        let parsed = Sidecar::parse_text(text);
        assert_eq!(
            parsed.tag_colors.get("work").map(String::as_str),
            Some("#3584e4")
        );
        assert!(parsed.mode.is_none());
    }

    #[test]
    fn parse_tolerates_blank_lines_and_comments() {
        let text = "\
# top of file
mode = \"simple\"

# tag colours below
[tags]
work = \"#3584e4\"
";
        let parsed = Sidecar::parse_text(text);
        assert_eq!(parsed.mode.as_deref(), Some("simple"));
        assert_eq!(
            parsed.tag_colors.get("work").map(String::as_str),
            Some("#3584e4")
        );
    }

    #[test]
    fn quoted_string_round_trips_special_characters() {
        let value = "hello \"world\" \\ end";
        let quoted = quote_string(value);
        let unquoted = unquote_string(&quoted);
        assert_eq!(unquoted, value);
    }

    #[test]
    fn key_with_dashes_emits_unquoted() {
        let key = "high-priority";
        assert_eq!(quote_key(key), key);
    }

    #[test]
    fn key_with_space_gets_quoted() {
        let key = "two words";
        let quoted = quote_key(key);
        assert!(quoted.starts_with('"') && quoted.ends_with('"'));
        assert_eq!(unquote_key(&quoted), key);
    }

    #[test]
    fn write_then_read_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("atrium-sidecar-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let original = populated();
        write_sidecar(&dir, &original).unwrap();
        let read_back = read_sidecar(&dir).unwrap();
        assert_eq!(read_back, original);
        // The .atrium/config.toml lives where the spec says.
        assert!(dir.join(".atrium").join("config.toml").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_returns_default_when_absent() {
        let dir =
            std::env::temp_dir().join(format!("atrium-sidecar-absent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let s = read_sidecar(&dir).unwrap();
        assert_eq!(s, Sidecar::default());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
