// SPDX-License-Identifier: MIT
//! Filter-expression parser for the search bar (Phase 7d).
//!
//! Splits a user query into freeform text (for FTS5) and structured
//! filter clauses (applied in Rust after the FTS5 hit list comes
//! back). Spec §4.2 / spec §7's filter language is the model.
//!
//! Supported forms:
//!
//! - `tag:NAME` — task must bear the named tag (case-insensitive).
//! - `is:open` — completion state is open (`completed_at IS NULL`).
//! - `is:done` — completion state is done.
//! - `is:overdue` — open task with `deadline < today`.
//! - `due:today` — open task with `deadline == today`.
//!
//! Anything else stays in the freeform text. Unknown `foo:bar`
//! tokens are kept in the title-search query (no silent dropping).

use atrium_core::Task;
use chrono::NaiveDate;

use crate::ui::task_list::TagMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    Tag(String),
    IsOpen,
    IsDone,
    IsOverdue,
    DueToday,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterQuery {
    /// Freeform text passed to FTS5. Empty = "match anything that
    /// passes the structured filters".
    pub text: String,
    pub filters: Vec<Filter>,
}

pub fn parse(input: &str) -> FilterQuery {
    let mut text_parts: Vec<&str> = Vec::new();
    let mut filters: Vec<Filter> = Vec::new();

    for word in input.split_whitespace() {
        // `key:value` shape — try to recognise a known key.
        if let Some((key, value)) = word.split_once(':') {
            let key_lower = key.to_ascii_lowercase();
            match key_lower.as_str() {
                "tag" if !value.is_empty() => {
                    filters.push(Filter::Tag(value.to_string()));
                    continue;
                }
                "is" => match value.to_ascii_lowercase().as_str() {
                    "open" => {
                        filters.push(Filter::IsOpen);
                        continue;
                    }
                    "done" | "completed" | "complete" => {
                        filters.push(Filter::IsDone);
                        continue;
                    }
                    "overdue" => {
                        filters.push(Filter::IsOverdue);
                        continue;
                    }
                    _ => {} // fall through
                },
                "due" => match value.to_ascii_lowercase().as_str() {
                    "today" => {
                        filters.push(Filter::DueToday);
                        continue;
                    }
                    "overdue" => {
                        filters.push(Filter::IsOverdue);
                        continue;
                    }
                    _ => {} // fall through
                },
                _ => {} // unknown prefix — keep in text
            }
        }
        text_parts.push(word);
    }

    FilterQuery {
        text: text_parts.join(" "),
        filters,
    }
}

/// Apply the parsed filters to a task list. Returns only tasks that
/// pass *every* filter (AND semantics).
pub fn apply(
    tasks: Vec<Task>,
    filters: &[Filter],
    tag_map: &TagMap,
    today: NaiveDate,
) -> Vec<Task> {
    if filters.is_empty() {
        return tasks;
    }
    tasks
        .into_iter()
        .filter(|t| {
            filters.iter().all(|f| match f {
                Filter::Tag(name) => tag_map
                    .get(&t.id)
                    .is_some_and(|names| names.iter().any(|n| n.eq_ignore_ascii_case(name))),
                Filter::IsOpen => t.completed_at.is_none(),
                Filter::IsDone => t.completed_at.is_some(),
                Filter::IsOverdue => {
                    t.completed_at.is_none() && t.deadline.is_some_and(|d| d < today)
                }
                Filter::DueToday => {
                    t.completed_at.is_none() && t.deadline.is_some_and(|d| d == today)
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_no_filters() {
        let q = parse("buy milk");
        assert_eq!(q.text, "buy milk");
        assert!(q.filters.is_empty());
    }

    #[test]
    fn tag_filter() {
        let q = parse("tag:errand");
        assert!(q.text.is_empty());
        assert_eq!(q.filters, vec![Filter::Tag("errand".into())]);
    }

    #[test]
    fn tag_with_text() {
        let q = parse("milk tag:errand");
        assert_eq!(q.text, "milk");
        assert_eq!(q.filters, vec![Filter::Tag("errand".into())]);
    }

    #[test]
    fn multiple_filters_and_text() {
        let q = parse("Q3 tag:work is:overdue");
        assert_eq!(q.text, "Q3");
        assert_eq!(q.filters.len(), 2);
        assert!(q.filters.contains(&Filter::Tag("work".into())));
        assert!(q.filters.contains(&Filter::IsOverdue));
    }

    #[test]
    fn is_done_synonyms() {
        for s in ["is:done", "is:completed", "is:complete"] {
            let q = parse(s);
            assert_eq!(q.filters, vec![Filter::IsDone]);
        }
    }

    #[test]
    fn unknown_prefix_stays_in_text() {
        let q = parse("foo:bar baz");
        assert_eq!(q.text, "foo:bar baz");
        assert!(q.filters.is_empty());
    }

    #[test]
    fn due_today_and_due_overdue() {
        assert_eq!(parse("due:today").filters, vec![Filter::DueToday]);
        assert_eq!(parse("due:overdue").filters, vec![Filter::IsOverdue]);
    }

    #[test]
    fn case_insensitive_keys_and_synonyms() {
        let q = parse("TAG:Errand IS:Open");
        assert_eq!(q.filters.len(), 2);
        assert!(q.filters.contains(&Filter::Tag("Errand".into())));
        assert!(q.filters.contains(&Filter::IsOpen));
    }

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    fn dummy_task(id: i64, deadline: Option<NaiveDate>, completed: bool) -> Task {
        use chrono::Utc;
        Task {
            id,
            uuid: format!("u{id}"),
            title: format!("t{id}"),
            note: String::new(),
            project_id: None,
            parent_id: None,
            scheduled_for: None,
            deadline,
            defer_until: None,
            estimated_minutes: None,
            completed_at: completed.then(Utc::now),
            repeat_rule: None,
            position: id as f64,
            created_at: Utc::now(),
            modified_at: Utc::now(),
        }
    }

    #[test]
    fn apply_overdue_keeps_only_open_late() {
        let today = d(2026, 5, 15);
        let tasks = vec![
            dummy_task(1, Some(d(2026, 5, 1)), false),  // overdue
            dummy_task(2, Some(d(2026, 5, 1)), true),   // overdue but done
            dummy_task(3, Some(d(2026, 5, 20)), false), // future
            dummy_task(4, None, false),                 // no deadline
        ];
        let kept = apply(tasks, &[Filter::IsOverdue], &TagMap::new(), today);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, 1);
    }

    #[test]
    fn apply_tag_filter_uses_tag_map() {
        let today = d(2026, 5, 15);
        let tasks = vec![dummy_task(1, None, false), dummy_task(2, None, false)];
        let mut tag_map = TagMap::new();
        tag_map.insert(1, vec!["errand".into()]);
        let kept = apply(tasks, &[Filter::Tag("Errand".into())], &tag_map, today);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, 1);
    }
}
