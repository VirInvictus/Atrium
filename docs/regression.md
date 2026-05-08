# Atrium — Regression Gate

The single command that answers "is `main` ready to tag?" is:

```bash
scripts/regression.sh
```

It runs every gate the v0.1 release sequence (Phase 9) cares about, in fail-fast order, and ends with a single `PASS` / `FAIL` line. No network calls, no Docker, no external deps beyond the standard cargo + GTK4 toolchain that's already required to build the project.

## What it checks

| Step | Command | Time |
|---|---|---|
| 1 | `cargo fmt --all -- --check` | instant |
| 2 | `cargo clippy --workspace --all-targets -- -D warnings` | ~3 s incremental |
| 3 | `cargo test --workspace` | <1 s |
| 4 | `cargo build --release --workspace` | ~45 s clean / fast incrementally |
| 5 | 1K-task fixture smoke (`atrium --fixture small` against a tmp `XDG_DATA_HOME`) | ~80 ms |
| 5.5 | `atrium-cli` end-to-end smoke against the fixture DB (read + write subcommands, kanban renderer, perspective CRUD) | ~200 ms |
| 6 | Cold-start sanity ×3 (`atrium --version`, asserts < 500 ms each) | ~100 ms total |

The cold-start step is conservative: 500 ms is well above what the §8 budget calls for (250 ms on a 5K-task DB) and well above the observed numbers (~30–40 ms in `docs/perf-baseline.md`). Headroom keeps it from flapping on a slow host while still catching regressions of multiple-x.

The fixture smoke uses a `mktemp -d` directory passed as `XDG_DATA_HOME`, so the gate never touches the developer's real `atrium.db`. The directory is removed on script exit.

The 5.5 `atrium-cli` smoke (added at v0.5.x and grown through v0.6.x) reuses the same fixture DB. It exercises every read subcommand (`list` over all canonical lists + areas + projects + tags + perspectives), every write subcommand (`add`, `capture`, `edit`, `complete`, `delete`), and every metadata flow (`info`, `search` with both fast-path and fallback predicates). v0.5.4 added the kanban smoke against the fixture-seeded "Fixture Board" perspective; v0.6.5 added the `perspective` write-side smoke (create / edit / delete + the no-op-print case). Together with the unit-test suite this is the closest the CLI gate can get to "actually used the app" without a display.

## When to run it

- Before tagging any **minor** or **major** version.
- Before merging a branch that touches the data layer, the worker, or the schema.
- Before running `flatpak-builder` against the manifest — a green gate is the precondition for trusting a release build.

For patch versions (typo fixes, doc-only changes), running the gate is optional but recommended; it costs ~5 s incrementally.

## Flags

```
scripts/regression.sh                 # full gate
scripts/regression.sh --skip-build    # reuse existing target/release
scripts/regression.sh --help          # render this section
```

`--skip-build` is the right call when chaining the gate after another `cargo build --release` you've just run — saves 45 s on cold builds. The script verifies `target/release/atrium` still exists before invoking the fixture step.

## Failure semantics

- The script aborts on the first failing step (`set -e` + explicit `fail` on each gate).
- Step output (clippy diagnostics, test output, fixture summary) goes to stdout/stderr in real time — readable without re-running.
- The trailing `PASS` line carries the current `VERSION` so the log identifies which build was tested.

## What it does NOT cover

These are deliberate gaps the gate doesn't try to close:

- **GUI smoke** — opening the actual window requires a display server, which `cargo` in CI doesn't have. Manual verification stays manual; `docs/accessibility.md` lists the checks you'd walk through.
- **Flatpak build** — needs `flatpak-builder` + the GNOME 50 runtime. Run `flatpak-builder --user --install --force-clean build-dir data/io.github.virinvictus.atrium.yml` separately when packaging.
- **`heaptrack` profiling** — the perf baseline doc (`docs/perf-baseline.md`) covers this with explicit reproduction steps; not part of the everyday gate because it's slow and needs the tool installed.

## Adding a new gate

If a future check warrants a place in the regression script:

1. Pick a position in the fail-fast order (cheap checks early, slow checks late).
2. Add a `step "..."` heading and the actual command + `|| fail "..."` line.
3. Update the table above.
4. Mention it in the Phase 9 notes for whichever release introduces it.

Keep the gate fast. The whole script should finish under a minute on a clean release build, under 5 s when the build is warm.
