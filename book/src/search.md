# Search expressions

The search bar and saved Perspectives speak a Calibre-shaped boolean
expression language (`atrium-search`). The same text is stored verbatim
in a Perspective, so a saved query inherits future grammar additions.

A quick taste:

```text
tag:work AND is:overdue sort:-due
project:"Q3 plans" AND !is:done
title:~^Re: OR note:invoice
due:thisweek defer:<=today
is:blocked
```

- **Boolean composition:** `AND` (implicit between terms), `OR`, `NOT` / `!`, parentheses. Precedence is `NOT > AND > OR`.
- **Fields:** `tag:` `area:` `project:` `title:` `note:` `due:` `scheduled:` `defer:` `created:` `modified:` `completed:` `estimated:` `repeats:`.
- **Match modifiers** on text fields: substring (default), `=exact`, `~regex`, `?fuzzy`, `true`/`false`.
- **Comparisons + ranges** on dates/numbers: `>`, `<=`, `lo..hi`, plus date keywords (`today`, `thisweek`, `Ndaysago`, …).
- **State predicates:** `is:open` `is:done` `is:overdue` `is:today` `is:blocked` `is:available` `is:tagged` … (each negatable with `!is:`).
- **Sorting:** `sort:KEY` / `sort:-KEY`, composable.

Unknown tokens fall through to free text with a warning, never an
error. The full grammar, every field, and the SQL-translation
fast-path are in `spec.md` §4.3–§4.5.
