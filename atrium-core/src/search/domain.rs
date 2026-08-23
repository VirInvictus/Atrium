use std::fmt;
use vir_search::ast::{FieldType, ParseField, ParseSort, ParseState};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Due,
    Scheduled,
    Defer,
    Created,
    Modified,
    Completed,
    Estimated,
    Title,
    Position,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// `tag:` / `tags:` (alias). Match is against the task's tag
    /// names; supports all five MatchKind variants.
    Tag,
    /// `area:` — area title via project.area_id lookup.
    Area,
    /// `project:` — project title.
    Project,
    /// `title:` — task title only (column-scoped FTS5).
    Title,
    /// `note:` — task note only (column-scoped FTS5).
    Note,
    /// `due:` / `deadline:` (alias). Date field.
    Due,
    /// `scheduled:` — date field.
    Scheduled,
    /// `defer:` / `defer_until:` — date field.
    Defer,
    /// `created:` — datetime field; truncated to date for matching.
    Created,
    /// `modified:` — datetime field; same.
    Modified,
    /// `completed:` — datetime field; same.
    Completed,
    /// `estimated:` / `est:` — numeric (minutes).
    Estimated,
    /// `repeats:` — boolean (has a repeat_rule). Only `:true` / `:false`.
    Repeats,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Open — completed_at IS NULL.
    Open,
    /// Done — completed_at IS NOT NULL.
    Done,
    /// Overdue — open AND deadline < today.
    Overdue,
    /// Has a scheduled_for date.
    Scheduled,
    /// Has a deadline.
    Deadline,
    /// Has a defer_until in the future.
    Deferred,
    /// Has a repeat_rule.
    Repeating,
    /// Belongs to a project whose archived_at IS NOT NULL.
    Archived,
    /// In the Logbook (synonym for Done).
    Logbook,
    /// Has a project_id.
    InProject,
    /// Belongs (transitively) to an area.
    InArea,
    /// Has at least one tag.
    Tagged,
    /// Sequential project; not the first incomplete task.
    Queued,
    /// v0.29.0 — open AND not blocked by any open prerequisite
    /// (task dependencies). Dependency-only: defer and sequential
    /// state are queried separately (`is:deferred`, etc.).
    Available,
    /// v0.29.0 — open AND blocked by at least one open prerequisite
    /// (task dependencies). A completed task is never blocked.
    Blocked,
    /// v0.4.1 — mirrors the Today list per spec §4.2: open AND
    /// (Schedule ≤ today OR Deadline ≤ today + N) AND defer-resolved.
    /// `N` is `EvalContext::today_deadline_window_days`, default 7
    /// (matches the binary's existing behaviour).
    Today,
    /// v0.4.1 — mirrors the Inbox list: open AND project_id IS NULL.
    Inbox,
    /// v0.4.1 — mirrors the Upcoming list: open AND scheduled_for is
    /// a date strictly in the future.
    Upcoming,
    /// v0.4.1 — mirrors the Anytime list: open AND no scheduled_for
    /// AND defer-resolved.
    Anytime,
    /// v0.4.1 — mirrors the Someday list: open AND scheduled_for ==
    /// Someday sentinel.
    Someday,
}

impl ParseField for Field {
    fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "tag" | "tags" => Some(Self::Tag),
            "area" => Some(Self::Area),
            "project" => Some(Self::Project),
            "title" => Some(Self::Title),
            "note" | "notes" => Some(Self::Note),
            "due" | "deadline" => Some(Self::Due),
            "scheduled" | "when" => Some(Self::Scheduled),
            "defer" | "defer_until" | "deferred" => Some(Self::Defer),
            "created" => Some(Self::Created),
            "modified" | "updated" => Some(Self::Modified),
            "completed" | "done" => Some(Self::Completed),
            "estimated" | "est" | "effort" => Some(Self::Estimated),
            "repeats" | "repeating" => Some(Self::Repeats),
            _ => None,
        }
    }

    fn field_type(&self) -> FieldType {
        match self {
            Self::Estimated => FieldType::Int,
            Self::Due
            | Self::Scheduled
            | Self::Defer
            | Self::Created
            | Self::Modified
            | Self::Completed => FieldType::Date,
            _ => FieldType::String,
        }
    }
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tag => write!(f, "tag"),
            Self::Area => write!(f, "area"),
            Self::Project => write!(f, "project"),
            Self::Title => write!(f, "title"),
            Self::Note => write!(f, "note"),
            Self::Due => write!(f, "due"),
            Self::Scheduled => write!(f, "scheduled"),
            Self::Defer => write!(f, "defer"),
            Self::Created => write!(f, "created"),
            Self::Modified => write!(f, "modified"),
            Self::Completed => write!(f, "completed"),
            Self::Estimated => write!(f, "estimated"),
            Self::Repeats => write!(f, "repeats"),
        }
    }
}

impl ParseState for State {
    fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "open" => Some(Self::Open),
            "done" | "complete" | "completed" => Some(Self::Done),
            "overdue" => Some(Self::Overdue),
            "scheduled" => Some(Self::Scheduled),
            "deadline" => Some(Self::Deadline),
            "deferred" => Some(Self::Deferred),
            "repeating" => Some(Self::Repeating),
            "archived" => Some(Self::Archived),
            "logbook" => Some(Self::Logbook),
            "project" | "in_project" | "inproject" => Some(Self::InProject),
            "area" | "in_area" | "inarea" => Some(Self::InArea),
            "tagged" => Some(Self::Tagged),
            "queued" => Some(Self::Queued),
            "available" => Some(Self::Available),
            "blocked" => Some(Self::Blocked),
            // v0.4.1 — canonical-list mirrors. Each maps `is:NAME` to
            // the same membership query the corresponding sidebar list
            // uses (per spec §4.2).
            "today" => Some(Self::Today),
            "inbox" => Some(Self::Inbox),
            "upcoming" => Some(Self::Upcoming),
            "anytime" => Some(Self::Anytime),
            "someday" => Some(Self::Someday),
            _ => None,
        }
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::Done => write!(f, "done"),
            Self::Overdue => write!(f, "overdue"),
            Self::Scheduled => write!(f, "scheduled"),
            Self::Deadline => write!(f, "deadline"),
            Self::Deferred => write!(f, "deferred"),
            Self::Repeating => write!(f, "repeating"),
            Self::Archived => write!(f, "archived"),
            Self::Logbook => write!(f, "logbook"),
            Self::InProject => write!(f, "project"),
            Self::InArea => write!(f, "area"),
            Self::Tagged => write!(f, "tagged"),
            Self::Queued => write!(f, "queued"),
            Self::Available => write!(f, "available"),
            Self::Blocked => write!(f, "blocked"),
            Self::Today => write!(f, "today"),
            Self::Inbox => write!(f, "inbox"),
            Self::Upcoming => write!(f, "upcoming"),
            Self::Anytime => write!(f, "anytime"),
            Self::Someday => write!(f, "someday"),
        }
    }
}

impl ParseSort for SortKey {
    fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "due" | "deadline" => Some(Self::Due),
            "scheduled" | "when" => Some(Self::Scheduled),
            "defer" | "defer_until" | "deferred" => Some(Self::Defer),
            "created" => Some(Self::Created),
            "modified" | "updated" => Some(Self::Modified),
            "completed" | "done" => Some(Self::Completed),
            "estimated" | "est" | "effort" => Some(Self::Estimated),
            "title" => Some(Self::Title),
            "position" | "manual" => Some(Self::Position),
            _ => None,
        }
    }
}

impl fmt::Display for SortKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Due => write!(f, "due"),
            Self::Scheduled => write!(f, "scheduled"),
            Self::Defer => write!(f, "defer"),
            Self::Created => write!(f, "created"),
            Self::Modified => write!(f, "modified"),
            Self::Completed => write!(f, "completed"),
            Self::Estimated => write!(f, "estimated"),
            Self::Title => write!(f, "title"),
            Self::Position => write!(f, "position"),
        }
    }
}
