// SPDX-License-Identifier: MIT
//! v0.30.0 — drag external files / URLs onto the window to capture.
//!
//! A top-level `gtk::DropTarget` accepts dropped files (`gdk::FileList`,
//! how file managers deliver a drag) and text (`String`, how a browser
//! delivers a URL or selected text). Either opens Quick Entry pre-filled
//! so the user can review, add `#tag`s, and commit — capture, not a
//! silent insert. Mirrors how Errands / Planify treat drops.

use std::path::Path;

use adw::prelude::*;
use gtk::glib::clone;
use gtk::{gdk, glib};

use super::AtriumWindow;

impl AtriumWindow {
    pub(super) fn install_drop_target(&self) {
        let target = gtk::DropTarget::new(glib::Type::INVALID, gdk::DragAction::COPY);
        target.set_types(&[gdk::FileList::static_type(), String::static_type()]);
        target.connect_drop(clone!(
            #[weak(rename_to = win)]
            self,
            #[upgrade_or]
            false,
            move |_, value, _x, _y| {
                let prefill = if let Ok(files) = value.get::<gdk::FileList>() {
                    files
                        .files()
                        .first()
                        .and_then(|f| f.path())
                        .and_then(|p| p.to_str().map(path_stem))
                        .unwrap_or_default()
                } else if let Ok(text) = value.get::<String>() {
                    capture_prefill_from_drop(&text)
                } else {
                    String::new()
                };
                if prefill.is_empty() {
                    return false;
                }
                let worker = win.worker_handle_for_quickentry();
                let pool = win.read_pool_for_quickentry();
                crate::quickentry::modal::open(&win, worker, pool, Some(prefill));
                true
            }
        ));
        self.add_controller(target);
    }
}

/// Derive a Quick Entry pre-fill string from a dropped text payload.
/// A `file://` URI becomes the file's base name (extension stripped);
/// an http(s) URL (or anything else) keeps its first non-empty line
/// verbatim. Pure so it can be unit-tested without GTK.
pub(super) fn capture_prefill_from_drop(payload: &str) -> String {
    let first = payload
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if let Some(rest) = first.strip_prefix("file://") {
        // Drop an optional authority: file://host/path → /path.
        let path = match rest.find('/') {
            Some(i) => &rest[i..],
            None => rest,
        };
        return path_stem(&percent_decode(path));
    }
    first.to_string()
}

/// Base name of a filesystem path with the final extension stripped
/// (`/home/u/My Report.pdf` → `My Report`). Falls back to the raw
/// input when there's no usable stem.
fn path_stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}

/// Minimal percent-decoding for file URIs (`%20` → space, etc.).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            out.push((h << 4) | l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::capture_prefill_from_drop;

    #[test]
    fn file_uri_becomes_basename_without_extension() {
        assert_eq!(
            capture_prefill_from_drop("file:///home/u/My%20Report.pdf"),
            "My Report"
        );
        assert_eq!(capture_prefill_from_drop("file:///tmp/notes.txt"), "notes");
    }

    #[test]
    fn file_uri_with_authority() {
        assert_eq!(capture_prefill_from_drop("file://host/srv/plan.md"), "plan");
    }

    #[test]
    fn url_kept_verbatim() {
        assert_eq!(
            capture_prefill_from_drop("https://example.com/page"),
            "https://example.com/page"
        );
    }

    #[test]
    fn plain_text_uses_first_nonempty_line() {
        assert_eq!(capture_prefill_from_drop("Buy milk\nsecond"), "Buy milk");
        assert_eq!(capture_prefill_from_drop("   \n  Call Sam  "), "Call Sam");
    }

    #[test]
    fn empty_payload_yields_empty() {
        assert_eq!(capture_prefill_from_drop("  \n  "), "");
    }
}
