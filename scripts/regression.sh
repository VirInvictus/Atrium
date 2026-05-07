#!/usr/bin/env bash
# Atrium — regression gate (Phase 9a).
#
# Runs every check the ship gate cares about, in the order that
# fails fastest first:
#
#   1. cargo fmt --check                           (instant)
#   2. cargo clippy -D warnings                    (~3 s incremental)
#   3. cargo test --workspace                      (<1 s)
#   4. cargo build --release                       (~45 s clean, fast incrementally)
#   5. release-mode 1K-task fixture smoke           (~50 ms)
#   6. release-mode --version cold start (×3)      (~100 ms total)
#
# Exits non-zero on the first failing step. No network calls; no
# Docker; no external deps beyond the standard cargo + GTK toolchain.
#
# Usage:
#   scripts/regression.sh
#   scripts/regression.sh --skip-build      # use existing target/release
#
# Run it before tagging any minor or major version. The output ends
# with a single PASS / FAIL line so it's easy to grep in CI logs.

set -euo pipefail

SKIP_BUILD=false
for arg in "$@"; do
  case "$arg" in
    --skip-build)
      SKIP_BUILD=true
      ;;
    -h|--help)
      sed -n '/^# Atrium/,/^$/p' "$0" | sed 's/^# \?//'
      exit 0
      ;;
    *)
      echo "regression.sh: unknown argument '$arg'" >&2
      exit 2
      ;;
  esac
done

# Move to the repo root regardless of where the script is invoked from.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

# Pretty step headers so the log is scannable.
step() {
  printf '\n\033[1;34m==> %s\033[0m\n' "$1"
}

fail() {
  printf '\n\033[1;31mFAIL\033[0m — %s\n' "$1" >&2
  exit 1
}

# 1. Formatting.
step "cargo fmt --all -- --check"
cargo fmt --all -- --check || fail "cargo fmt found unformatted code"

# 2. Clippy with warnings as errors.
step "cargo clippy --workspace --all-targets -- -D warnings"
cargo clippy --workspace --all-targets -- -D warnings || fail "clippy reported warnings"

# 3. Unit + integration tests.
step "cargo test --workspace"
cargo test --workspace || fail "tests failed"

# 4. Release build (skippable when chained — the fixture step needs it).
if [[ "$SKIP_BUILD" == "false" ]]; then
  step "cargo build --release --workspace"
  cargo build --release --workspace || fail "release build failed"
fi

if [[ ! -x "target/release/atrium" ]]; then
  fail "target/release/atrium not found — run without --skip-build"
fi

# 5. 1K-task fixture smoke. Uses an isolated XDG_DATA_HOME so we
#    don't touch the user's real database.
step "1K-task fixture smoke"
FIXTURE_DIR="$(mktemp -d -t atrium-regression-XXXXXX)"
trap 'rm -rf "$FIXTURE_DIR"' EXIT
SUMMARY="$(XDG_DATA_HOME="$FIXTURE_DIR" target/release/atrium --fixture small 2>&1 | tail -1)"
echo "$SUMMARY"
case "$SUMMARY" in
  *"Generated 1000 tasks"*)
    : # OK
    ;;
  *)
    fail "fixture summary did not report 1000 tasks: $SUMMARY"
    ;;
esac

# 6. Cold-start sanity (×3). Median should comfortably beat the
#    spec §8 250 ms budget for the GUI cold start. The CLI --version
#    path is an even tighter floor; we assert <500 ms with headroom.
step "cold-start sanity (×3)"
for i in 1 2 3; do
  ELAPSED_MS="$(/usr/bin/time -f '%e' target/release/atrium --version 2>&1 | tail -1 | awk '{ printf("%d", $1 * 1000) }')"
  echo "  run $i: ${ELAPSED_MS} ms"
  if [[ "$ELAPSED_MS" -gt 500 ]]; then
    fail "cold start exceeded 500 ms (run $i: ${ELAPSED_MS} ms)"
  fi
done

# Done.
printf '\n\033[1;32mPASS\033[0m — Atrium regression gate (v%s)\n' \
  "$(cat VERSION)"
