# GTD patterns for Atrium

Atrium isn't a GTD app — it's a task manager that *supports* GTD if you want to work that way. This page documents the conventions Atrium users have settled on for the GTD-shaped workflows the schema doesn't model directly. Most are tag idioms; some are search-expression recipes; one is the seeded **Weekly Review** Perspective.

If you're new to GTD, the canonical reference is David Allen's *Getting Things Done*. The notes below assume you already know what "next action," "context," and "weekly review" mean.

---

## Waiting on someone (`#waiting`)

GTD's "Waiting For" list is for things blocked on other people. Atrium doesn't ship a dedicated list for this — instead, the convention is a `#waiting` tag.

**Capture pattern:**

```text
Email Q3 budget approval from Sam @today #waiting
```

**Search recipe:**

```text
tag:waiting AND is:open
```

Save that as a Perspective named "Waiting For" and it surfaces in the Builder-mode sidebar. The seeded Weekly Review perspective intentionally doesn't filter `#waiting` — it includes them so you remember to follow up at review time.

**Tip:** put the deadline on the *follow-up date* (when you'll nudge), not the date you expect the other person to deliver. That way the task surfaces in Today the day you should chase rather than the day you're nominally blocked until.

---

## Delegated tasks (`#delegated`)

When you've handed something off but still want it on your radar, use `#delegated`. Same shape as `#waiting`, different intent — `#delegated` means "I'm tracking this; not actively chasing." Distinct from `#waiting` because the search recipes differ:

```text
tag:delegated AND is:open                  # all delegated, low-noise
tag:delegated AND scheduled:thismonth      # delegated tasks scheduled to land this month
```

You can use both tags on the same task — `#delegated #waiting` means "I delegated it AND I'm now waiting on a follow-up."

---

## Blocked by something else (`#blocked`)

For tasks whose blocker is a thing rather than a person — a dependency, a PR review, a system not yet provisioned. Same idiom; the tag separates "blocked by X" from "delegated to Y" so weekly-review triage knows what to do with each.

```text
tag:blocked AND is:open       # what's stuck
```

When the blocker resolves, drop the tag (`atrium-cli edit ID --untag blocked`) and the task rejoins the regular flow.

---

## Someday / maybe

Atrium ships a Someday list for this — `is:someday` is the canonical predicate. Tasks marked Someday don't appear in Today, Anytime, or Forecast; they live in their own list until you bring them back.

**Capture pattern:**

```text
Learn Welsh #language @someday
```

**Promote a Someday task to active:**

In the GUI, drag it out of Someday or use the schedule picker to set a real date. From the CLI:

```bash
atrium-cli edit 42 --scheduled today
```

`is:someday` and `is:open` are mutually exclusive on the search side: if you want "everything I might do," use `is:someday OR is:anytime`.

---

## Weekly review

The Weekly Review Perspective is seeded on first install (you can rename, retune, or delete it freely). It uses this filter:

```text
is:overdue OR scheduled:thisweek OR (is:deadline AND due:nextweek) OR (is:deferred AND defer:<=today)
```

The pieces:

- `is:overdue` — anything with a deadline already past. Triage first.
- `scheduled:thisweek` — what you said you'd do this week.
- `is:deadline AND due:nextweek` — what's due in the heads-up window. Decide if it's a *this-week* task even though the deadline is ahead.
- `is:deferred AND defer:<=today` — defers that just expired. Anything deferred until "now" needs a fresh decision.

**Workflow:**

1. Open the Weekly Review Perspective.
2. For each task: do it, defer it, delegate it (add `#delegated`), or kill it.
3. When the list reads as *only* future-scheduled work and conscious deferrals, the review is done.

If the seeded filter doesn't fit your habits, edit the Perspective in place (right-click → Rename) or duplicate it via the CLI:

```bash
# Capture your current filter as a fresh Perspective
atrium-cli list perspectives --json | jq '.[] | select(.name == "Weekly Review")'
```

---

## Tag-as-context

GTD contexts (`@home`, `@phone`, `@calls`) are tags in Atrium. There's no separate `context` field — tags do the job, and they compose with everything else search can do.

```text
tag:home AND is:today          # what can I do here right now
tag:phone AND is:open          # everything that needs a call
tag:errand AND scheduled:thisweek
```

**Tip:** keep contexts and projects orthogonal. Project = "what is this for"; tag = "what does it require." A "Buy milk" task lives in the **Errands** project (or Inbox) and carries the `#errand` tag — searchable by either lens.

---

## Areas as life domains

Atrium's `area` is the GTD "area of focus." A typical setup: **Work**, **Personal**, **Health**, **Home**, plus one or two project-rich domains (e.g., a side project). Areas are not tags — they're a single-parent grouping for projects.

The `area.color` field (v0.5.0) lets you tint each area; the colour propagates to the row's left edge so cross-list views (Today, Forecast) show at a glance which area a task belongs to without you reading the chip text.

**Power move:** save a Perspective per area for "everything in [area]" — `area:Work AND is:open` — to slice the Today / Anytime / Upcoming sets through that lens.

---

## Repeating tasks for habits

Atrium's repeating tasks (Phase 15) are RFC 5545 RRULE under the hood with three Org-style completion semantics. The gist:

- **Cumulative (default).** Skip ahead until the next occurrence is in the future. Fits "every Monday" — if you miss two weeks, the next instance lands on the *next* upcoming Monday, not the one you missed.
- **Next-from-completion.** Anchor on when you finished, ignore the previous schedule. Right for "every N after I last did this" (haircut, oil change, exercise habits).
- **Always shift by interval (Basic).** Always shift exactly one rule increment from the previous anchor, even if the result lands in the past. Rare; included for round-trip fidelity with Org files.

**Habit pattern:** create a task with `repeat = weekly`, mode = *next from completion*, schedule = today. Each time you finish, the next instance arrives with a fresh date. The Logbook keeps the history (visible per-day under Today / Yesterday / Last 7 Days / Older).

---

## Mode switching

Simple Mode hides the Builder fields (`defer_until`, sequential projects, repeating rules, perspectives). Switching modes never touches data — see `tests/mode_flip_snapshot.rs` for the contract.

**When to switch to Simple Mode:** when the system is doing the work for you (Today list is right, your projects are well-shaped, defer dates already match reality). Simple Mode hides the dials so you don't tinker.

**When to switch to Builder Mode:** during the weekly review, when shaping a new project, when something needs a defer / repeat rule, or when you want to use the Inspector pane to see every field at once.

It's not a one-way flip — power users live in Builder Mode for triage and Simple Mode for execution. The mode is a preference, not a setting that changes behaviour beyond which fields are visible.

---

## Quick reference

| Workflow | Pattern |
|---|---|
| Waiting on a person | `#waiting` tag, deadline on the *follow-up* date |
| Delegated and tracking | `#delegated` tag |
| Blocked by something | `#blocked` tag — drop when unstuck |
| Someday / maybe | `@someday` capture or `is:someday` predicate |
| Weekly review | Seeded **Weekly Review** Perspective |
| Contexts | Tags: `#home`, `#phone`, `#errand`, `#calls` |
| Life domains | Areas (with optional accent colour) |
| Habits | Repeating tasks with *next from completion* |

For the search expression language, see `spec.md` §4.3. For the schema and the Builder-vs-Simple split, see `spec.md` §3 and §5.
