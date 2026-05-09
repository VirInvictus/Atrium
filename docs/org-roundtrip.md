# Org-mode round-trip — what Atrium converts

A reference for what makes it through the DB ↔ vault loop, what doesn't, and how to see it for yourself. Pairs with [`spec.md` §7.3](../spec.md) (the contract) and the round-trip discipline rules in §7.3.3.

## The four surfaces

The same parser drives every capture surface and the same emitter drives every vault write. There is one inline-syntax vocabulary and one Org-emit format, shared across:

```
                        ┌─────────────────┐
                        │   atrium-core   │
                        │   (SQLite, the  │
                        │    canonical    │
                        │    store)       │
                        └────────┬────────┘
                                 │
                ┌────────────────┼────────────────┐
                │                │                │
        ┌───────▼─────┐  ┌───────▼─────┐  ┌──────▼────────┐
        │   Atrium    │  │ atrium-cli  │  │ Org vault     │
        │   GUI       │  │ (capture +  │  │ (~/Tasks/,    │
        │   (Quick    │  │  import +   │  │  the Doom-    │
        │   Entry +   │  │  export)    │  │  visible      │
        │   inline    │  │             │  │  mirror)      │
        │   rename)   │  │             │  │               │
        └─────────────┘  └─────────────┘  └───────────────┘
```

The Org vault is a *projection* of the DB, not a sibling store ([spec §3.5](../spec.md)). The DB is canonical; the vault is downstream. Atrium runs cleanly without a vault; the vault never runs without the DB.

## See it for yourself

```bash
# From the workspace root.
rm -rf ~/Tasks ~/.local/share/atrium/atrium.db*
mkdir -p ~/Tasks
gsettings set io.github.virinvictus.atrium vault-path ~/Tasks
cargo run -p atrium-cli -- import org demos/showcase/
cargo run -p atrium
```

Three projects across two areas land in the DB. The v0.13.5 fresh-vault seed mirrors them to `~/Tasks/` the moment the GUI's data layer attaches; opening any of the regenerated `.org` files in DoomEmacs shows the canonical Atrium emit format. Edit a task title in either Atrium or Emacs, save, and the other side picks the change up in ~200 ms via the `inotify` watcher.

`demos/README.md` has the full run sequence including reset commands.

## Supported constructs (the extent of the conversion)

Every entry below is exercised by `atrium-org/tests/org_roundtrip.rs`'s `comprehensive_*` suite — failing tests would name the specific construct that broke.

### TODO-cycle keywords

| Source keyword | DB representation | Round-trip |
|---|---|---|
| `TODO` | `task.completed_at = NULL`, no `orig_keyword` | clean |
| `DONE` | `task.completed_at = <CLOSED stamp>`, no `orig_keyword` | clean |
| `CANCELLED` | `task.completed_at = <CLOSED stamp>`, `orig_keyword = 'CANCELLED'` | clean — needs a `CLOSED:` cookie on the source |
| `WAITING` / `IN-PROGRESS` / `BLOCKED` / any custom word | `task.orig_keyword = '<word>'`, `task.completed_at = NULL` | clean — migration 0007 added `orig_keyword` for exactly this |

### Planning cookies

`SCHEDULED:` / `DEADLINE:` / `CLOSED:` and every subset of the three round-trip. Stock Emacs concatenates multiple cookies onto one "planning line" and Atrium matches; the parser tolerates either form on read.

```org
* DONE All three cookies
SCHEDULED: <2026-05-15 Fri> DEADLINE: <2026-06-01 Mon> CLOSED: [2026-05-30 Sat]
:PROPERTIES:
:ID: …
:END:
```

`CLOSED:` carries an optional time-of-day component (`[2026-05-08 Fri 14:22]`) that survives. When the source has no time component, Atrium emits the date-only form (`[2026-05-08 Fri]`).

### Repeaters

All three Org repeater modes round-trip via `task.repeat_rule` (canonical RFC 5545 RRULE, [spec §7.3.3 rule 3](../spec.md)) plus a best-fit Org cookie on `SCHEDULED`:

| Cookie | Atrium `RepeatMode` | Semantics |
|---|---|---|
| `+1w` | `Basic` | Date stays as scheduled; doesn't auto-advance |
| `++1w` (default) | `Cumulative` | Next instance lands one week after the most recent due date even if completion was late |
| `.+1w` | `Next` | Next instance schedules from when the user actually completed it |

Multi-day RRULEs (`BYDAY=MO,WE,FR`) and BYMONTHDAY-style patterns are stored canonically in `:RRULE:` and projected to a best-fit cookie on the `SCHEDULED` line. Stock `org-agenda` renders the cookie; Atrium reads `:RRULE:` on read-back. If the user retunes the cookie alone (without updating `:RRULE:`), divergence detection rewrites the file from canonical (v0.10.3).

### Headline tags

`:tag1:tag2:tag3:` slot. Many tags survive — order isn't semantic on the DB side. The Org writer emits in tag-id order (which matches `task_tag` insertion order).

### Properties drawer

`task.uuid` ↔ `:ID:`, `task.repeat_rule` ↔ `:RRULE:`, `task.estimated_minutes` ↔ `:EFFORT:` (in `H:MM` form), `task.defer_until` ↔ `:DEFER_UNTIL:`. These four well-known keys are the lossless set.

### File-level project metadata

Sourced from a top-level `:PROPERTIES:` drawer that appears before the first headline:

| Org property | DB column | Atrium field |
|---|---|---|
| `:ID:` | `project.uuid` | round-trip anchor |
| `:SEQUENTIAL:` | `project.sequential` | Builder mode "next-task-only" availability |
| `:REVIEW_INTERVAL:` | `project.review_interval_days` | Review queue cadence |
| `:LAST_REVIEWED:` | `project.last_reviewed_at` | inactive timestamp |
| `:ARCHIVED:` | `project.archived_at` | inactive timestamp |

The file-level `#+TITLE:` directive becomes `project.title`.

### Subtask hierarchy

Arbitrary depth via `task.parent_id`. The showcase goes four levels deep; the schema doesn't constrain it.

### Body content

Everything between the headline + cookies + properties drawer and the next headline goes verbatim into `task.note`. That makes Atrium safe for vault-as-living-document use — Org tables, source blocks, lists, internal links, and external URL links all survive even though Atrium itself doesn't render them. Spec §7.3.3 rule 1 is the contract: "preserve unknown constructs verbatim."

```org
* TODO Refactor the dashboard
:PROPERTIES:
:ID: …
:END:
Background notes:

#+BEGIN_SRC sql
SELECT user_id, COUNT(*) FROM events GROUP BY user_id;
#+END_SRC

| Sub-module | Status      |
|------------+-------------|
| Charts     | DONE        |
| Filters    | IN-PROGRESS |

- bullet item
- nested
  - deeper
- sibling

[[https://example.com][external link]]
[[file:./other.org::Heading][internal link]]
```

All of that round-trips intact.

### Vault layout

```
<vault_root>/
├── inbox.org                          ← unfiled projects (one .org per project)
├── <Project Title>.org                ← unfiled, one file per project
├── <Area Title>/                      ← filed projects under area subdirectories
│   ├── <Project Title>.org
│   └── <Project Title>.org
└── .atrium/
    └── config.toml                    ← Atrium-only sidecar (tag colours, mode pref, saved Perspectives)
```

Every `.org` file is written atomically (`write-temp + fsync + rename`) and re-parsed for an integrity check before the writer considers the flush successful. Crash mid-write never corrupts the destination.

### Sidecar (`<vault>/.atrium/config.toml`)

Hand-rolled minimal TOML. Currently round-trips:

- **Tag colours.** `[tags]` section with `name = "#RRGGBB"` entries.
- **Mode preference.** Top-level `mode = "simple"` / `"builder"` (recorded; not authoritative — the GUI's local GSettings wins on conflict).
- **Saved Perspectives.** TOML array-of-tables (`[[perspectives]]`) with `name`, `filter`, optional `icon`, `renderer` (`"list"` / `"board"`), optional `renderer_config` (opaque JSON for board configs). Added v0.13.1.

Other Org tools ignore the `.atrium/` directory by convention.

## Known limits

Two construct classes don't fully round-trip yet. Both have dedicated `documented_limit_*` tests that fail the moment the gap closes — flipping each from "documenting the limit" to "asserting preservation" is the regression-detection target.

### Project sub-headings (writer-only)

The v0.12.0 writer learned to emit project sub-headings as depth-1 keyword-less headlines (driven by the Todoist mapper). The Org *importer* still skips them — they're counted in `ImportSummary::headings_skipped` and don't land in the `heading` table. Tasks under a sub-heading flow into the project at top level "as if the sub-heading were transparent".

```org
* First section                    ← currently skipped on import

** TODO Task under first section   ← lands at project top-level
```

Closing the loop is a bounded change: have the importer call `WorkerHandle::ensure_heading` on every depth-1 keyword-less headline (plumbing already in place since the v0.12.0 mapper uses it).

Test pinning the limit: `documented_limit_org_importer_skips_sub_headings`.

### Custom property-drawer keys

Atrium's importer cherry-picks the four well-known keys (`ID`, `EFFORT`, `DEFER_UNTIL`, `RRULE`) and writes them through typed columns. Custom keys — `:CATEGORY:`, `:CLIENT:`, `:URL:`, anything else a user might put in their drawer — get dropped because the schema doesn't have a place for arbitrary key-value extras.

```org
* TODO Task with rich drawer
:PROPERTIES:
:ID: …                              ← survives
:EFFORT: 1:30                       ← survives
:CATEGORY: Q3-deliverables          ← dropped on import
:URL: https://example.com/ticket/42 ← dropped on import
:END:
```

Spec §7.3.3 rule 1 ("preserve unknown constructs verbatim") is upheld for body content but not for property-drawer keys outside the well-known set. Closing this gap needs either a `task_property` table or a JSON column on `task` — both schema-changing, both schedulable as their own work item.

Test pinning the limit: `documented_limit_org_importer_drops_custom_property_keys`.

## Where this lives in the code

| Concern | Location |
|---|---|
| Org parser (text → AST) | `atrium-org/src/org/parse.rs` — hand-rolled headline / cookie / properties / body parser. No third-party Org crate; `orgize` and `starsector` were both surveyed and rejected at Phase 16 (dormant + alpha). |
| Org emitter (AST → text) | `atrium-org/src/org/emit.rs` — produces stable, org-agenda-readable output with byte-stable property ordering and the v0.13.3 blank-line-between-headlines styling. |
| One-shot import path | `atrium-org/src/org/import.rs` — single-file + multi-file vault walker. `import_org_file(handle, path, dry_run)` and `import_org_directory(handle, path, dry_run)`. |
| Vault writer task | `atrium-org/src/vault_writer.rs` — receives `ProjectDirty(project_id)` over `tokio::mpsc`, debounces ~100 ms, atomically rewrites the affected `.org` files. |
| Vault watcher task | `atrium-org/src/vault_watcher.rs` — `notify` v8 backend with a 200 ms debounce. Reads each modified file, diffs by `:ID:` against the DB, dispatches CRUD via the worker handle. |
| Self-write filter | `atrium-org/src/self_write.rs` — shared `RecentWrites` set keyed on `(path, mtime)` exact-tuple equality. Suppresses the inotify echo a writer creates by writing its own files. |
| RRULE projection helpers | `atrium-org/src/rrule_cookie.rs` — RRULE ↔ Org cookie projection. The lossy direction is detected and surfaced as `VaultEvent::RruleDiverged`. |
| Sidecar | `atrium-org/src/sidecar.rs` — `.atrium/config.toml` round-trip. Hand-rolled minimal TOML; no `toml` crate dep. |

## Round-trip contract — spec §7.3.3 in plain English

1. **Preserve unknown constructs verbatim.** If Atrium doesn't model a construct, it survives in the body field and re-emits as-is. This is what makes the vault safe to edit in Doom — your Org tables, source blocks, custom drawers, etc. won't get clobbered.

2. **`:ID:` is the round-trip anchor.** Tasks without an `:ID:` on import get one auto-generated; subsequent edits flow through that uuid. Never delete `:ID:` lines manually.

3. **`:RRULE:` is canonical; the SCHEDULED cookie is best-fit projection.** When you retune the cookie alone in Emacs, divergence detection fires and the file gets rewritten from the canonical `:RRULE:`.

4. **Conflicts are surfaced, not silenced.** Pre-write the writer stats the destination file. If the mtime isn't in `RecentWrites` (an external editor touched it), the current contents copy to `<file>.atrium.bak.<UTC-timestamp>` first. The user's hand-edits never get lost — only relocated.

5. **Atomic writes.** `write-temp + fsync + rename` for every vault write, plus a post-write integrity check that re-parses the file and fails the flush on any divergence.

## Further reading

- [`spec.md` §3.5](../spec.md) — the architectural commitment to vault-as-projection.
- [`spec.md` §7.3](../spec.md) — the full contract (vault layout, field mapping, round-trip rules).
- [`spec.md` §6](../spec.md) — Quick Entry vocabulary (the inline-syntax tokens are part of the same conversion story).
- [`patchnotes.md`](../patchnotes.md) — release-by-release detail; v0.7.x is the hand-rolled Org parser arc, v0.8.0 stamped Phase 16, v0.10.x is Phase 17 (vault → DB sync), v0.13.x is the inline-syntax + first-boot polish.
- [`docs/regression.md`](regression.md) — what the ship-gate runs.
