# Meson build wrapper for Atrium.
#
# Thin orchestration layer around `cargo build --release`. The Rust
# workspace at `atrium/` (binary) and `atrium-core/` (headless library)
# is the source of truth — meson handles the GNOME-shaped install
# layout (binary into $bindir, GSettings schema into
# $datadir/glib-2.0/schemas, fonts and stylesheet into
# $datadir/atrium/) so the project ships cleanly through Flatpak and
# any distro that expects a meson buildsystem.
#
# Use:
#   meson setup builddir --prefix=/usr
#   meson compile -C builddir
#   meson install -C builddir
#
# Phase 3 lands the application shell — desktop entry / AppStream
# metainfo / icons join in Phase 8.

project(
  'atrium',
  'rust',
  version: '0.0.16',
  meson_version: '>= 0.59.0',
  license: 'MIT',
)

application_id = 'io.github.virinvictus.atrium'
prefix = get_option('prefix')
bindir = prefix / get_option('bindir')
datadir = prefix / get_option('datadir')
appdir = datadir / 'atrium'

cargo = find_program('cargo')
glib_compile_schemas = find_program('glib-compile-schemas')

# Cargo build target. `build_always_stale` lets cargo handle its own
# incremental logic without meson tracking the Rust source tree.
# ATRIUM_DATADIR points cargo's build script at the install-time data
# location so the binary's runtime resolution lands on the right path.
cargo_options = ['--manifest-path', meson.project_source_root() / 'Cargo.toml']
cargo_options += ['--release']
cargo_options += ['--package', 'atrium']

cargo_build = custom_target(
  'cargo-build',
  build_by_default: true,
  build_always_stale: true,
  output: 'atrium',
  console: true,
  install: true,
  install_dir: bindir,
  command: [
    'env',
    'CARGO_TARGET_DIR=' + meson.project_build_root() / 'cargo-target',
    'ATRIUM_DATADIR=' + appdir,
    cargo, 'build', cargo_options,
    '&&', 'cp', meson.project_build_root() / 'cargo-target' / 'release' / 'atrium', '@OUTPUT@',
  ],
)

# GSettings schema. install_data drops the .xml into the canonical
# schema directory; meson.add_install_script invokes
# glib-compile-schemas at install time so end users don't need a
# post-install hook.
install_data(
  'data/' + application_id + '.gschema.xml',
  install_dir: datadir / 'glib-2.0' / 'schemas',
)

meson.add_install_script(
  glib_compile_schemas,
  datadir / 'glib-2.0' / 'schemas',
)

# Bundled fonts. Variable TTFs land at $datadir/atrium/fonts/ where the
# typography module's exe-relative resolver finds them. License files
# travel alongside.
install_subdir(
  'data/fonts',
  install_dir: appdir,
)

# Base stylesheet. Goes next to the fonts so the runtime resolver finds
# both at the same install root.
install_data(
  'data/style.css',
  install_dir: appdir,
)

summary({
  'prefix': prefix,
  'bindir': bindir,
  'datadir': datadir,
  'appdir': appdir,
  'application id': application_id,
}, section: 'Configuration')
