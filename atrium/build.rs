// SPDX-License-Identifier: MIT
//! Build script.
//!
//! Compiles the GSettings schema (`data/<APP_ID>.gschema.xml`) into
//! `gschemas.compiled` so `cargo run` works without a system install.
//! The compiled output goes into `OUT_DIR`; the binary points
//! `GSETTINGS_SCHEMA_DIR` at it via the `ATRIUM_GSCHEMA_DIR`
//! `cargo:rustc-env` below.
//!
//! For the meson/Flatpak install path, this build script's output is
//! ignored — meson installs the schema XML to
//! `$datadir/glib-2.0/schemas/` and runs `glib-compile-schemas` there.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env_var("CARGO_MANIFEST_DIR"));
    let workspace_data = manifest_dir.parent().expect("workspace root").join("data");
    let out_dir = PathBuf::from(env_var("OUT_DIR"));
    let schema_dir = out_dir.join("glib-2.0").join("schemas");

    let schema_xml = workspace_data.join("io.github.virinvictus.atrium.gschema.xml");
    if !schema_xml.exists() {
        panic!("schema XML not found at {}", schema_xml.display());
    }

    std::fs::create_dir_all(&schema_dir).expect("create schema_dir");

    // Copy XML next to where the compiled blob will land — that's
    // where `glib-compile-schemas` looks for sources.
    let dst_xml = schema_dir.join("io.github.virinvictus.atrium.gschema.xml");
    std::fs::copy(&schema_xml, &dst_xml).expect("copy schema XML");

    compile_schemas(&schema_dir);

    // Bake the compiled-schema directory into the binary so the
    // runtime can populate `GSETTINGS_SCHEMA_DIR` at startup when the
    // binary isn't installed system-wide.
    println!(
        "cargo:rustc-env=ATRIUM_GSCHEMA_DIR={}",
        schema_dir.display()
    );

    // Fonts + style.css live under the workspace `data/` tree; bake
    // their location too so `pango::FontMap::add_font_file` and
    // `gtk::CssProvider::load_from_path` can find them in dev builds.
    println!(
        "cargo:rustc-env=ATRIUM_DATADIR={}",
        workspace_data.display()
    );

    // Localedir: meson exports ATRIUM_LOCALEDIR (= $prefix/$localedir)
    // into the cargo env; plain `cargo build` falls back to the system
    // location (harmless — no atrium.mo there means msgid fallback).
    // Read here and re-emit so option_env! in i18n.rs sees a value
    // regardless of how the ambient env reaches rustc.
    let localedir =
        std::env::var("ATRIUM_LOCALEDIR").unwrap_or_else(|_| "/usr/share/locale".into());
    println!("cargo:rustc-env=ATRIUM_LOCALEDIR={localedir}");
    println!("cargo:rerun-if-env-changed=ATRIUM_LOCALEDIR");

    println!("cargo:rerun-if-changed={}", schema_xml.display());
    println!(
        "cargo:rerun-if-changed={}",
        workspace_data.join("style.css").display()
    );
}

fn compile_schemas(dir: &Path) {
    let status = Command::new("glib-compile-schemas")
        .arg(dir)
        .status()
        .unwrap_or_else(|e| {
            panic!("could not run `glib-compile-schemas` (is glib-2.0 installed?): {e}")
        });
    if !status.success() {
        panic!("glib-compile-schemas exited with {status}");
    }
}

fn env_var(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("{key} not set"))
}
