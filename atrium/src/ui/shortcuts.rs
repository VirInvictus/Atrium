// SPDX-License-Identifier: MIT
//! Keyboard-shortcuts dialog (`Ctrl+?` / `F1`).
//!
//! Loads a `gtk::ShortcutsWindow` from inline XML — keeps the layout
//! declarative without spinning up a third `data/*.ui` file. The
//! source-of-truth for what binds to what is `main.rs::install_accels`;
//! `docs/keymap.md` is the human-readable cousin and stays manually
//! aligned.

const SHORTCUTS_XML: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<interface>
  <object class="GtkShortcutsWindow" id="shortcuts_window">
    <property name="modal">true</property>
    <child>
      <object class="GtkShortcutsSection">
        <property name="section-name">main</property>
        <property name="max-height">10</property>

        <child>
          <object class="GtkShortcutsGroup">
            <property name="title">General</property>
            <child>
              <object class="GtkShortcutsShortcut">
                <property name="title">New task</property>
                <property name="accelerator">&lt;Primary&gt;n</property>
              </object>
            </child>
            <child>
              <object class="GtkShortcutsShortcut">
                <property name="title">Quick Entry</property>
                <property name="accelerator">&lt;Primary&gt;&lt;Alt&gt;space</property>
              </object>
            </child>
            <child>
              <object class="GtkShortcutsShortcut">
                <property name="title">Show keyboard shortcuts</property>
                <property name="accelerator">&lt;Primary&gt;question F1</property>
              </object>
            </child>
            <child>
              <object class="GtkShortcutsShortcut">
                <property name="title">Quit</property>
                <property name="accelerator">&lt;Primary&gt;q</property>
              </object>
            </child>
          </object>
        </child>

        <child>
          <object class="GtkShortcutsGroup">
            <property name="title">Navigation</property>
            <child>
              <object class="GtkShortcutsShortcut">
                <property name="title">Inbox</property>
                <property name="accelerator">&lt;Primary&gt;1</property>
              </object>
            </child>
            <child>
              <object class="GtkShortcutsShortcut">
                <property name="title">Today</property>
                <property name="accelerator">&lt;Primary&gt;2</property>
              </object>
            </child>
            <child>
              <object class="GtkShortcutsShortcut">
                <property name="title">Upcoming</property>
                <property name="accelerator">&lt;Primary&gt;3</property>
              </object>
            </child>
            <child>
              <object class="GtkShortcutsShortcut">
                <property name="title">Anytime</property>
                <property name="accelerator">&lt;Primary&gt;4</property>
              </object>
            </child>
            <child>
              <object class="GtkShortcutsShortcut">
                <property name="title">Someday</property>
                <property name="accelerator">&lt;Primary&gt;5</property>
              </object>
            </child>
            <child>
              <object class="GtkShortcutsShortcut">
                <property name="title">Logbook</property>
                <property name="accelerator">&lt;Primary&gt;6</property>
              </object>
            </child>
          </object>
        </child>

        <child>
          <object class="GtkShortcutsGroup">
            <property name="title">List actions</property>
            <child>
              <object class="GtkShortcutsShortcut">
                <property name="title">Toggle completion of focused task</property>
                <property name="accelerator">space</property>
              </object>
            </child>
            <child>
              <object class="GtkShortcutsShortcut">
                <property name="title">Delete focused task</property>
                <property name="accelerator">Delete</property>
              </object>
            </child>
          </object>
        </child>

        <child>
          <object class="GtkShortcutsGroup">
            <property name="title">Library</property>
            <child>
              <object class="GtkShortcutsShortcut">
                <property name="title">New Project</property>
                <property name="accelerator">&lt;Primary&gt;&lt;Shift&gt;n</property>
              </object>
            </child>
            <child>
              <object class="GtkShortcutsShortcut">
                <property name="title">New Area</property>
                <property name="accelerator">&lt;Primary&gt;&lt;Shift&gt;a</property>
              </object>
            </child>
            <child>
              <object class="GtkShortcutsShortcut">
                <property name="title">Rename active project / area</property>
                <property name="accelerator">F2</property>
              </object>
            </child>
            <child>
              <object class="GtkShortcutsShortcut">
                <property name="title">Delete active project / area</property>
                <property name="accelerator">&lt;Primary&gt;&lt;Shift&gt;Delete</property>
              </object>
            </child>
          </object>
        </child>
      </object>
    </child>
  </object>
</interface>
"##;

pub fn build_shortcuts_window() -> gtk::ShortcutsWindow {
    let builder = gtk::Builder::from_string(SHORTCUTS_XML);
    builder
        .object::<gtk::ShortcutsWindow>("shortcuts_window")
        .expect("shortcuts_window in inline XML")
}

// No unit tests here: `gtk::Builder::from_string` needs a fully
// initialised GTK process, which conflicts with parallel test
// scheduling. The XML is exercised end-to-end every time the user
// hits Ctrl+? (or F1); a parse failure surfaces immediately.
