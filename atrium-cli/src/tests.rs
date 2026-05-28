// SPDX-License-Identifier: MIT
//! Unit tests for the pure-Rust pieces of atrium-cli — argv parsing
//! and output formatting. The DB-touching paths (run_search /
//! run_list / run_info) are covered end-to-end by the regression
//! script; testing them here would require spinning up a fixture
//! database, which is more painful than scripting `atrium-cli` for
//! the same coverage.

use crate::args::{
    AddArgs, EditArgs, EditIcon, EditProject, Format, PerspectiveArgs, PerspectiveSub, Subcommand,
    TargetSpec, parse,
};
use crate::output::{Row, format_row, format_rows, format_rows_human, row_to_json, rows_to_json};

fn s(args: &[&str]) -> Vec<String> {
    args.iter().map(std::string::ToString::to_string).collect()
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
fn parse_edit_tag_add_repeatable() {
    let a = parse(&s(&["edit", "42", "--tag", "urgent", "--tag", "work"])).unwrap();
    let Some(Subcommand::Edit { id: _, edit }) = a.subcommand else {
        panic!("expected Edit");
    };
    assert_eq!(edit.tags_add, vec!["urgent".to_string(), "work".into()]);
    assert!(edit.tags_remove.is_empty());
    assert!(!edit.clear_tags);
    assert!(edit.touches_tags());
}

#[test]
fn parse_edit_remove_tag_repeatable() {
    let a = parse(&s(&[
        "edit",
        "42",
        "--remove-tag",
        "stale",
        "--untag",
        "urgent",
    ]))
    .unwrap();
    let Some(Subcommand::Edit { id: _, edit }) = a.subcommand else {
        panic!("expected Edit");
    };
    assert_eq!(edit.tags_remove, vec!["stale".to_string(), "urgent".into()]);
    assert!(edit.touches_tags());
}

#[test]
fn parse_edit_clear_tags_flag() {
    let a = parse(&s(&["edit", "42", "--clear-tags"])).unwrap();
    let Some(Subcommand::Edit { id: _, edit }) = a.subcommand else {
        panic!("expected Edit");
    };
    assert!(edit.clear_tags);
    assert!(edit.touches_tags());
}

#[test]
fn parse_edit_replace_tags_via_clear_then_add() {
    // The "replace whole set" idiom: --clear-tags --tag X.
    let a = parse(&s(&["edit", "42", "--clear-tags", "--tag", "newtag"])).unwrap();
    let Some(Subcommand::Edit { id: _, edit }) = a.subcommand else {
        panic!("expected Edit");
    };
    assert!(edit.clear_tags);
    assert_eq!(edit.tags_add, vec!["newtag".to_string()]);
}

#[test]
fn touches_tags_false_when_no_tag_flags() {
    let edit = EditArgs::default();
    assert!(!edit.touches_tags());
}

#[test]
fn parse_complete_takes_id() {
    let a = parse(&s(&["complete", "42"])).unwrap();
    assert_eq!(
        a.subcommand,
        Some(Subcommand::Complete {
            target: TargetSpec::Id(42)
        })
    );
}

#[test]
fn parse_complete_aliases() {
    // `done` and `toggle` route to the same Complete branch.
    assert_eq!(
        parse(&s(&["done", "7"])).unwrap().subcommand,
        Some(Subcommand::Complete {
            target: TargetSpec::Id(7)
        })
    );
    assert_eq!(
        parse(&s(&["toggle", "7"])).unwrap().subcommand,
        Some(Subcommand::Complete {
            target: TargetSpec::Id(7)
        })
    );
}

#[test]
fn parse_delete_takes_id() {
    let a = parse(&s(&["delete", "42"])).unwrap();
    assert_eq!(
        a.subcommand,
        Some(Subcommand::Delete {
            target: TargetSpec::Id(42),
            force: false
        })
    );
}

#[test]
fn parse_delete_rm_alias() {
    assert_eq!(
        parse(&s(&["rm", "9"])).unwrap().subcommand,
        Some(Subcommand::Delete {
            target: TargetSpec::Id(9),
            force: false
        })
    );
}

#[test]
fn parse_complete_invalid_id_errors() {
    let err = parse(&s(&["complete", "not-a-number"])).unwrap_err();
    assert!(err.contains("invalid task id"));
}

// ── Bulk --where ────────────────────────────────────────────────

#[test]
fn parse_complete_with_where_expression() {
    let a = parse(&s(&[
        "complete",
        "--where",
        "tag:work",
        "AND",
        "is:overdue",
    ]))
    .unwrap();
    assert_eq!(
        a.subcommand,
        Some(Subcommand::Complete {
            target: TargetSpec::Where("tag:work AND is:overdue".into())
        })
    );
}

#[test]
fn parse_complete_where_alias_filter() {
    let a = parse(&s(&["complete", "--filter", "is:overdue"])).unwrap();
    assert_eq!(
        a.subcommand,
        Some(Subcommand::Complete {
            target: TargetSpec::Where("is:overdue".into())
        })
    );
}

#[test]
fn parse_delete_with_where_default_no_force() {
    let a = parse(&s(&["delete", "--where", "is:done"])).unwrap();
    assert_eq!(
        a.subcommand,
        Some(Subcommand::Delete {
            target: TargetSpec::Where("is:done".into()),
            force: false
        })
    );
}

#[test]
fn parse_delete_with_where_and_force() {
    let a = parse(&s(&["delete", "--where", "is:done", "--force"])).unwrap();
    assert_eq!(
        a.subcommand,
        Some(Subcommand::Delete {
            target: TargetSpec::Where("is:done".into()),
            force: true
        })
    );
}

#[test]
fn parse_delete_yes_aliases_force() {
    let a = parse(&s(&["delete", "--where", "is:done", "--yes"])).unwrap();
    let Some(Subcommand::Delete { force, .. }) = a.subcommand else {
        panic!("expected Delete");
    };
    assert!(force);
}

#[test]
fn parse_complete_force_flag_unrecognised() {
    // --force is delete-only; complete shouldn't accept it.
    let err = parse(&s(&["complete", "--where", "is:overdue", "--force"])).unwrap_err();
    assert!(err.contains("unknown flag"));
}

#[test]
fn parse_complete_id_and_where_mutually_exclusive() {
    let err = parse(&s(&["complete", "42", "--where", "is:overdue"])).unwrap_err();
    assert!(err.contains("either"));
}

#[test]
fn parse_complete_no_target_errors() {
    let err = parse(&s(&["complete"])).unwrap_err();
    assert!(err.contains("task id") || err.contains("--where"));
}

#[test]
fn parse_delete_with_where_and_format_flag() {
    let a = parse(&s(&["delete", "--where", "is:done", "--force", "--json"])).unwrap();
    assert_eq!(a.format, Format::Json);
    let Some(Subcommand::Delete { force, .. }) = a.subcommand else {
        panic!("expected Delete");
    };
    assert!(force);
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

// ── Perspective write subcommand ───────────────────────────────

#[test]
fn parse_perspective_create_minimum() {
    let a = parse(&s(&[
        "perspective",
        "create",
        "Q3 Plans",
        "--filter",
        "tag:work",
    ]))
    .unwrap();
    let Some(Subcommand::Perspective(PerspectiveSub::Create { name, args })) = a.subcommand else {
        panic!("expected Perspective::Create");
    };
    assert_eq!(name, "Q3 Plans");
    assert_eq!(args.filter, Some("tag:work".into()));
    assert!(args.renderer.is_none());
    assert!(args.columns.is_none());
}

#[test]
fn parse_perspective_create_requires_filter() {
    let err = parse(&s(&["perspective", "create", "Q3 Plans"])).unwrap_err();
    assert!(err.contains("--filter"));
}

#[test]
fn parse_perspective_create_with_board_renderer_and_columns() {
    let a = parse(&s(&[
        "perspective",
        "create",
        "Q3",
        "--filter",
        "tag:work",
        "--renderer",
        "board",
        "--columns",
        "todo,doing,done",
    ]))
    .unwrap();
    let Some(Subcommand::Perspective(PerspectiveSub::Create { name: _, args })) = a.subcommand
    else {
        panic!("expected Perspective::Create");
    };
    assert_eq!(args.renderer.as_deref(), Some("board"));
    assert_eq!(args.columns.as_deref(), Some("todo,doing,done"));
}

#[test]
fn parse_perspective_create_rejects_rename() {
    // Rename only makes sense on edit — guard so the user notices.
    let err = parse(&s(&[
        "perspective",
        "create",
        "Q3",
        "--filter",
        "x",
        "--rename",
        "Q4",
    ]))
    .unwrap_err();
    assert!(err.contains("--rename"));
}

#[test]
fn parse_perspective_create_invalid_renderer() {
    let err = parse(&s(&[
        "perspective",
        "create",
        "Q3",
        "--filter",
        "x",
        "--renderer",
        "matrix",
    ]))
    .unwrap_err();
    assert!(err.contains("list"));
    assert!(err.contains("board"));
}

#[test]
fn parse_perspective_edit_collects_all_flags() {
    let a = parse(&s(&[
        "perspective",
        "edit",
        "Q3 Plans",
        "--rename",
        "Q4 Plans",
        "--filter",
        "tag:newfilter",
        "--icon",
        "starred-symbolic",
        "--renderer",
        "board",
        "--columns",
        "a,b,c",
    ]))
    .unwrap();
    let Some(Subcommand::Perspective(PerspectiveSub::Edit { name, args })) = a.subcommand else {
        panic!("expected Perspective::Edit");
    };
    assert_eq!(name, "Q3 Plans");
    assert_eq!(args.rename.as_deref(), Some("Q4 Plans"));
    assert_eq!(args.filter.as_deref(), Some("tag:newfilter"));
    assert_eq!(args.icon, Some(EditIcon::Set("starred-symbolic".into())));
    assert_eq!(args.renderer.as_deref(), Some("board"));
    assert_eq!(args.columns.as_deref(), Some("a,b,c"));
}

#[test]
fn parse_perspective_edit_icon_none_clears() {
    let a = parse(&s(&["perspective", "edit", "Q3", "--icon", "none"])).unwrap();
    let Some(Subcommand::Perspective(PerspectiveSub::Edit { args, .. })) = a.subcommand else {
        panic!("expected Perspective::Edit");
    };
    assert_eq!(args.icon, Some(EditIcon::Clear));
}

#[test]
fn parse_perspective_edit_no_flags_is_a_noop() {
    let a = parse(&s(&["perspective", "edit", "Q3 Plans"])).unwrap();
    let Some(Subcommand::Perspective(PerspectiveSub::Edit { name, args })) = a.subcommand else {
        panic!("expected Perspective::Edit");
    };
    assert_eq!(name, "Q3 Plans");
    assert_eq!(args, PerspectiveArgs::default());
}

#[test]
fn parse_perspective_delete_takes_only_a_name() {
    let a = parse(&s(&["perspective", "delete", "Q3 Plans"])).unwrap();
    assert_eq!(
        a.subcommand,
        Some(Subcommand::Perspective(PerspectiveSub::Delete {
            name: "Q3 Plans".into()
        }))
    );
}

#[test]
fn parse_perspective_delete_rejects_body_flags() {
    let err = parse(&s(&["perspective", "delete", "Q3", "--filter", "tag:x"])).unwrap_err();
    assert!(err.to_lowercase().contains("delete"));
}

#[test]
fn parse_perspective_unknown_sub_errors() {
    let err = parse(&s(&["perspective", "frobulate", "Q3"])).unwrap_err();
    assert!(err.contains("frobulate"));
}

#[test]
fn parse_perspective_no_sub_errors() {
    let err = parse(&s(&["perspective"])).unwrap_err();
    assert!(err.to_lowercase().contains("sub-subcommand"));
}

#[test]
fn parse_perspective_joins_multi_word_name() {
    // Shell already split — `perspective create Q3 Plans --filter ...`
    // — we rejoin Q3 + Plans into the name.
    let a = parse(&s(&[
        "perspective",
        "create",
        "Q3",
        "Plans",
        "--filter",
        "tag:x",
    ]))
    .unwrap();
    let Some(Subcommand::Perspective(PerspectiveSub::Create { name, .. })) = a.subcommand else {
        panic!("expected Perspective::Create");
    };
    assert_eq!(name, "Q3 Plans");
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

// ── v0.25.0 — VTODO import / export argv ────────────────────────

#[test]
fn parse_import_vtodo_requires_into_project() {
    use crate::args::ImportSource;
    let a = parse(&s(&["import", "vtodo", "/tmp/x.ics", "--into", "Errands"])).unwrap();
    let Some(Subcommand::Import {
        source,
        path,
        dry_run,
    }) = a.subcommand
    else {
        panic!("expected Import");
    };
    assert_eq!(
        source,
        ImportSource::Vtodo {
            project_name: "Errands".to_string(),
        }
    );
    assert_eq!(path, "/tmp/x.ics");
    assert!(!dry_run);
}

#[test]
fn parse_import_vtodo_dry_run_flag_threads_through() {
    let a = parse(&s(&[
        "import",
        "vtodo",
        "/tmp/x.ics",
        "--into",
        "Errands",
        "--dry-run",
    ]))
    .unwrap();
    let Some(Subcommand::Import { dry_run, .. }) = a.subcommand else {
        panic!("expected Import");
    };
    assert!(dry_run);
}

#[test]
fn parse_import_vtodo_missing_into_errors() {
    let err = parse(&s(&["import", "vtodo", "/tmp/x.ics"])).unwrap_err();
    assert!(
        err.contains("--into"),
        "expected --into requirement in error; got: {err}"
    );
}

// ── v0.26.0 — Taskwarrior import argv ───────────────────────────

#[test]
fn parse_import_taskwarrior_defaults_uda_as_to_tag() {
    use crate::args::{ImportSource, UdaPolicy};
    let a = parse(&s(&[
        "import",
        "taskwarrior",
        "/tmp/x.json",
        "--into",
        "Inbox",
    ]))
    .unwrap();
    let Some(Subcommand::Import { source, .. }) = a.subcommand else {
        panic!("expected Import");
    };
    assert_eq!(
        source,
        ImportSource::Taskwarrior {
            project_name: "Inbox".to_string(),
            uda_as: UdaPolicy::Tag,
        },
    );
}

#[test]
fn parse_import_taskwarrior_uda_as_flag_round_trips() {
    use crate::args::{ImportSource, UdaPolicy};
    for (flag, expected) in [
        ("tag", UdaPolicy::Tag),
        ("note", UdaPolicy::Note),
        ("drop", UdaPolicy::Drop),
    ] {
        let a = parse(&s(&[
            "import",
            "taskwarrior",
            "/tmp/x.json",
            "--into",
            "Inbox",
            "--uda-as",
            flag,
        ]))
        .unwrap();
        let Some(Subcommand::Import { source, .. }) = a.subcommand else {
            panic!("expected Import");
        };
        assert_eq!(
            source,
            ImportSource::Taskwarrior {
                project_name: "Inbox".to_string(),
                uda_as: expected,
            },
        );
    }
}

#[test]
fn parse_import_taskwarrior_missing_into_errors() {
    let err = parse(&s(&["import", "taskwarrior", "/tmp/x.json"])).unwrap_err();
    assert!(
        err.contains("--into"),
        "expected --into requirement in error; got: {err}"
    );
}

#[test]
fn parse_import_taskwarrior_bad_uda_value_errors() {
    let err = parse(&s(&[
        "import",
        "taskwarrior",
        "/tmp/x.json",
        "--into",
        "Inbox",
        "--uda-as",
        "garbage",
    ]))
    .unwrap_err();
    assert!(
        err.contains("--uda-as"),
        "expected --uda-as in error; got: {err}"
    );
}

#[test]
fn parse_import_org_rejects_uda_as_flag() {
    let err = parse(&s(&["import", "org", "/tmp/x.org", "--uda-as", "tag"])).unwrap_err();
    assert!(
        err.contains("--uda-as"),
        "expected --uda-as rejection in error; got: {err}"
    );
}

#[test]
fn parse_export_vtodo_round_trips() {
    use crate::args::ExportSource;
    let a = parse(&s(&["export", "vtodo", "/tmp/out.ics"])).unwrap();
    let Some(Subcommand::Export {
        source,
        path,
        dry_run,
    }) = a.subcommand
    else {
        panic!("expected Export");
    };
    assert_eq!(source, ExportSource::Vtodo);
    assert_eq!(path, "/tmp/out.ics");
    assert!(!dry_run);
}

// ── SQL fast-path ↔ in-memory eval parity ───────────────────────
//
// The SQL translator is the v0.5.3 perf optimization: queries that
// translate cleanly to SQL run at the database layer instead of
// pulling every row into memory. The translator's "all-or-nothing"
// rule makes this safe in principle — anything that can't be
// expressed in SQL falls back to the in-memory evaluator. These
// tests are the empirical safety net: same fixture, same query,
// both paths must return the same id set. If the SQL path and the
// in-memory path ever disagree, this is the alarm bell.

mod sql_parity {
    use atrium_core::db::{self, read};
    use atrium_search::{EvalContext, evaluate};
    use chrono::NaiveDate;
    use rusqlite::{Connection, params};
    use std::collections::{HashMap, HashSet};
    use std::path::Path;

    fn fresh_conn() -> Connection {
        // `:memory:` keeps each test isolated and dodges the need
        // to plumb a temp dir; db::open accepts it (the helper
        // skips create_dir_all for the literal ":memory:" path).
        db::open(Path::new(":memory:")).unwrap()
    }

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 5, 15).unwrap()
    }

    /// Seed a small mixed-shape fixture: completed and open tasks,
    /// a few with deadlines / scheduled dates / defer dates / tags
    /// / repeat rules. Ids are autoincrement; we read them back at
    /// the end so the parity tests can compare id sets without
    /// caring about the exact id values.
    fn seed_mixed_fixture(conn: &Connection) {
        // Tags.
        conn.execute(
            "INSERT INTO tag (uuid, name) VALUES \
             ('tag-work', 'work'), ('tag-home', 'home'), ('tag-urgent', 'urgent')",
            [],
        )
        .unwrap();
        let work_id: i64 = conn
            .query_row("SELECT id FROM tag WHERE name='work'", [], |r| r.get(0))
            .unwrap();
        let urgent_id: i64 = conn
            .query_row("SELECT id FROM tag WHERE name='urgent'", [], |r| r.get(0))
            .unwrap();

        // Tasks with assorted shapes — see test names below for what
        // each row exercises. Tuple = (uuid, title, scheduled_for,
        // deadline, defer_until, completed_at, repeat_rule).
        #[allow(clippy::type_complexity)]
        let rows: &[(
            &str,
            &str,
            Option<&str>,
            Option<&str>,
            Option<&str>,
            Option<&str>,
            Option<&str>,
        )] = &[
            ("u1", "Buy milk", None, None, None, None, None),
            (
                "u2",
                "Pay invoice",
                None,
                Some("2026-05-14"),
                None,
                None,
                None,
            ),
            (
                "u3",
                "Stand-up meeting",
                Some("2026-05-15"),
                None,
                None,
                None,
                None,
            ),
            (
                "u4",
                "Old task",
                None,
                None,
                None,
                Some("2026-05-10T08:00:00.000Z"),
                None,
            ),
            (
                "u5",
                "Quarterly review",
                None,
                Some("2026-05-22"),
                None,
                None,
                None,
            ),
            (
                "u6",
                "Future planning",
                Some("2026-06-01"),
                None,
                None,
                None,
                None,
            ),
            (
                "u7",
                "Weekly report",
                None,
                Some("2026-05-15"),
                None,
                None,
                Some("RRULE:FREQ=WEEKLY"),
            ),
            (
                "u8",
                "Deferred decision",
                None,
                None,
                Some("2026-06-15"),
                None,
                None,
            ),
        ];

        for (i, row) in rows.iter().enumerate() {
            let pos = (i + 1) as f64;
            conn.execute(
                "INSERT INTO task \
                 (uuid, title, scheduled_for, deadline, defer_until, completed_at, repeat_rule, position) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                params![row.0, row.1, row.2, row.3, row.4, row.5, row.6, pos],
            )
            .unwrap();
        }

        // Tag attachments. (task_id, tag_id) — note ids are
        // autoincremented so we look up by uuid.
        let attach = |uuid: &str, tag_id: i64| {
            let task_id: i64 = conn
                .query_row("SELECT id FROM task WHERE uuid = ?", params![uuid], |r| {
                    r.get(0)
                })
                .unwrap();
            conn.execute(
                "INSERT INTO task_tag (task_id, tag_id) VALUES (?, ?)",
                params![task_id, tag_id],
            )
            .unwrap();
        };
        attach("u1", work_id);
        attach("u2", work_id);
        attach("u6", urgent_id);
    }

    /// Run `query` against `conn` through both paths and return the
    /// (sql_path_ids, in_memory_ids) pair as `HashSet<i64>` so the
    /// caller asserts equality independent of ordering.
    fn ids_from_both_paths(conn: &Connection, query: &str) -> (HashSet<i64>, HashSet<i64>) {
        let parsed = atrium_search::parse(query).unwrap();

        // In-memory path.
        let tag_names = read::tag_names_per_task(conn).unwrap_or_default();
        let project_titles = HashMap::new();
        let project_areas = HashMap::new();
        let area_titles = HashMap::new();
        let ctx = EvalContext::new(
            today(),
            &tag_names,
            &project_titles,
            &project_areas,
            &area_titles,
        );
        let mut all = read::list_all_tasks(conn).unwrap();
        all.retain(|t| evaluate(&parsed.expr, t, &ctx));
        let in_memory: HashSet<i64> = all.iter().map(|t| t.id).collect();

        // SQL path — only valid when try_translate returns Some.
        let sql_path: HashSet<i64> =
            if let Some(clause) = atrium_search::try_translate(&parsed.expr, today()) {
                let params: Vec<atrium_core::SqlBindValue> =
                    clause.params.iter().map(Into::into).collect();
                read::list_tasks_matching(conn, &clause.sql, &params)
                    .unwrap()
                    .iter()
                    .map(|t| t.id)
                    .collect()
            } else {
                // No SQL path available — copy the in-memory set so the
                // test doesn't fail on the assert; a sibling test
                // confirms that try_translate returned None for the
                // un-translatable shapes.
                in_memory.clone()
            };
        (sql_path, in_memory)
    }

    fn assert_paths_agree(query: &str) {
        let conn = fresh_conn();
        seed_mixed_fixture(&conn);
        let (sql_ids, mem_ids) = ids_from_both_paths(&conn, query);
        assert_eq!(
            sql_ids, mem_ids,
            "SQL path and in-memory path disagreed on `{query}`: \
             sql={sql_ids:?}, mem={mem_ids:?}"
        );
    }

    #[test]
    fn parity_open_only() {
        assert_paths_agree("is:open");
    }

    #[test]
    fn parity_done_only() {
        assert_paths_agree("is:done");
    }

    #[test]
    fn parity_overdue() {
        assert_paths_agree("is:overdue");
    }

    #[test]
    fn parity_repeating() {
        assert_paths_agree("is:repeating");
    }

    #[test]
    fn parity_deferred() {
        assert_paths_agree("is:deferred");
    }

    #[test]
    fn parity_tagged() {
        assert_paths_agree("is:tagged");
    }

    #[test]
    fn parity_bare_text_substring() {
        assert_paths_agree("invoice");
    }

    #[test]
    fn parity_title_substring() {
        assert_paths_agree("title:meeting");
    }

    #[test]
    fn parity_tag_substring() {
        assert_paths_agree("tag:work");
    }

    #[test]
    fn parity_tag_exact() {
        assert_paths_agree("tag:=work");
    }

    #[test]
    fn parity_due_today() {
        assert_paths_agree("due:today");
    }

    #[test]
    fn parity_due_thisweek() {
        assert_paths_agree("due:thisweek");
    }

    #[test]
    fn parity_due_gt_today() {
        assert_paths_agree("due:>today");
    }

    #[test]
    fn parity_due_range() {
        assert_paths_agree("due:2026-05-01..2026-05-31");
    }

    #[test]
    fn parity_compound_and() {
        assert_paths_agree("is:open AND tag:work");
    }

    #[test]
    fn parity_compound_or() {
        assert_paths_agree("tag:work OR tag:urgent");
    }

    #[test]
    fn parity_negation() {
        assert_paths_agree("NOT tag:work");
    }

    #[test]
    fn parity_complex_compound() {
        assert_paths_agree("is:open AND (tag:work OR is:overdue)");
    }

    // Sanity: the fall-back shapes are translator-rejected, so
    // ids_from_both_paths fakes the SQL set from the in-memory set.
    // We assert that try_translate genuinely returned None — that's
    // the contract.

    #[test]
    fn falls_back_for_regex() {
        let parsed = atrium_search::parse("tag:~wo").unwrap();
        assert!(atrium_search::try_translate(&parsed.expr, today()).is_none());
    }

    #[test]
    fn falls_back_for_fuzzy() {
        let parsed = atrium_search::parse("tag:?wrok").unwrap();
        assert!(atrium_search::try_translate(&parsed.expr, today()).is_none());
    }

    #[test]
    fn falls_back_for_is_today() {
        // is:today is composite (mirrors list_today's deadline
        // window etc.); deferred from v1 SQL translation.
        let parsed = atrium_search::parse("is:today").unwrap();
        assert!(atrium_search::try_translate(&parsed.expr, today()).is_none());
    }
}
