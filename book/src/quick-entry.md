# Quick Entry & inline syntax

`Ctrl+Alt+Space` opens Quick Entry, a small modal that drops a task
into the Inbox. The same inline vocabulary works in the bottom-of-list
entry, inline row rename (double-click / F2), and the CLI `capture`
subcommand — one parser (`atrium-inline`) drives them all.

| Token | Effect |
|---|---|
| `#tag` | attach a tag (created on first use; case-insensitive) |
| `@today` / `@tomorrow` / `@someday` | set the schedule |
| `@2026-05-15` | schedule a specific date |
| `@mon` / `@monday` | schedule the next occurrence of that weekday |
| `@deadline 2026-05-15` | set a deadline |
| `!1` / `!2` / `!3` | priority (high / medium / low), projected to a `priority-N` tag |

`Enter` commits, `Esc` discards. A tab-completion popover suggests
tags, schedule keywords, and priorities as you type. Quick Entry
doesn't steal focus from the window you were in.

Drop a file or URL onto the window (v0.30.0) and Quick Entry opens
pre-filled. See `spec.md` §6 for the full grammar.
