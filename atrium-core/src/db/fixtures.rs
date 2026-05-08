// SPDX-License-Identifier: MIT
//! Stress fixture generator (debug harness, spec §3.4).
//!
//! Generates realistically-shaped task data at four scales — Small
//! (1K), Medium (10K), Large (50K), Stress (100K). Used both by
//! integration tests and by the `--fixture <scale>` CLI flag in debug
//! builds. Distribution roughly mirrors a working OmniFocus library:
//! ~20 tasks per project, ~14 % inbox-only tasks, a mix of
//! scheduled / completed / Someday states, and unicode-hostile titles
//! to keep rendering honest.
//!
//! The data is deterministic given the modulo seeds; SQLite's own
//! `RANDOM()` is intentionally avoided so tests are reproducible.

use std::time::Instant;

use rusqlite::{Connection, params};
use tracing::info;
use uuid::Uuid;

use crate::error::DbError;

/// Fixture density preset.
#[derive(Debug, Clone, Copy)]
pub enum FixtureScale {
    /// 1K tasks, 50 projects, 5 areas, 20 tags.
    Small,
    /// 10K tasks, 500 projects, 10 areas, 50 tags.
    Medium,
    /// 50K tasks, 2 500 projects, 20 areas, 100 tags.
    Large,
    /// 100K tasks, 5 000 projects, 30 areas, 200 tags.
    Stress,
}

impl FixtureScale {
    pub fn task_count(self) -> usize {
        match self {
            Self::Small => 1_000,
            Self::Medium => 10_000,
            Self::Large => 50_000,
            Self::Stress => 100_000,
        }
    }

    pub fn area_count(self) -> usize {
        match self {
            Self::Small => 5,
            Self::Medium => 10,
            Self::Large => 20,
            Self::Stress => 30,
        }
    }

    pub fn project_count(self) -> usize {
        // ~20 tasks per project at every scale.
        self.task_count() / 20
    }

    pub fn tag_count(self) -> usize {
        match self {
            Self::Small => 20,
            Self::Medium => 50,
            Self::Large => 100,
            Self::Stress => 200,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "small" => Some(Self::Small),
            "medium" => Some(Self::Medium),
            "large" => Some(Self::Large),
            "stress" => Some(Self::Stress),
            _ => None,
        }
    }
}

/// Outcome summary returned by [`generate`]. Surfaced in `--debug` logs
/// and used in tests to assert generation actually populated the DB.
#[derive(Debug, Clone)]
pub struct FixtureSummary {
    pub areas: usize,
    pub projects: usize,
    pub tasks: usize,
    pub tags: usize,
    pub elapsed_ms: u128,
}

/// Generate fixture data at the given scale into `conn`.
///
/// Wraps everything in a single transaction so the cost of FTS5 trigger
/// fan-out is amortised across the whole batch instead of paying per
/// row. Caller must commit a clean schema (`db::open`-d connection or
/// equivalent) — this function does not run migrations.
pub fn generate(conn: &mut Connection, scale: FixtureScale) -> Result<FixtureSummary, DbError> {
    let start = Instant::now();
    info!(
        ?scale,
        target_tasks = scale.task_count(),
        "generating fixtures"
    );

    let tx = conn.transaction()?;

    // Areas
    let mut area_ids = Vec::with_capacity(scale.area_count());
    for i in 0..scale.area_count() {
        tx.execute(
            "INSERT INTO area (uuid, title, position) VALUES (?, ?, ?)",
            params![Uuid::new_v4().to_string(), area_title(i), (i + 1) as f64],
        )?;
        area_ids.push(tx.last_insert_rowid());
    }

    // Projects — round-robin across areas, ~14 % unfiled (area_id NULL).
    let mut project_ids = Vec::with_capacity(scale.project_count());
    for i in 0..scale.project_count() {
        let area_id = if i % 7 == 0 {
            None
        } else {
            Some(area_ids[i % area_ids.len()])
        };
        tx.execute(
            "INSERT INTO project (uuid, title, area_id, sequential, position) \
             VALUES (?, ?, ?, ?, ?)",
            params![
                Uuid::new_v4().to_string(),
                project_title(i),
                area_id,
                i32::from(i % 9 == 0), // ~11 % sequential
                (i + 1) as f64
            ],
        )?;
        project_ids.push(tx.last_insert_rowid());
    }

    // Tags — mix of plain ASCII, cyrillic, japanese, and emoji-prefixed.
    let mut tag_ids = Vec::with_capacity(scale.tag_count());
    for i in 0..scale.tag_count() {
        tx.execute(
            "INSERT INTO tag (uuid, name) VALUES (?, ?)",
            params![Uuid::new_v4().to_string(), tag_name(i)],
        )?;
        tag_ids.push(tx.last_insert_rowid());
    }

    // Tasks
    for i in 0..scale.task_count() {
        let project_id = if i % 7 == 0 {
            None // ~14 % inbox
        } else {
            Some(project_ids[i % project_ids.len()])
        };

        let scheduled_for: Option<String> = match i % 11 {
            0 => Some("__someday__".to_string()),
            1..=3 => Some(format!("2026-05-{:02}", (i % 28) + 1)),
            _ => None,
        };

        let deadline: Option<String> = match i % 17 {
            0 => Some(format!("2026-06-{:02}", (i % 28) + 1)),
            _ => None,
        };

        let defer_until: Option<String> = match i % 23 {
            0 => Some(format!("2026-05-{:02}", (i % 28) + 1)),
            _ => None,
        };

        let estimated_minutes: Option<i64> = match i % 5 {
            0 => Some(15),
            1 => Some(30),
            2 => Some(60),
            _ => None,
        };

        let completed_at: Option<String> = if i % 13 == 0 {
            Some(format!("2026-04-{:02}T12:00:00.000Z", (i % 28) + 1))
        } else {
            None
        };

        tx.execute(
            "INSERT INTO task \
                (uuid, title, project_id, scheduled_for, deadline, defer_until, \
                 estimated_minutes, completed_at, position) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                Uuid::new_v4().to_string(),
                task_title(i),
                project_id,
                scheduled_for,
                deadline,
                defer_until,
                estimated_minutes,
                completed_at,
                (i + 1) as f64
            ],
        )?;
        let task_id = tx.last_insert_rowid();

        // Tag assignment: ~30 % of tasks get one tag, ~10 % get two.
        if i % 10 < 3 {
            let tag_id = tag_ids[i % tag_ids.len()];
            tx.execute(
                "INSERT OR IGNORE INTO task_tag (task_id, tag_id) VALUES (?, ?)",
                params![task_id, tag_id],
            )?;
        }
        if i % 10 == 0 {
            let tag_id = tag_ids[(i + 7) % tag_ids.len()];
            tx.execute(
                "INSERT OR IGNORE INTO task_tag (task_id, tag_id) VALUES (?, ?)",
                params![task_id, tag_id],
            )?;
        }
    }

    // Slice D fixture — one board-renderer perspective so the
    // kanban subcommand has something to render against `--fixture
    // small`. Uses three tag-prefix columns that overlap the tag
    // pool (tag-0, urgent-3, home-4) so each column gets at least
    // a handful of matches.
    let board_uuid = Uuid::new_v4().to_string();
    tx.execute(
        "INSERT INTO perspective \
         (uuid, name, icon, filter_expr, renderer, renderer_config, position) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![
            board_uuid,
            "Fixture Board",
            "view-grid-symbolic",
            "is:open",
            "board",
            r#"{"axis":"tag","columns":["tag-0","urgent-3","home-4"]}"#,
            10.0_f64,
        ],
    )?;

    tx.commit()?;

    let summary = FixtureSummary {
        areas: scale.area_count(),
        projects: scale.project_count(),
        tasks: scale.task_count(),
        tags: scale.tag_count(),
        elapsed_ms: start.elapsed().as_millis(),
    };
    info!(?summary, "fixture generation complete");
    Ok(summary)
}

fn area_title(i: usize) -> String {
    let pool = [
        "Personal",
        "Work",
        "Home",
        "Side Projects",
        "Reading",
        "Health",
    ];
    format!("{} ({})", pool[i % pool.len()], i + 1)
}

fn project_title(i: usize) -> String {
    let verbs = [
        "Plan", "Ship", "Refactor", "Audit", "Migrate", "Triage", "Draft",
    ];
    let nouns = [
        "dashboard",
        "import flow",
        "схема",
        "ドキュメント",
        "Q3 review",
        "API",
    ];
    format!(
        "{} the {} #{}",
        verbs[i % verbs.len()],
        nouns[i % nouns.len()],
        i + 1
    )
}

fn task_title(i: usize) -> String {
    match i % 9 {
        0 => format!("Buy {} 🛒", supply_word(i)),
        1 => format!("Review PR #{}", i + 1000),
        2 => format!("研究プロジェクト {}", i),
        3 => format!("Email João about Q{}", (i % 4) + 1),
        4 => format!(
            "Task with a deliberately long title to exercise wrapping behaviour and \
             list-row layout when text exceeds a reasonable terminal column count #{}",
            i
        ),
        5 => format!("⏰ Reminder: {} #{}", reminder_action(i), i),
        6 => format!("Файл переименовать {}", i),
        7 => format!("[empty placeholder {}]", i),
        _ => format!("Task #{}", i),
    }
}

fn supply_word(i: usize) -> &'static str {
    let words = [
        "milk",
        "bread",
        "coffee",
        "пельмени",
        "焼きそば",
        "groceries",
    ];
    words[i % words.len()]
}

fn reminder_action(i: usize) -> &'static str {
    let actions = ["call dentist", "water plants", "back up disks", "stretch"];
    actions[i % actions.len()]
}

fn tag_name(i: usize) -> String {
    match i % 5 {
        0 => format!("tag-{}", i),
        1 => format!("работа-{}", i),
        2 => format!("作業-{}", i),
        3 => format!("urgent-{}", i),
        _ => format!("home-{}", i),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use rusqlite::Connection;

    fn fresh_db() -> Connection {
        let mut conn = Connection::open_in_memory().unwrap();
        db::configure_pragmas(&conn).unwrap();
        super::super::migrations::migrate(&mut conn).unwrap();
        conn
    }

    #[test]
    fn small_scale_generates_expected_counts() {
        let mut conn = fresh_db();
        let summary = generate(&mut conn, FixtureScale::Small).unwrap();
        assert_eq!(summary.tasks, 1_000);
        assert_eq!(summary.projects, 50);
        assert_eq!(summary.areas, 5);
        assert_eq!(summary.tags, 20);

        let task_count: i64 = conn
            .query_row("SELECT count(*) FROM task", [], |r| r.get(0))
            .unwrap();
        assert_eq!(task_count, 1_000);
    }

    #[test]
    fn fts_index_populates_for_fixtures() {
        let mut conn = fresh_db();
        generate(&mut conn, FixtureScale::Small).unwrap();
        // Search for a token we know the generator emits ("Buy" appears in
        // every 9th task title).
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM task_fts WHERE task_fts MATCH 'buy'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(count > 50, "expected >50 'buy' matches, got {count}");
    }

    #[test]
    fn someday_sentinel_present() {
        let mut conn = fresh_db();
        generate(&mut conn, FixtureScale::Small).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT count(*) FROM task WHERE scheduled_for = '__someday__'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(count > 0, "expected Someday-scheduled tasks");
    }

    #[test]
    fn parse_round_trips() {
        assert!(matches!(
            FixtureScale::parse("small"),
            Some(FixtureScale::Small)
        ));
        assert!(matches!(
            FixtureScale::parse("MEDIUM"),
            Some(FixtureScale::Medium)
        ));
        assert!(FixtureScale::parse("nonsense").is_none());
    }
}
