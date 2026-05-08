// SPDX-License-Identifier: MIT
//! Unit tests for the pure-Rust pieces of atrium-cli — argv parsing
//! and output formatting. The DB-touching paths (run_search /
//! run_list / run_info) are covered end-to-end by the regression
//! script; testing them here would require spinning up a fixture
//! database, which is more painful than scripting `atrium-cli` for
//! the same coverage.

use crate::args::{AddArgs, EditArgs, EditProject, Format, Subcommand, parse};
use crate::output::{Row, format_row, format_rows, format_rows_human, row_to_json, rows_to_json};

fn s(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

// ── Argv parsing ────────────────────────────────────────────────

#[test]
fn parse_no_args_returns_empty_args() {
    let a = parse(&s(&[])).unwrap();
    assert!(a.subcommand.is_none());
    assert!(!a.show_help);
}

#[test]
fn parse_help_flag_short_and_long() {
    assert!(parse(&s(&["-h"])).unwrap().show_help);
    assert!(parse(&s(&["--help"])).unwrap().show_help);
}

#[test]
fn parse_version_flag() {
    assert!(parse(&s(&["-V"])).unwrap().show_version);
    assert!(parse(&s(&["--version"])).unwrap().show_version);
}

#[test]
fn parse_format_flags_default_tsv() {
    assert_eq!(parse(&s(&[])).unwrap().format, Format::Tsv);
    assert_eq!(parse(&s(&["--json"])).unwrap().format, Format::Json);
    assert_eq!(parse(&s(&["--human"])).unwrap().format, Format::Human);
    assert_eq!(parse(&s(&["--tsv"])).unwrap().format, Format::Tsv);
}

#[test]
fn parse_db_flag_takes_path() {
    let a = parse(&s(&["--db", "/tmp/test.db"])).unwrap();
    assert_eq!(
        a.db_path.as_ref().map(|p| p.to_string_lossy().to_string()),
        Some("/tmp/test.db".into())
    );
}

#[test]
fn parse_db_flag_missing_path_errors() {
    let err = parse(&s(&["--db"])).unwrap_err();
    assert!(err.contains("--db"));
}

#[test]
fn parse_search_subcommand() {
    let a = parse(&s(&["search", "tag:work"])).unwrap();
    assert_eq!(
        a.subcommand,
        Some(Subcommand::Search {
            expression: "tag:work".into()
        })
    );
}

#[test]
fn parse_search_joins_multiple_words() {
    // Shell may have already split on spaces — atrium-cli rejoins
    // unquoted multi-word expressions so users don't have to think
    // about quoting unless the shell would itself eat the tokens.
    let a = parse(&s(&["search", "tag:work", "AND", "is:overdue"])).unwrap();
    assert_eq!(
        a.subcommand,
        Some(Subcommand::Search {
            expression: "tag:work AND is:overdue".into()
        })
    );
}

#[test]
fn parse_search_with_trailing_format_flag() {
    let a = parse(&s(&["search", "is:overdue", "--json"])).unwrap();
    assert_eq!(a.format, Format::Json);
    assert_eq!(
        a.subcommand,
        Some(Subcommand::Search {
            expression: "is:overdue".into()
        })
    );
}

#[test]
fn parse_search_with_leading_format_flag() {
    let a = parse(&s(&["--json", "search", "tag:work"])).unwrap();
    assert_eq!(a.format, Format::Json);
}

#[test]
fn parse_search_empty_expression_errors() {
    let err = parse(&s(&["search"])).unwrap_err();
    assert!(err.contains("expression required"));
}

#[test]
fn parse_search_with_trailing_db_flag() {
    let a = parse(&s(&["search", "tag:work", "--db", "/tmp/x.db"])).unwrap();
    assert_eq!(
        a.db_path.as_ref().map(|p| p.to_string_lossy().to_string()),
        Some("/tmp/x.db".into())
    );
}

#[test]
fn parse_list_subcommand() {
    let a = parse(&s(&["list", "today"])).unwrap();
    assert_eq!(
        a.subcommand,
        Some(Subcommand::List {
            name: "today".into()
        })
    );
}

#[test]
fn parse_list_missing_name_errors() {
    let err = parse(&s(&["list"])).unwrap_err();
    assert!(err.contains("name"));
}

#[test]
fn parse_info_subcommand() {
    let a = parse(&s(&["info", "42"])).unwrap();
    assert_eq!(a.subcommand, Some(Subcommand::Info { id: 42 }));
}

#[test]
fn parse_info_invalid_id_errors() {
    let err = parse(&s(&["info", "abc"])).unwrap_err();
    assert!(err.contains("invalid task id"));
}

#[test]
fn parse_info_with_human_flag() {
    let a = parse(&s(&["info", "7", "--human"])).unwrap();
    assert_eq!(a.format, Format::Human);
    assert_eq!(a.subcommand, Some(Subcommand::Info { id: 7 }));
}

#[test]
fn parse_unknown_subcommand_errors() {
    let err = parse(&s(&["frobnicate"])).unwrap_err();
    assert!(err.contains("unknown subcommand"));
}

// ── Write subcommands ──────────────────────────────────────────

#[test]
fn parse_add_with_just_a_title() {
    let a = parse(&s(&["add", "Buy milk"])).unwrap();
    let Some(Subcommand::Add(add)) = a.subcommand else {
        panic!("expected Add");
    };
    assert_eq!(add.title, "Buy milk");
    assert!(add.tags.is_empty());
    assert!(add.scheduled.is_none());
}

#[test]
fn parse_add_joins_multi_word_title() {
    // No quotes — shell already split on spaces. atrium-cli rejoins.
    let a = parse(&s(&["add", "Buy", "milk", "and", "eggs"])).unwrap();
    let Some(Subcommand::Add(add)) = a.subcommand else {
        panic!("expected Add");
    };
    assert_eq!(add.title, "Buy milk and eggs");
}

#[test]
fn parse_add_collects_tag_flags() {
    let a = parse(&s(&[
        "add", "Buy milk", "--tag", "errand", "--tag", "grocery",
    ]))
    .unwrap();
    let Some(Subcommand::Add(add)) = a.subcommand else {
        panic!("expected Add");
    };
    assert_eq!(add.tags, vec!["errand".to_string(), "grocery".into()]);
}

#[test]
fn parse_add_picks_up_date_flags() {
    let a = parse(&s(&[
        "add",
        "Buy milk",
        "--scheduled",
        "today",
        "--due",
        "2026-05-20",
        "--defer",
        "tomorrow",
    ]))
    .unwrap();
    let Some(Subcommand::Add(add)) = a.subcommand else {
        panic!("expected Add");
    };
    assert_eq!(add.scheduled, Some("today".into()));
    assert_eq!(add.due, Some("2026-05-20".into()));
    assert_eq!(add.defer, Some("tomorrow".into()));
}

#[test]
fn parse_add_estimated_must_be_integer() {
    let err = parse(&s(&["add", "Buy", "--estimated", "thirty"])).unwrap_err();
    assert!(err.contains("--estimated"));
    assert!(err.contains("integer"));
}

#[test]
fn parse_add_supports_when_alias() {
    // `--when` aliases `--scheduled` to match Atrium's GUI vocab.
    let a = parse(&s(&["add", "Buy", "--when", "today"])).unwrap();
    let Some(Subcommand::Add(add)) = a.subcommand else {
        panic!("expected Add");
    };
    assert_eq!(add.scheduled, Some("today".into()));
}

#[test]
fn parse_add_supports_deadline_alias() {
    let a = parse(&s(&["add", "Buy", "--deadline", "today"])).unwrap();
    let Some(Subcommand::Add(add)) = a.subcommand else {
        panic!("expected Add");
    };
    assert_eq!(add.due, Some("today".into()));
}

#[test]
fn parse_add_with_format_flag_anywhere() {
    // Global flags can appear interleaved with the title / flags.
    let a = parse(&s(&["add", "Buy", "milk", "--json"])).unwrap();
    assert_eq!(a.format, Format::Json);
}

#[test]
fn parse_add_empty_title_errors() {
    let err = parse(&s(&["add", "--tag", "errand"])).unwrap_err();
    assert!(err.contains("title"));
}

#[test]
fn parse_add_unknown_flag_errors() {
    let err = parse(&s(&["add", "Buy", "--frobulate", "x"])).unwrap_err();
    assert!(err.contains("unknown flag"));
}

#[test]
fn parse_capture_single_line() {
    let a = parse(&s(&["capture", "Buy milk #errand @today"])).unwrap();
    assert_eq!(
        a.subcommand,
        Some(Subcommand::Capture {
            line: "Buy milk #errand @today".into()
        })
    );
}

#[test]
fn parse_capture_joins_words() {
    // Shell already split — atrium-cli rejoins.
    let a = parse(&s(&["capture", "Buy", "milk", "#errand", "@today"])).unwrap();
    assert_eq!(
        a.subcommand,
        Some(Subcommand::Capture {
            line: "Buy milk #errand @today".into()
        })
    );
}

#[test]
fn parse_capture_with_format_flag() {
    let a = parse(&s(&["capture", "Buy milk #errand", "--json"])).unwrap();
    assert_eq!(a.format, Format::Json);
    assert!(matches!(a.subcommand, Some(Subcommand::Capture { .. })));
}

#[test]
fn parse_capture_empty_errors() {
    let err = parse(&s(&["capture"])).unwrap_err();
    assert!(err.contains("capture"));
}

// ── Edit subcommand ───────────────────────────────────────────

#[test]
fn parse_edit_with_no_flags_is_noop_shape() {
    // `edit ID` with no flags is allowed — run_edit returns the
    // unchanged row so users can use it as a "show single task"
    // alternative to `info`.
    let a = parse(&s(&["edit", "42"])).unwrap();
    let Some(Subcommand::Edit { id, edit }) = a.subcommand else {
        panic!("expected Edit");
    };
    assert_eq!(id, 42);
    assert_eq!(edit, EditArgs::default());
}

#[test]
fn parse_edit_invalid_id_errors() {
    let err = parse(&s(&["edit", "abc"])).unwrap_err();
    assert!(err.contains("invalid task id"));
}

#[test]
fn parse_edit_title_and_note() {
    let a = parse(&s(&[
        "edit",
        "42",
        "--title",
        "Buy milk and eggs",
        "--note",
        "before 6pm",
    ]))
    .unwrap();
    let Some(Subcommand::Edit { id: _, edit }) = a.subcommand else {
        panic!("expected Edit");
    };
    assert_eq!(edit.title, Some("Buy milk and eggs".into()));
    assert_eq!(edit.note, Some("before 6pm".into()));
}

#[test]
fn parse_edit_project_named() {
    let a = parse(&s(&["edit", "42", "--project", "Q3 plans"])).unwrap();
    let Some(Subcommand::Edit { id: _, edit }) = a.subcommand else {
        panic!("expected Edit");
    };
    assert_eq!(edit.project, Some(EditProject::Named("Q3 plans".into())));
}

#[test]
fn parse_edit_inbox_via_project_keyword() {
    // --project inbox routes to EditProject::Inbox, same as --inbox.
    let a = parse(&s(&["edit", "42", "--project", "inbox"])).unwrap();
    let Some(Subcommand::Edit { id: _, edit }) = a.subcommand else {
        panic!("expected Edit");
    };
    assert_eq!(edit.project, Some(EditProject::Inbox));
}

#[test]
fn parse_edit_inbox_via_short_flag() {
    let a = parse(&s(&["edit", "42", "--inbox"])).unwrap();
    let Some(Subcommand::Edit { id: _, edit }) = a.subcommand else {
        panic!("expected Edit");
    };
    assert_eq!(edit.project, Some(EditProject::Inbox));
}

#[test]
fn parse_edit_clear_field_via_none_keyword() {
    let a = parse(&s(&["edit", "42", "--due", "none"])).unwrap();
    let Some(Subcommand::Edit { id: _, edit }) = a.subcommand else {
        panic!("expected Edit");
    };
    assert_eq!(edit.due, Some("none".into()));
}

#[test]
fn parse_edit_estimated_validates_integer() {
    // Anything that isn't `none` must parse as i64 at parse-time.
    let err = parse(&s(&["edit", "42", "--estimated", "thirty"])).unwrap_err();
    assert!(err.contains("--estimated"));
    assert!(err.contains("integer"));
}

#[test]
fn parse_edit_estimated_accepts_none_keyword() {
    let a = parse(&s(&["edit", "42", "--estimated", "none"])).unwrap();
    let Some(Subcommand::Edit { id: _, edit }) = a.subcommand else {
        panic!("expected Edit");
    };
    assert_eq!(edit.estimated, Some("none".into()));
}

#[test]
fn parse_edit_modify_alias() {
    let a = parse(&s(&["modify", "42", "--inbox"])).unwrap();
    assert!(matches!(
        a.subcommand,
        Some(Subcommand::Edit { id: 42, .. })
    ));
}

#[test]
fn parse_edit_with_format_flag() {
    let a = parse(&s(&["edit", "42", "--due", "tomorrow", "--json"])).unwrap();
    assert_eq!(a.format, Format::Json);
}

#[test]
fn parse_complete_takes_id() {
    let a = parse(&s(&["complete", "42"])).unwrap();
    assert_eq!(a.subcommand, Some(Subcommand::Complete { id: 42 }));
}

#[test]
fn parse_complete_aliases() {
    // `done` and `toggle` route to the same Complete branch.
    assert_eq!(
        parse(&s(&["done", "7"])).unwrap().subcommand,
        Some(Subcommand::Complete { id: 7 })
    );
    assert_eq!(
        parse(&s(&["toggle", "7"])).unwrap().subcommand,
        Some(Subcommand::Complete { id: 7 })
    );
}

#[test]
fn parse_delete_takes_id() {
    let a = parse(&s(&["delete", "42"])).unwrap();
    assert_eq!(a.subcommand, Some(Subcommand::Delete { id: 42 }));
}

#[test]
fn parse_delete_rm_alias() {
    assert_eq!(
        parse(&s(&["rm", "9"])).unwrap().subcommand,
        Some(Subcommand::Delete { id: 9 })
    );
}

#[test]
fn parse_complete_invalid_id_errors() {
    let err = parse(&s(&["complete", "not-a-number"])).unwrap_err();
    assert!(err.contains("invalid task id"));
}

#[test]
fn add_args_default_is_empty() {
    let add = AddArgs::default();
    assert!(add.title.is_empty());
    assert!(add.tags.is_empty());
    assert!(add.scheduled.is_none());
}

#[test]
fn parse_unknown_global_flag_errors() {
    let err = parse(&s(&["--frobulate"])).unwrap_err();
    assert!(err.contains("unknown flag"));
}

// ── Output formatting ──────────────────────────────────────────

fn sample_row() -> Row {
    Row {
        id: 42,
        status: "open".into(),
        title: "Buy milk".into(),
        scheduled: "2026-05-15".into(),
        deadline: "2026-05-20".into(),
        project: "Groceries".into(),
        area: "Personal".into(),
        tags: "errand,grocery".into(),
    }
}

#[test]
fn format_row_emits_eight_tsv_fields() {
    let row = sample_row();
    let line = format_row(&row);
    let fields: Vec<&str> = line.split('\t').collect();
    assert_eq!(fields.len(), 8);
    assert_eq!(fields[0], "42");
    assert_eq!(fields[1], "open");
    assert_eq!(fields[2], "Buy milk");
    assert_eq!(fields[7], "errand,grocery");
}

#[test]
fn format_rows_emits_header_first() {
    let out = format_rows(&[sample_row()]);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("id\t"));
    assert!(lines[0].contains("\ttitle\t"));
    assert!(lines[0].ends_with("\ttags"));
}

#[test]
fn format_row_sanitises_tabs_in_title() {
    let mut row = sample_row();
    row.title = "Buy\tmilk".into();
    let line = format_row(&row);
    let fields: Vec<&str> = line.split('\t').collect();
    // Embedded tab should have been converted to a space so the
    // column count stays at 8.
    assert_eq!(fields.len(), 8);
    assert_eq!(fields[2], "Buy milk");
}

#[test]
fn rows_to_json_round_trips_to_array() {
    let json = rows_to_json(&[sample_row()]);
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.is_array());
    let arr = parsed.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["id"], 42);
    assert_eq!(arr[0]["title"], "Buy milk");
}

#[test]
fn row_to_json_emits_object() {
    let json = row_to_json(&sample_row());
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed.is_object());
    assert_eq!(parsed["id"], 42);
}

#[test]
fn format_rows_human_handles_empty_input() {
    let out = format_rows_human(&[]);
    assert!(out.contains("no matches"));
}

#[test]
fn format_rows_human_includes_id_status_and_title() {
    let out = format_rows_human(&[sample_row()]);
    assert!(out.contains("42"));
    assert!(out.contains("open"));
    assert!(out.contains("Buy milk"));
}

#[test]
fn format_rows_human_truncates_long_titles() {
    let mut row = sample_row();
    row.title = "x".repeat(120);
    let out = format_rows_human(&[row]);
    // Truncation marker — "…" — should appear once the title would
    // exceed 60 visible chars.
    assert!(out.contains("…"));
}
