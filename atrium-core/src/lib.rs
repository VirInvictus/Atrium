// SPDX-License-Identifier: MIT
//! Headless core for the Atrium task manager.
//!
//! Hosts the SQLite worker, domain types, XDG path helpers, and the
//! shared error hierarchy. No GTK or GUI dependencies — anything that
//! wants Atrium's data layer (the binary, the future `atriumd` capture
//! daemon, the eventual `atrium-tui` frontend, integration tests)
//! depends on `atrium-core` directly.

pub mod db;
pub mod domain;
pub mod error;
pub mod paths;
pub mod repeat;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use db::changes::{LibraryChanges, TaskChanges};
pub use db::worker::{WorkerHandle, spawn as spawn_worker};
pub use domain::{
    Area, AreaUpdate, Heading, NewArea, NewPerspective, NewProject, NewTag, NewTask, Perspective,
    PerspectiveUpdate, Project, ProjectUpdate, ScheduledFor, Tag, TagUpdate, Task, TaskUpdate,
};
pub use error::{CoreError, DbError, DomainError};
pub use paths::{APP_ID, cache_dir, data_dir, db_path};
pub use repeat::{RepeatMode, RepeatRule};
