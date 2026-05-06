// SPDX-License-Identifier: MIT
//! `TaskChanges` — coalesced batch of task mutations delivered to UI
//! subscribers. Per spec §3.2: "UI updates apply as deltas, never full
//! reloads." This is the unit of delivery.

use serde::{Deserialize, Serialize};

use crate::domain::{Area, Project, Tag, Task};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TaskChanges {
    /// Tasks that were just created. Full row, ready for the UI to
    /// render.
    pub created: Vec<Task>,

    /// Tasks whose non-status fields were modified. Full row.
    pub updated: Vec<Task>,

    /// IDs of tasks that were removed.
    pub deleted: Vec<i64>,

    /// IDs of tasks whose `completed_at` flipped (open ↔ done). The
    /// row also appears in `updated` so the UI gets the new completion
    /// timestamp; `status_changed` is the signal that the *list
    /// membership* may have changed (Today → Logbook, etc.).
    pub status_changed: Vec<i64>,
}

impl TaskChanges {
    /// `true` when no change is carried.
    pub fn is_empty(&self) -> bool {
        self.created.is_empty()
            && self.updated.is_empty()
            && self.deleted.is_empty()
            && self.status_changed.is_empty()
    }

    /// Fold `other` into `self`. Used by the worker's coalescer when
    /// a single command produces multiple change rows, and by future
    /// time-debounced batching at the UI side.
    pub fn merge(&mut self, mut other: TaskChanges) {
        self.created.append(&mut other.created);
        self.updated.append(&mut other.updated);
        self.deleted.append(&mut other.deleted);
        self.status_changed.append(&mut other.status_changed);
    }

    /// Total number of affected rows across all categories. Tasks
    /// that appear in both `updated` and `status_changed` count twice.
    pub fn len(&self) -> usize {
        self.created.len() + self.updated.len() + self.deleted.len() + self.status_changed.len()
    }
}

/// Coalesced batch of library-shape mutations — areas, projects, and
/// (Phase 5.5) headings. Delivered on a separate channel from
/// `TaskChanges` so UI subscribers can pick what they care about
/// (the sidebar listens here; the task list listens on `TaskChanges`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LibraryChanges {
    pub areas_created: Vec<Area>,
    pub areas_updated: Vec<Area>,
    pub areas_deleted: Vec<i64>,
    pub projects_created: Vec<Project>,
    pub projects_updated: Vec<Project>,
    pub projects_deleted: Vec<i64>,
    pub tags_created: Vec<Tag>,
    pub tags_updated: Vec<Tag>,
    pub tags_deleted: Vec<i64>,
}

impl LibraryChanges {
    pub fn is_empty(&self) -> bool {
        self.areas_created.is_empty()
            && self.areas_updated.is_empty()
            && self.areas_deleted.is_empty()
            && self.projects_created.is_empty()
            && self.projects_updated.is_empty()
            && self.projects_deleted.is_empty()
            && self.tags_created.is_empty()
            && self.tags_updated.is_empty()
            && self.tags_deleted.is_empty()
    }

    pub fn merge(&mut self, mut other: LibraryChanges) {
        self.areas_created.append(&mut other.areas_created);
        self.areas_updated.append(&mut other.areas_updated);
        self.areas_deleted.append(&mut other.areas_deleted);
        self.projects_created.append(&mut other.projects_created);
        self.projects_updated.append(&mut other.projects_updated);
        self.projects_deleted.append(&mut other.projects_deleted);
        self.tags_created.append(&mut other.tags_created);
        self.tags_updated.append(&mut other.tags_updated);
        self.tags_deleted.append(&mut other.tags_deleted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn dummy_task(id: i64) -> Task {
        Task {
            id,
            uuid: format!("uuid-{id}"),
            title: format!("task {id}"),
            note: String::new(),
            project_id: None,
            parent_id: None,
            scheduled_for: None,
            deadline: None,
            defer_until: None,
            estimated_minutes: None,
            completed_at: None,
            repeat_rule: None,
            position: id as f64,
            created_at: Utc::now(),
            modified_at: Utc::now(),
        }
    }

    #[test]
    fn empty_default() {
        let c = TaskChanges::default();
        assert!(c.is_empty());
        assert_eq!(c.len(), 0);
    }

    #[test]
    fn merge_concatenates() {
        let mut a = TaskChanges {
            created: vec![dummy_task(1)],
            ..Default::default()
        };
        let b = TaskChanges {
            updated: vec![dummy_task(2)],
            deleted: vec![3],
            status_changed: vec![2],
            ..Default::default()
        };
        a.merge(b);
        assert_eq!(a.created.len(), 1);
        assert_eq!(a.updated.len(), 1);
        assert_eq!(a.deleted, vec![3]);
        assert_eq!(a.status_changed, vec![2]);
        assert_eq!(a.len(), 4);
    }
}
