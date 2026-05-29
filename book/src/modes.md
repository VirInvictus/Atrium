# Simple & Builder modes

Atrium has two modes over **one** schema. Mode is a view choice, not a
data choice; switching it never touches the database (the
`mode_flip_snapshot` test enforces this).

**Simple Mode** is the default: a Things-3-style three-pane surface.
Visible fields are title, note, schedule (When), deadline, tags, and
the completion checkbox. Defer dates, estimates, the repeat editor,
subtasks, Forecast, Review, and Perspectives are hidden.

**Builder Mode** adds the OmniFocus depth on top of the same rows:

- **Forecast**: a 30-day calendar-axis strip; drag to reschedule.
- **Calendar**: a paper-calendar month grid (sibling lens to Forecast).
- **Review**: projects with a stale review date, oldest first.
- **Perspectives**: saved search expressions, optionally rendered as a kanban board.
- **Inspector pane**: an always-visible editor that autosaves per field, with Subtasks, a "Blocked by" dependency picker, time tracking, and the repeat-rule editor.
- **Defer dates, sequential projects, estimates.**

The **Agenda** canonical page (Overdue / Today / Tomorrow / This Week /
Next Week) shows in both modes.

Switch in *Preferences → General → Default mode*, or persist your
choice across launches. See `spec.md` §5 for the full surface.
