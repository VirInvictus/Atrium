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
#   5.5 atrium-cli end-to-end smoke against the same fixture (~200 ms)
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

# 5.5 atrium-cli end-to-end smoke. Exercises every read + write
#     subcommand against the fixture DB just generated, asserts
#     each exits cleanly, and confirms the bulk dry-run path
#     returns status 2 (not 0, not 1). Closes the architectural
#     commitment that every non-GUI surface stays CLI-testable.
step "atrium-cli end-to-end smoke"

if [[ ! -x "target/release/atrium-cli" ]]; then
  fail "target/release/atrium-cli not found — run without --skip-build"
fi

CLI_DB="$FIXTURE_DIR/atrium/atrium.db"
CLI=(target/release/atrium-cli --db "$CLI_DB")

# Read paths — every subcommand exits cleanly on the fixture.
"${CLI[@]}" list today        >/dev/null  || fail "atrium-cli list today failed"
"${CLI[@]}" list inbox        >/dev/null  || fail "atrium-cli list inbox failed"
"${CLI[@]}" list upcoming     >/dev/null  || fail "atrium-cli list upcoming failed"
"${CLI[@]}" list anytime      >/dev/null  || fail "atrium-cli list anytime failed"
"${CLI[@]}" list someday      >/dev/null  || fail "atrium-cli list someday failed"
"${CLI[@]}" list logbook      >/dev/null  || fail "atrium-cli list logbook failed"
"${CLI[@]}" list all          >/dev/null  || fail "atrium-cli list all failed"
"${CLI[@]}" list areas        >/dev/null  || fail "atrium-cli list areas failed"
"${CLI[@]}" list projects     >/dev/null  || fail "atrium-cli list projects failed"
"${CLI[@]}" list tags         >/dev/null  || fail "atrium-cli list tags failed"
"${CLI[@]}" list perspectives >/dev/null  || fail "atrium-cli list perspectives failed"

# Search expression smoke — every operator class shipped at v0.5.0.
"${CLI[@]}" search 'is:open'                         >/dev/null || fail "search is:open failed"
"${CLI[@]}" search 'is:today'                        >/dev/null || fail "search is:today failed"
"${CLI[@]}" search 'tag:work AND is:open'            >/dev/null || fail "search compound failed"
"${CLI[@]}" search 'is:open sort:-due'               >/dev/null || fail "search sort: failed"
"${CLI[@]}" search 'is:open AND tag:?wrok'           >/dev/null || fail "search fuzzy failed"

# JSON output is a valid JSON array — sanity check the formatter
# without forcing a python / jq dependency. Piping into `head -c 1`
# also exercises the broken-pipe path: Rust's default stdout
# panics when the pipe closes early; atrium-cli resets SIGPIPE to
# SIG_DFL at startup so it exits cleanly. We disable pipefail
# locally because the producer's SIGPIPE death (exit 141) is the
# *correct* Unix behaviour and not a regression — only the byte
# value matters here.
set +o pipefail
FIRST_BYTE="$("${CLI[@]}" --json list all | head -c 1)"
set -o pipefail
[[ "$FIRST_BYTE" == "[" ]] || fail "atrium-cli --json list all did not emit a JSON array"

# Write path — add → info → search-finds-it → complete → delete.
ADDED_ROW="$("${CLI[@]}" add 'CLI regression smoke' --tag regression-smoke)"
ADDED_ID="$(printf '%s' "$ADDED_ROW" | cut -f1)"
case "$ADDED_ID" in
  ''|*[!0-9]*) fail "atrium-cli add did not return a numeric id: $ADDED_ROW" ;;
esac
"${CLI[@]}" info "$ADDED_ID" >/dev/null || fail "atrium-cli info failed for id=$ADDED_ID"
"${CLI[@]}" search 'tag:regression-smoke' | grep -q 'CLI regression smoke' \
  || fail "atrium-cli search did not surface the freshly-added task"
"${CLI[@]}" edit "$ADDED_ID" --due tomorrow >/dev/null \
  || fail "atrium-cli edit failed for id=$ADDED_ID"
"${CLI[@]}" complete "$ADDED_ID" >/dev/null \
  || fail "atrium-cli complete failed for id=$ADDED_ID"
"${CLI[@]}" delete "$ADDED_ID" >/dev/null \
  || fail "atrium-cli delete failed for id=$ADDED_ID"

# Capture path uses the inline parser (the same one the GUI Quick
# Entry uses). We leave the captured task in place — the bulk
# dry-run + --force flow below uses it as a target.
CAPTURED_ROW="$("${CLI[@]}" capture 'CLI capture smoke #regression-smoke @today')"
CAPTURED_ID="$(printf '%s' "$CAPTURED_ROW" | cut -f1)"
case "$CAPTURED_ID" in
  ''|*[!0-9]*) fail "atrium-cli capture did not return a numeric id: $CAPTURED_ROW" ;;
esac

# Seed a second regression-smoke row so the bulk path has >1 match.
"${CLI[@]}" add 'CLI bulk smoke 2' --tag regression-smoke >/dev/null \
  || fail "atrium-cli add (bulk seed) failed"

# Bulk dry-run: delete --where without --force must exit status 2
# (matched, but did not delete) and leave the rows intact.
set +e
"${CLI[@]}" delete --where 'tag:regression-smoke' >/dev/null 2>&1
DRY_EXIT=$?
set -e
if [[ "$DRY_EXIT" -ne 2 ]]; then
  fail "atrium-cli delete --where (dry run) should exit 2, got $DRY_EXIT"
fi

# --force commits the delete; the rows go away.
"${CLI[@]}" delete --where 'tag:regression-smoke' --force >/dev/null \
  || fail "atrium-cli delete --where --force failed"

# --json emits an array (no header row), so empty == "[]" — easier
# to assert than counting TSV lines past the header.
REMAINING_JSON="$("${CLI[@]}" --json search 'tag:regression-smoke')"
if [[ "$REMAINING_JSON" != "[]" ]]; then
  fail "atrium-cli delete --where --force left rows behind: $REMAINING_JSON"
fi

# Slice D — kanban subcommand renders the seeded board perspective.
# `--fixture small` seeds a "Fixture Board" perspective with three
# tag columns so this exercise has something to render against.
"${CLI[@]}" kanban Fixture Board >/dev/null \
  || fail "atrium-cli kanban Fixture Board (tsv) failed"
"${CLI[@]}" --human kanban Fixture Board >/dev/null \
  || fail "atrium-cli kanban Fixture Board (human) failed"
KANBAN_JSON="$("${CLI[@]}" --json kanban Fixture Board)"
case "$KANBAN_JSON" in
  *'"perspective":"Fixture Board"'*) : ;;
  *) fail "atrium-cli --json kanban did not include perspective field" ;;
esac
# Trying to render a list-renderer perspective as a kanban must
# error (exit non-zero) with a clear message.
set +e
ERR="$("${CLI[@]}" kanban Weekly Review 2>&1)"
RC=$?
set -e
if [[ "$RC" -eq 0 ]]; then
  fail "atrium-cli kanban on a list-renderer perspective should fail"
fi
case "$ERR" in
  *"is a list, not a board"*) : ;;
  *) fail "atrium-cli kanban error message changed: $ERR" ;;
esac

echo "  atrium-cli: 17 read commands + write round-trip + bulk dry-run + bulk force + kanban all OK"

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
