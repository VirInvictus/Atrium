// SPDX-License-Identifier: MIT
//! In-app debug surface (spec §3.4).
//!
//! Activated by the `--debug` CLI flag. Phase 8e replaced the Phase 0
//! stub with a real **Memory Watch** window — opens from the primary
//! menu's *Debug → Memory Watch* entry, samples `/proc/self/status`
//! once a second, and surfaces VmRSS / VmHWM / VmData live so leaks
//! and growth show up without leaving the app.
//!
//! The "drop caches" affordance from spec §3.4 (a button that
//! triggers SQLite `PRAGMA shrink_memory` and any internal cache
//! flush) is a follow-up — it needs worker-side plumbing to dispatch
//! the PRAGMA on the writable connection.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

/// Open a top-level Memory Watch window. The window owns its own
/// 1-second sampler timeout via `glib::timeout_add_local`; the
/// timeout self-terminates when the window is dropped.
pub fn open_memory_watch(parent: &impl IsA<gtk::Window>) {
    let window = adw::Window::builder()
        .title("Atrium Debug — Memory Watch")
        .transient_for(parent)
        .modal(false)
        .default_width(420)
        .default_height(280)
        .resizable(false)
        .css_classes(["atrium-debug-pane"])
        .build();

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(
        &adw::HeaderBar::builder()
            .show_start_title_buttons(false)
            .show_end_title_buttons(true)
            .build(),
    );

    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_start(16)
        .margin_end(16)
        .margin_top(12)
        .margin_bottom(16)
        .build();

    // Big rows: label + value. The value labels are kept in cells so
    // the timeout closure can update them every tick.
    let rss_label = make_value_label();
    let peak_label = make_value_label();
    let data_label = make_value_label();
    let samples_label = make_value_label();

    body.append(&make_pair_row("Resident set size (VmRSS)", &rss_label));
    body.append(&make_pair_row("Peak resident set (VmHWM)", &peak_label));
    body.append(&make_pair_row("Heap (VmData)", &data_label));
    body.append(&make_pair_row("Samples taken", &samples_label));

    let hint = gtk::Label::builder()
        .label("Sampled once per second from /proc/self/status. Close to stop.")
        .halign(gtk::Align::Start)
        .wrap(true)
        .build();
    hint.add_css_class("dim-label");
    hint.add_css_class("caption");
    body.append(&hint);

    toolbar.set_content(Some(&body));
    window.set_content(Some(&toolbar));

    // Shared sample count + timeout id. The id lives in a RefCell so
    // we can remove the source on close (otherwise it'd keep firing
    // and quietly leak CPU after the window is gone).
    let count = Rc::new(RefCell::new(0u64));
    let count_clone = count.clone();
    let rss_w = rss_label.clone();
    let peak_w = peak_label.clone();
    let data_w = data_label.clone();
    let samples_w = samples_label.clone();

    // Prime the readout so the first sample isn't a 1-second blank.
    apply_sample(&rss_w, &peak_w, &data_w, &samples_w, &count_clone);

    let timeout_id = glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
        apply_sample(&rss_w, &peak_w, &data_w, &samples_w, &count_clone);
        glib::ControlFlow::Continue
    });
    let timeout_holder: Rc<RefCell<Option<glib::SourceId>>> =
        Rc::new(RefCell::new(Some(timeout_id)));
    let timeout_for_close = timeout_holder.clone();
    window.connect_close_request(move |_| {
        if let Some(id) = timeout_for_close.borrow_mut().take() {
            id.remove();
        }
        glib::Propagation::Proceed
    });

    window.present();
}

fn make_value_label() -> gtk::Label {
    let l = gtk::Label::builder()
        .halign(gtk::Align::End)
        .label("—")
        .build();
    l.add_css_class("numeric");
    l
}

fn make_pair_row(title: &str, value: &gtk::Label) -> gtk::Box {
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(12)
        .build();
    let key = gtk::Label::builder()
        .label(title)
        .halign(gtk::Align::Start)
        .hexpand(true)
        .build();
    key.add_css_class("dim-label");
    row.append(&key);
    row.append(value);
    row
}

fn apply_sample(
    rss: &gtk::Label,
    peak: &gtk::Label,
    data: &gtk::Label,
    samples: &gtk::Label,
    count: &Rc<RefCell<u64>>,
) {
    *count.borrow_mut() += 1;
    let n = *count.borrow();
    samples.set_text(&format!("{n}"));

    if let Ok(s) = read_proc_status(Path::new("/proc/self/status")) {
        if let Some(kib) = s.vm_rss_kib {
            rss.set_text(&format_kib(kib));
        }
        if let Some(kib) = s.vm_hwm_kib {
            peak.set_text(&format_kib(kib));
        }
        if let Some(kib) = s.vm_data_kib {
            data.set_text(&format_kib(kib));
        }
    }
    // Non-Linux or denied: leave placeholders. Don't spam logs.
}

/// Parsed subset of `/proc/self/status` we surface in the watch.
/// Each field is in KiB (the kernel's reporting unit); `None` means
/// the line was missing from the file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct ProcStatus {
    pub vm_rss_kib: Option<u64>,
    pub vm_hwm_kib: Option<u64>,
    pub vm_data_kib: Option<u64>,
}

pub(crate) fn read_proc_status(path: &Path) -> std::io::Result<ProcStatus> {
    let raw = std::fs::read_to_string(path)?;
    Ok(parse_proc_status(&raw))
}

pub(crate) fn parse_proc_status(raw: &str) -> ProcStatus {
    let mut s = ProcStatus::default();
    for line in raw.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            s.vm_rss_kib = parse_kib_value(rest);
        } else if let Some(rest) = line.strip_prefix("VmHWM:") {
            s.vm_hwm_kib = parse_kib_value(rest);
        } else if let Some(rest) = line.strip_prefix("VmData:") {
            s.vm_data_kib = parse_kib_value(rest);
        }
    }
    s
}

/// Parse a `/proc/self/status` numeric line value of the form
/// "  12345 kB" — leading whitespace and a "kB" suffix.
fn parse_kib_value(rest: &str) -> Option<u64> {
    let trimmed = rest.trim();
    let number = trimmed.split_whitespace().next()?;
    number.parse().ok()
}

/// Render a KiB value as a friendly MB / KB display.
pub(crate) fn format_kib(kib: u64) -> String {
    if kib >= 1024 {
        let mib = kib as f64 / 1024.0;
        format!("{mib:.1} MB")
    } else {
        format!("{kib} KB")
    }
}

/// Phase 0 / 8e stub kept for backwards compatibility with the
/// existing `connect_activate` hook in `main.rs`. Logs that the
/// debug surface is active; the window itself opens from the
/// primary menu.
#[derive(Debug, Default)]
pub struct Pane;

impl Pane {
    pub fn new() -> Self {
        tracing::info!(
            "debug pane active — open Debug → Memory Watch from the primary menu for the live readout"
        );
        Self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_constructs() {
        let _ = Pane::new();
    }

    #[test]
    fn parses_known_status_block() {
        let raw = "Name:\tatrium\n\
                   Pid:\t1234\n\
                   VmPeak:\t  102400 kB\n\
                   VmSize:\t  100000 kB\n\
                   VmHWM:\t   80000 kB\n\
                   VmRSS:\t   75000 kB\n\
                   VmData:\t  40000 kB\n";
        let s = parse_proc_status(raw);
        assert_eq!(s.vm_rss_kib, Some(75000));
        assert_eq!(s.vm_hwm_kib, Some(80000));
        assert_eq!(s.vm_data_kib, Some(40000));
    }

    #[test]
    fn parse_handles_missing_lines() {
        let raw = "Name:\tatrium\nVmRSS:\t  4096 kB\n";
        let s = parse_proc_status(raw);
        assert_eq!(s.vm_rss_kib, Some(4096));
        assert_eq!(s.vm_hwm_kib, None);
        assert_eq!(s.vm_data_kib, None);
    }

    #[test]
    fn parse_handles_unexpected_format() {
        let raw = "VmRSS:\tnot_a_number kB\n";
        let s = parse_proc_status(raw);
        assert_eq!(s.vm_rss_kib, None);
    }

    #[test]
    fn format_kib_prefers_mb_above_one_megabyte() {
        assert_eq!(format_kib(0), "0 KB");
        assert_eq!(format_kib(512), "512 KB");
        assert_eq!(format_kib(1024), "1.0 MB");
        assert_eq!(format_kib(75000), "73.2 MB");
        assert_eq!(format_kib(10240), "10.0 MB");
    }

    #[test]
    fn read_proc_status_works_on_self() {
        // Smoke check on Linux — own /proc/self/status is always
        // readable and always has VmRSS while the test process runs.
        let status = read_proc_status(Path::new("/proc/self/status"));
        if let Ok(s) = status {
            assert!(s.vm_rss_kib.is_some(), "VmRSS should be present");
        }
        // On non-Linux hosts the read errors; skip silently.
    }
}
