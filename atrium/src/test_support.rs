// SPDX-License-Identifier: MIT
//! Process-wide GTK initialisation for the test suite.
//!
//! `gtk::init` is process-global and not safe to call concurrently:
//! libtest runs every test in its own thread, so two tests entering
//! `gtk::init` at the same moment can abort the whole binary with
//! `gdk_display_manager_get() was called before gtk_init()` (seen once
//! on CI, run 34059225968; roadmap Phase 23 recon finding). The `Once`
//! below makes the initialisation exactly-once and blocks every other
//! thread until it completes, so no test can observe an uninitialised
//! GDK.
//!
//! GTK-touching test modules must go through `gtk_init_once` instead
//! of calling `gtk::init` directly. The helper lives in the binary
//! crate because `atrium-core` is GUI-free by commitment and cannot
//! carry a `gtk` dependency.

use std::sync::Once;

static GTK_INIT: Once = Once::new();

/// Initialise GTK exactly once per process. Concurrent callers block
/// until the initialisation finishes; later callers return
/// immediately. Mirrors the old `let _ = gtk::init()` tolerance: an
/// error means the environment has no display (use `xvfb-run` on CI),
/// and the first widget touch surfaces it.
pub(crate) fn gtk_init_once() {
    GTK_INIT.call_once(|| {
        let _ = gtk::init();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_init_leaves_gtk_initialised() {
        // Contention regression for the init race: eight threads
        // racing the helper must all come back alive with GTK
        // initialised. The pre-Once shape (each test calling
        // `gtk::init` directly) could abort the process here.
        let handles: Vec<_> = (0..8).map(|_| std::thread::spawn(gtk_init_once)).collect();
        for h in handles {
            h.join().expect("init thread must not abort");
        }
        assert!(gtk::is_initialized());
    }
}
