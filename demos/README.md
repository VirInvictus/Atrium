# Atrium demos

Hand-crafted artifacts for showing Atrium off and exercising
the data layer with realistic shapes. Not on the install /
distribution path — these live alongside the code so the
in-tree story stays one step from the CLI.

## `showcase/` — the Org-mode conversion in action

Three projects across two areas, deliberately rich:

```
showcase/
├── Q3-Launch.org              ← unfiled (lands in Inbox → root)
├── Personal/
│   └── Reading-list.org       ← Personal area
└── Work/
    └── On-call-rotation.org   ← Work area
```

What it exercises:

- **Every TODO-cycle keyword.** TODO / DONE / CANCELLED
  (canonical) plus IN-PROGRESS / WAITING / BLOCKED
  (non-canonical via `task.orig_keyword`).
- **Every cookie combination.** SCHEDULED, DEADLINE, CLOSED,
  and the seven non-empty subsets thereof.
- **All three repeater modes.** Basic (`+1w`), Cumulative
  (`++1w`, the Atrium default), and Next-from-completion
  (`.+1w`). Plus a multi-day RRULE (`BYDAY=MO,WE,FR`) where
  the SCHEDULED cookie is best-fit projection and the
  `:RRULE:` property is canonical.
- **Subtask hierarchies.** Mostly two levels deep; one task
  goes four levels deep to show the parent_id chain holding
  up.
- **File-level project metadata.** `#+TITLE:` plus a
  top-level `:PROPERTIES:` drawer carrying `:ID:`,
  `:SEQUENTIAL:`, `:REVIEW_INTERVAL:`.
- **Body content with Org constructs.** Source blocks, a
  table, bullet lists, external + internal links — all
  preserved verbatim per spec §7.3.3 rule 1.
- **Multi-tag headlines.** `:tag1:tag2:tag3:` with three
  or more tags in many places.
- **Unicode.** Japanese, Cyrillic, emoji, and an
  RTL-display test string.

## Running it

From the workspace root:

```bash
# (Optional) start from a fresh DB + vault.
rm -rf ~/Tasks ~/.local/share/atrium/atrium.db*
mkdir -p ~/Tasks
gsettings set io.github.virinvictus.atrium vault-path ~/Tasks

# Import the showcase. Directory walk: subdirs become areas,
# .org files become projects.
cargo run -p atrium-cli -- import org demos/showcase/

# Boot the GUI. The fresh-vault seed (v0.13.5) mirrors every
# project to ~/Tasks the moment the data layer attaches.
cargo run -p atrium
```

After that, `~/Tasks/` has the projects mirrored back as
`.org` files — same content, canonical Atrium emit format.
Open any of them in DoomEmacs to see how the round-trip
looks; edit a task, save, and watch Atrium pick the change
up in ~200 ms via the `inotify` watcher.

## Reset

```bash
rm -rf ~/Tasks ~/.local/share/atrium/atrium.db*
gsettings reset io.github.virinvictus.atrium vault-path
```
