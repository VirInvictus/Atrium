# Importing & exporting

Imports are best-effort: each source has a documented field mapping,
lossy fields surface in a post-import report, and every importer has a
dry-run mode. The GUI's **Import…** dialog (v0.34.0) drives all five
sources with a dry-run preview; the CLI exposes the same paths.

| Source | Format | CLI |
|---|---|---|
| Org-mode | `.org` (two-way vault) | `atrium-cli import org PATH` |
| Todoist | CSV export | `atrium-cli import todoist PATH --into PROJECT` |
| VTODO | `.ics` (Endeavour, Errands, Nextcloud Tasks, Planify) | `atrium-cli import vtodo PATH --into PROJECT` |
| Taskwarrior | `task export` JSON | `atrium-cli import taskwarrior PATH --into PROJECT --uda-as tag\|note\|drop` |
| todo.txt | plain text | `atrium-cli import todotxt PATH --into PROJECT` |

Export targets: Org vault (two-way), a lossless JSON snapshot, and a
one-way VTODO `.ics` dump. Atrium is **not** a CalDAV client; VTODO
export is a file hand-off.

The non-Org importers live in the `atrium-import` crate (shared by the
CLI and the GUI dialog); Org import/export lives in `atrium-org`. Full
mapping tables are in `spec.md` §7.
