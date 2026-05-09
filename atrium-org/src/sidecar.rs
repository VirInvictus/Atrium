// SPDX-License-Identifier: MIT
//! `<vault>/.atrium/config.toml` — the Atrium-only sidecar.
//!
//! Spec §7.3.1: tag colors, mode preference, and saved Perspectives
//! live in a sidecar file under a hidden `.atrium/` directory at
//! the vault root. Other Org tools ignore the directory; Atrium
//! regenerates the file from DB state, so manual edits are
//! overwritten.
//!
//! v0.10.1 shipped **tag colors + mode**; v0.13.x adds **saved
//! Perspectives** — the third leg of spec §7.3.1's slot list now
//! that the schema is mature enough that perspective definitions
//! round-trip cleanly. Perspectives use TOML's array-of-tables
//! shape (`[[perspectives]]`) so each entry carries its own block
//! of fields (name / icon / filter / renderer / renderer_config).
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
//! [[perspectives]]
//! name = "Today's work"
//! filter = "tag:work AND is:today"
//! icon = "starred-symbolic"
//! renderer = "list"
//!
//! [[perspectives]]
//! name = "Q3 board"
//! filter = "project:\"Q3 plans\""
//! renderer = "board"
//! renderer_config = "{\"axis\":\"tag\",\"columns\":[\"todo\",\"doing\",\"done\"]}"
//! ```
//!
//! ## Why hand-rolled
//!
//! No `toml` crate dependency — the dep ledger in CLAUDE.md +
//! `Cargo.toml` keeps the surface tight, and the schema we need
//! is small (top-level scalars + one level of `[section]` with
//! string-string entries + one level of `[[array]]` with
//! string-string entries per element). Same ethos as the
//! hand-rolled Org parser. If the schema grows past arrays /
//! nested tables this decision earns a re-discussion.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use atrium_core::error::DbError;
use rusqlite::Connection;

/// Parsed sidecar contents. `BTreeMap` so the emit order for
/// scalar maps is deterministic — round-tripping the file
/// produces byte-stable output, which keeps git diffs honest if
/// a user commits their vault. Perspectives use a `Vec` because
/// the user-set order matters (sidebar ordering survives the
/// round-trip).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sidecar {
    /// `"simple"` or `"builder"`. `None` lets the GUI's local
    /// GSettings value win — the sidecar doesn't override on
    /// import, just records.
    pub mode: Option<String>,
    /// Tag name → hex colour string (`"#RRGGBB"`).
    pub tag_colors: BTreeMap<String, String>,
    /// Saved Perspectives in display order. Position lives only
    /// in the file — re-import assigns fresh positions matching
    /// source-file order, so a user reordering the sidebar in
    /// Atrium and re-emitting the sidecar will see the new order
    /// here on the next round-trip.
    pub perspectives: Vec<PerspectiveEntry>,
}

/// One row of the `[[perspectives]]` array. Mirrors the subset
/// of [`atrium_core::domain::Perspective`] that's user-defined
/// (skipping id / uuid / created_at / modified_at — those are
/// DB-generated and would only confuse a hand-edit).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PerspectiveEntry {
    pub name: String,
    pub filter: String,
    /// `None` when the perspective uses the default icon.
    pub icon: Option<String>,
    /// `"list"` or `"board"`. Defaulted to `"list"` on a missing
    /// or empty value during parse.
    pub renderer: String,
    /// JSON config — opaque to the sidecar, parsed by the
    /// renderer. `None` for `"list"`.
    pub renderer_config: Option<String>,
}

impl Sidecar {
    /// Render to canonical TOML text. Empty maps emit the
    /// section header so external tools can see the placeholder.
    /// Perspectives emit as TOML array-of-tables (`[[perspectives]]`),
    /// one block per entry, in `Vec` order.
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

        if self.perspectives.is_empty() {
            // Reserve the slot so an Emacs-side power user editing
            // the sidecar by hand sees the section is intentional,
            // matching the v0.10.1-era placeholder shape.
            out.push_str("# [[perspectives]]\n");
            out.push_str("# name = \"Example\"\n");
            out.push_str("# filter = \"tag:work\"\n");
        } else {
            for (i, p) in self.perspectives.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                out.push_str("[[perspectives]]\n");
                out.push_str(&format!("name = {}\n", quote_string(&p.name)));
                out.push_str(&format!("filter = {}\n", quote_string(&p.filter)));
                if let Some(icon) = &p.icon {
                    out.push_str(&format!("icon = {}\n", quote_string(icon)));
                }
                out.push_str(&format!("renderer = {}\n", quote_string(&p.renderer)));
                if let Some(cfg) = &p.renderer_config {
                    out.push_str(&format!("renderer_config = {}\n", quote_string(cfg)));
                }
            }
        }
        out
    }

    /// Parse from TOML text. Tolerant: unknown sections / unknown
    /// top-level keys are dropped silently. Returns the default
    /// for genuinely malformed input (we'd rather surface a fresh
    /// sidecar than fail boot on a hand-edited typo).
    pub fn parse_text(text: &str) -> Self {
        /// Where the current key/value pair binds.
        enum Cursor {
            /// Top-level — accepts `mode`.
            Toplevel,
            /// Inside `[tags]`.
            Tags,
            /// Inside the most recent `[[perspectives]]` block —
            /// the index points at `out.perspectives[idx]`.
            Perspective(usize),
            /// Some other named section we don't care about.
            Unknown,
        }

        let mut out = Sidecar::default();
        let mut cursor = Cursor::Toplevel;

        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            // Array of tables — `[[name]]`. Try this before the
            // single-bracket form so `[[perspectives]]` doesn't
            // accidentally match the `[name]` arm with `name` =
            // `[perspectives`.
            if let Some(rest) = line.strip_prefix("[[").and_then(|s| s.strip_suffix("]]")) {
                let name = rest.trim();
                if name == "perspectives" {
                    out.perspectives.push(PerspectiveEntry {
                        // `renderer` defaults to "list" so a
                        // perspective entry that omits the field
                        // still parses cleanly.
                        renderer: "list".to_string(),
                        ..PerspectiveEntry::default()
                    });
                    cursor = Cursor::Perspective(out.perspectives.len() - 1);
                } else {
                    cursor = Cursor::Unknown;
                }
                continue;
            }
            if let Some(rest) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                cursor = match rest.trim() {
                    "tags" => Cursor::Tags,
                    _ => Cursor::Unknown,
                };
                continue;
            }
            // key = value
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            let key = unquote_key(k.trim());
            let value = unquote_string(v.trim());
            match &cursor {
                Cursor::Toplevel if key == "mode" => {
                    out.mode = Some(value);
                }
                Cursor::Tags => {
                    out.tag_colors.insert(key, value);
                }
                Cursor::Perspective(idx) => {
                    let entry = &mut out.perspectives[*idx];
                    match key.as_str() {
                        "name" => entry.name = value,
                        "filter" => entry.filter = value,
                        "icon" => entry.icon = Some(value),
                        "renderer" if !value.is_empty() => entry.renderer = value,
                        "renderer_config" => entry.renderer_config = Some(value),
                        _ => {}
                    }
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
/// set; the `perspectives` Vec is populated from
/// `read::list_perspectives` in stored position order.
pub fn build_from_db(conn: &Connection) -> Result<Sidecar, DbError> {
    let tags = atrium_core::db::read::list_tags(conn)?;
    let mut tag_colors = BTreeMap::new();
    for tag in tags {
        if let Some(color) = tag.color {
            tag_colors.insert(tag.name, color);
        }
    }

    let perspectives_raw = atrium_core::db::read::list_perspectives(conn)?;
    let perspectives = perspectives_raw
        .into_iter()
        .map(|p| PerspectiveEntry {
            name: p.name,
            filter: p.filter_expr,
            icon: p.icon,
            renderer: p.renderer,
            renderer_config: p.renderer_config,
        })
        .collect();

    Ok(Sidecar {
        mode: None,
        tag_colors,
        perspectives,
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
            perspectives: Vec::new(),
        }
    }

    fn populated_with_perspectives() -> Sidecar {
        let mut s = populated();
        s.perspectives = vec![
            PerspectiveEntry {
                name: "Today's work".to_string(),
                filter: "tag:work AND is:today".to_string(),
                icon: Some("starred-symbolic".to_string()),
                renderer: "list".to_string(),
                renderer_config: None,
            },
            PerspectiveEntry {
                name: "Q3 board".to_string(),
                filter: "project:\"Q3 plans\"".to_string(),
                icon: None,
                renderer: "board".to_string(),
                renderer_config: Some(
                    "{\"axis\":\"tag\",\"columns\":[\"todo\",\"doing\",\"done\"]}".to_string(),
                ),
            },
        ];
        s
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
        // Empty perspectives → commented placeholder so a hand-
        // editor still sees the slot intent.
        assert!(text.contains("# [[perspectives]]"));
        // Mode is absent → no `mode = ` line.
        assert!(!text.contains("mode ="));
    }

    // ── v0.13.x — perspective round-trip ────────────────────────

    #[test]
    fn perspectives_round_trip_through_text() {
        let original = populated_with_perspectives();
        let text = original.emit_text();
        let parsed = Sidecar::parse_text(&text);
        assert_eq!(parsed, original);
    }

    #[test]
    fn perspectives_emit_in_order() {
        let s = populated_with_perspectives();
        let text = s.emit_text();
        let today_idx = text.find("Today's work").expect("first entry present");
        let board_idx = text.find("Q3 board").expect("second entry present");
        assert!(today_idx < board_idx, "order preserved on emit");
    }

    #[test]
    fn perspectives_parse_array_of_tables() {
        let text = "\
[[perspectives]]
name = \"Solo\"
filter = \"is:today\"
renderer = \"list\"
";
        let parsed = Sidecar::parse_text(text);
        assert_eq!(parsed.perspectives.len(), 1);
        assert_eq!(parsed.perspectives[0].name, "Solo");
        assert_eq!(parsed.perspectives[0].filter, "is:today");
        assert_eq!(parsed.perspectives[0].renderer, "list");
        assert!(parsed.perspectives[0].icon.is_none());
        assert!(parsed.perspectives[0].renderer_config.is_none());
    }

    #[test]
    fn perspectives_parse_omitted_renderer_defaults_list() {
        // A perspective entry without a `renderer = ` line
        // defaults to "list" — matches the column DEFAULT.
        let text = "\
[[perspectives]]
name = \"Defaulted\"
filter = \"is:open\"
";
        let parsed = Sidecar::parse_text(text);
        assert_eq!(parsed.perspectives.len(), 1);
        assert_eq!(parsed.perspectives[0].renderer, "list");
    }

    #[test]
    fn perspectives_backwards_compat_with_v010_placeholder() {
        // Pre-v0.13.x sidecars emitted a single-bracket
        // `[perspectives]` section as a reserved placeholder.
        // The v0.13.x parser must keep loading those without
        // failing — the section is treated as Unknown so its
        // contents (if any) are silently dropped.
        let text = "\
mode = \"builder\"

[tags]
work = \"#3584e4\"

[perspectives]
# Reserved for future use.
";
        let parsed = Sidecar::parse_text(text);
        assert_eq!(parsed.mode.as_deref(), Some("builder"));
        assert_eq!(
            parsed.tag_colors.get("work").map(String::as_str),
            Some("#3584e4")
        );
        assert!(parsed.perspectives.is_empty());
    }

    #[test]
    fn perspectives_renderer_config_with_quotes_round_trips() {
        // The renderer_config string is opaque JSON — its
        // double-quotes need to escape through the TOML
        // basic-string layer cleanly.
        let cfg = r#"{"axis":"tag","columns":["todo","doing","done"]}"#;
        let s = Sidecar {
            mode: None,
            tag_colors: BTreeMap::new(),
            perspectives: vec![PerspectiveEntry {
                name: "Board".into(),
                filter: "is:open".into(),
                icon: None,
                renderer: "board".into(),
                renderer_config: Some(cfg.into()),
            }],
        };
        let parsed = Sidecar::parse_text(&s.emit_text());
        assert_eq!(parsed.perspectives[0].renderer_config.as_deref(), Some(cfg),);
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

    #[tokio::test]
    async fn build_from_db_populates_perspectives_in_order() {
        use atrium_core::domain::NewPerspective;
        use atrium_core::spawn_worker;

        let db_path = std::env::temp_dir().join(format!(
            "atrium-sidecar-build-test-{}.db",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&db_path);

        let mut writer = rusqlite::Connection::open(&db_path).unwrap();
        atrium_core::db::configure_pragmas(&writer).unwrap();
        atrium_core::db::migrations::migrate(&mut writer).unwrap();
        let read_conn = rusqlite::Connection::open(&db_path).unwrap();
        atrium_core::db::configure_pragmas(&read_conn).unwrap();

        let (handle, _changes_rx, _library_rx) = spawn_worker(writer);

        handle
            .create_perspective(NewPerspective {
                name: "First".to_string(),
                filter_expr: "is:today".to_string(),
                icon: Some("starred-symbolic".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();
        handle
            .create_perspective(NewPerspective {
                name: "Second".to_string(),
                filter_expr: "tag:work".to_string(),
                renderer: Some("board".to_string()),
                renderer_config: Some("{\"axis\":\"tag\",\"columns\":[\"a\"]}".to_string()),
                ..Default::default()
            })
            .await
            .unwrap();

        let sidecar = build_from_db(&read_conn).unwrap();
        assert_eq!(sidecar.perspectives.len(), 2);
        assert_eq!(sidecar.perspectives[0].name, "First");
        assert_eq!(
            sidecar.perspectives[0].icon.as_deref(),
            Some("starred-symbolic")
        );
        assert_eq!(sidecar.perspectives[0].renderer, "list");
        assert_eq!(sidecar.perspectives[1].name, "Second");
        assert_eq!(sidecar.perspectives[1].renderer, "board");
        assert!(sidecar.perspectives[1].renderer_config.is_some());

        // Round-trip through text — sidecar emit + parse must
        // preserve every field the worker stored.
        let text = sidecar.emit_text();
        let parsed = Sidecar::parse_text(&text);
        assert_eq!(parsed.perspectives, sidecar.perspectives);

        let _ = std::fs::remove_file(&db_path);
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
