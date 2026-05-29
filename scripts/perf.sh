#!/usr/bin/env bash
# Atrium — performance regression suite (Phase 20, v0.36.0).
#
# Turns the spec §8 budget from a one-off measurement into a
# repeatable, headless gate. Measures the data-layer cost at the
# Large (50K) and Stress (100K) fixture scales — generation time, a
# full read-path load, and peak resident memory — and asserts the
# pieces that don't need a display:
#
#   - data-layer working set at 50K stays well under the §8 idle
#     budget (< 80 MB),
#   - cold-start floor (the GUI `--version` path) beats the §8
#     250 ms first-frame budget.
#
# The GUI-surface budgets (active RSS during a 10K forecast/search
# session; first-interactive-frame on a populated DB) need a running
# window and are measured via the in-app Memory Watch
# (`atrium --debug` → Debug → Memory Watch) — that stays a manual
# checkpoint, noted in docs/perf-baseline.md.
#
# Usage:
#   scripts/perf.sh                 # build release, run the suite
#   scripts/perf.sh --skip-build    # use existing target/release
#   scripts/perf.sh --heaptrack     # also run a heaptrack pass if installed
#
# Exits non-zero on the first budget breach. No network; no Docker.

set -euo pipefail

SKIP_BUILD=false
HEAPTRACK=false
for arg in "$@"; do
  case "$arg" in
    --skip-build) SKIP_BUILD=true ;;
    --heaptrack)  HEAPTRACK=true ;;
    -h|--help)
      sed -n '/^# Atrium/,/^$/p' "$0" | sed 's/^# \?//'
      exit 0
      ;;
    *) echo "perf.sh: unknown argument '$arg'" >&2; exit 2 ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

step() { printf '\n\033[1;34m==> %s\033[0m\n' "$1"; }
fail() { printf '\n\033[1;31mFAIL\033[0m — %s\n' "$1" >&2; exit 1; }

# §8 budgets (the headless-checkable subset).
IDLE_BUDGET_MB=80
COLDSTART_BUDGET_MS=250

if [[ "$SKIP_BUILD" == "false" ]]; then
  step "cargo build --release --workspace"
  cargo build --release --workspace || fail "release build failed"
fi
[[ -x target/release/atrium ]]     || fail "target/release/atrium not found — run without --skip-build"
[[ -x target/release/atrium-cli ]] || fail "target/release/atrium-cli not found — run without --skip-build"

WORK="$(mktemp -d -t atrium-perf-XXXXXX)"
trap 'rm -rf "$WORK"' EXIT

# Peak RSS (MB) of a command, via /usr/bin/time -v "Maximum resident
# set size (kbytes)". Echoes the integer MB.
peak_rss_mb() {
  local logf="$WORK/time.$$"
  /usr/bin/time -v "$@" >/dev/null 2>"$logf" || true
  local kb
  kb="$(awk -F': ' '/Maximum resident set size/ {print $2}' "$logf")"
  echo $(( kb / 1024 ))
}

# Wall time (ms) of a command.
wall_ms() {
  local secs
  secs="$(/usr/bin/time -f '%e' "$@" 2>&1 >/dev/null | tail -1)"
  awk -v s="$secs" 'BEGIN { printf("%d", s * 1000) }'
}

declare -A GEN_MS LOAD_MS LOAD_RSS

for scale in large stress; do
  case "$scale" in
    large)  label="50K";  count="50000"  ;;
    stress) label="100K"; count="100000" ;;
  esac
  dir="$WORK/$scale"
  mkdir -p "$dir"

  step "$label fixture — generate"
  gen_start=$(date +%s%N)
  SUMMARY="$(XDG_DATA_HOME="$dir" target/release/atrium --fixture "$scale" 2>&1 | tail -1)"
  gen_end=$(date +%s%N)
  GEN_MS[$scale]=$(( (gen_end - gen_start) / 1000000 ))
  echo "  $SUMMARY"
  echo "  generated in ${GEN_MS[$scale]} ms"
  case "$SUMMARY" in
    *"$count tasks"*) : ;;
    *) fail "$label fixture did not report $count tasks: $SUMMARY" ;;
  esac

  db="$dir/atrium/atrium.db"
  cli=(target/release/atrium-cli --db "$db" list all)

  step "$label — data-layer full load (read path)"
  LOAD_MS[$scale]="$(wall_ms "${cli[@]}")"
  LOAD_RSS[$scale]="$(peak_rss_mb "${cli[@]}")"
  echo "  loaded all tasks in ${LOAD_MS[$scale]} ms; peak RSS ${LOAD_RSS[$scale]} MB"

  if [[ "$HEAPTRACK" == "true" ]]; then
    if command -v heaptrack >/dev/null 2>&1; then
      step "$label — heaptrack"
      out="$WORK/heaptrack.$scale"
      heaptrack -o "$out" "${cli[@]}" >/dev/null 2>&1 || true
      echo "  heaptrack data at ${out}.*.zst — analyse with: heaptrack_print ${out}.*.zst"
    else
      echo "  heaptrack not installed; skipping (external tool, not a build dep)"
    fi
  fi
done

# Cold-start floor: the GUI --version path (no window, no DB open) is
# the tightest measurable proxy for first-frame latency. Assert it
# beats the §8 250 ms budget with headroom.
step "cold-start floor (atrium --version, ×3)"
for i in 1 2 3; do
  ms="$(wall_ms target/release/atrium --version)"
  echo "  run $i: ${ms} ms"
  if [[ "$ms" -gt "$COLDSTART_BUDGET_MS" ]]; then
    fail "cold-start floor exceeded ${COLDSTART_BUDGET_MS} ms (run $i: ${ms} ms)"
  fi
done

# Budget assertion: the data layer at 50K must sit well under the idle
# budget (the GUI surface gets the rest). 100K is 2× the spec's
# reference stress scale and stays informational.
step "§8 budget check"
if [[ "${LOAD_RSS[large]}" -ge "$IDLE_BUDGET_MB" ]]; then
  fail "50K data-layer working set ${LOAD_RSS[large]} MB ≥ idle budget ${IDLE_BUDGET_MB} MB"
fi
echo "  50K data-layer working set: ${LOAD_RSS[large]} MB (idle budget ${IDLE_BUDGET_MB} MB) — OK"
echo "  100K (informational): load ${LOAD_MS[stress]} ms, peak ${LOAD_RSS[stress]} MB"
echo "  GUI active-RSS + first-frame budgets: measure via 'atrium --debug' → Memory Watch (needs a display)."

printf '\n\033[1;32mPASS\033[0m — Atrium perf suite (v%s)\n' "$(cat VERSION)"
