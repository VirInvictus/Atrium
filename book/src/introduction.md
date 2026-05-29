# Atrium

Atrium is a native GNOME task manager. It pairs **Org-mode internals**
(stable UUIDs, a plain-text round-trip, three repeater semantics) with
a **Things 3 / OmniFocus surface** (a calm Simple Mode and a deeper
Builder Mode over one OmniFocus-superset schema). It is local-first:
SQLite in WAL mode behind a single-writer worker, no network sync, no
telemetry.

This handbook is the reader's guide. The authoritative contract lives
in the repository:

- [`spec.md`](https://github.com/VirInvictus/Atrium/blob/main/spec.md): architecture, schema, search grammar, import/export mapping, perf budget.
- [`roadmap.md`](https://github.com/VirInvictus/Atrium/blob/main/roadmap.md): what shipped and what's next.
- [`patchnotes.md`](https://github.com/VirInvictus/Atrium/blob/main/patchnotes.md): release notes, newest first.

The **Guide** chapters are a quick on-ramp; the **Reference** chapters
(keyboard map, schema, accessibility, performance) are the canonical
`docs/` files, served here verbatim.
