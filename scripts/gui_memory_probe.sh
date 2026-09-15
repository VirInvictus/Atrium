#!/usr/bin/env bash
# Atrium — GUI memory probe (v0.76.0).
#
# Reproducible sampling of the GUI binary's RSS against an isolated
# fixture database, so the §8 GUI budgets (idle < 80 MB, active
# < 200 MB on a 10K-task DB) get measured the same way every time
# instead of via ad-hoc Memory Watch reads. perf.sh covers the
# data-layer budgets headlessly; this script is the GUI-side
# counterpart that perf.sh's own banner points at.
#
# What it does:
#   1. Builds the release binary (unless --skip-build).
#   2. Creates a throwaway XDG_DATA_HOME and generates a fixture DB
#      in it. The override is not optional: the generator appends to
#      whatever database db_path() resolves to (see the roadmap box
#      on the --fixture footgun), so this script never aims it at the
#      real one.
#   3. Launches `atrium --debug` on the live Wayland session, waits
#      for the window to settle, then samples VmRSS from
#      /proc/<pid>/status once per second.
#   4. Prints idle and peak RSS plus the budget verdicts.
#
# Navigating to a kanban board or other surfaces stays a manual pass
# (use --debug's Memory Watch live while you click); this probe
# covers boot + default-list idle, the cheapest reproducible slice.
#
# Usage:
#   scripts/gui_memory_probe.sh [small|medium|large]   # default medium
#   scripts/gui_memory_probe.sh --skip-build [scale]

set -euo pipefail

SKIP_BUILD=false
SCALE="medium"
for arg in "$@"; do
  case "$arg" in
    --skip-build) SKIP_BUILD=true ;;
    small|medium|large) SCALE="$arg" ;;
    *)
      echo "usage: scripts/gui_memory_probe.sh [--skip-build] [small|medium|large]" >&2
      exit 2
      ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

# Agent shells (the ZCode AppImage) can leak a GSETTINGS_SCHEMA_DIR
# pointing at their own bundle, which stops the app's early main() from
# pointing at the cargo-built schema (see reminders.rs's note). Unset it
# so install_gsettings_schema_dir does its job.
unset GSETTINGS_SCHEMA_DIR

fail() {
  printf '\n\033[1;31mFAIL\033[0m — %s\n' "$1" >&2
  exit 1
}

if [[ "$SKIP_BUILD" == "false" ]]; then
  echo "==> cargo build --release (atrium only)"
  cargo build --release -p atrium
fi
BIN=target/release/atrium
[[ -x "$BIN" ]] || fail "target/release/atrium not found (run without --skip-build)"

# Isolated data dir: the probe must never touch the real database.
PROBE_DIR="$(mktemp -d -t atrium-gui-probe-XXXXXX)"
trap 'kill "${APP_PID:-0}" 2>/dev/null || true; rm -rf "$PROBE_DIR"' EXIT

echo "==> generating $SCALE fixture into $PROBE_DIR"
XDG_DATA_HOME="$PROBE_DIR" "$BIN" --fixture "$SCALE" >/dev/null
DB="$PROBE_DIR/atrium/atrium.db"
[[ -s "$DB" ]] || fail "fixture database was not created at $DB"

echo "==> launching GUI on the live Wayland session"
if [[ -z "${WAYLAND_DISPLAY:-}" ]]; then
  fail "WAYLAND_DISPLAY is not set; the probe needs the live graphical session"
fi
XDG_DATA_HOME="$PROBE_DIR" GDK_BACKEND=wayland "$BIN" --debug &
APP_PID=$!

# Wait for the process and its RSS to appear, then for the window to
# settle (first frame + initial list load + reminder service boot).
for i in $(seq 1 60); do
  kill -0 "$APP_PID" 2>/dev/null || fail "atrium exited during startup (check the log above)"
  [[ -r "/proc/$APP_PID/status" ]] && grep -q '^VmRSS' "/proc/$APP_PID/status" && break
  sleep 0.5
done
sleep 5

echo "==> sampling RSS (1 Hz, 20 s idle window)"
SAMPLES=()
PEAK_KB=0
for i in $(seq 1 20); do
  KB="$(awk '/^VmRSS:/ {print $2}' "/proc/$APP_PID/status" 2>/dev/null || echo 0)"
  SAMPLES+=("$KB")
  (( KB > PEAK_KB )) && PEAK_KB=$KB
  sleep 1
done
IDLE_KB="${SAMPLES[-1]}"
PEAK_MB=$(( PEAK_KB / 1024 ))
IDLE_MB=$(( IDLE_KB / 1024 ))

kill "$APP_PID" 2>/dev/null || true
wait "$APP_PID" 2>/dev/null || true

echo "  scale:        $SCALE fixture"
echo "  idle RSS:     ${IDLE_MB} MB (last of ${#SAMPLES[@]} samples)"
echo "  peak RSS:     ${PEAK_MB} MB (idle window)"
echo "  idle budget:  80 MB (spec §8);  active budget: 200 MB on a 10K DB"

STATUS=0
if (( IDLE_MB >= 80 )); then
  echo "  idle OVER the 80 MB budget" >&2
  STATUS=1
else
  echo "  idle within the 80 MB budget"
fi

# Report-only exit: the active budget needs interactive navigation,
# so this script asserts only the idle line and prints the rest for
# the perf-baseline record.
printf '\n\033[1;32mDONE\033[0m — GUI memory probe (idle %s MB / peak %s MB)\n' "$IDLE_MB" "$PEAK_MB"
exit "$STATUS"
