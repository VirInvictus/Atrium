// SPDX-License-Identifier: MIT
//! `atrium-cli template` subcommands (Phase 18.5 Tier-1, v0.18.0).
//! Quick Entry templates — pre-filled capture recipes surfaced in
//! the Quick Entry modal as a picker bar.
//!
//! - `template list` — print configured templates
//! - `template add NAME [--shortcut LETTER --project NAME --prefix TEXT --tag TAG]`
//! - `template edit NAME [--rename NEW --shortcut LETTER|none ...]`
//! - `template remove NAME`
//!
//! Extracted from `main.rs` in the v0.21.0 maintenance pass.

use rusqlite::Connection;

use crate::args::{Format, TemplateArgs};
use crate::{CliError, CliResult, json_escape};

pub fn run_template_list(conn: &Connection, format: Format) -> CliResult<()> {
    let templates =
        atrium_core::db::read::list_quick_entry_templates(conn).map_err(CliError::from)?;
    match format {
        Format::Human => {
            if templates.is_empty() {
                println!("(no templates configured)");
                return Ok(());
            }
            for t in &templates {
                let shortcut = t
                    .shortcut_key
                    .as_deref()
                    .map(|k| format!("[{k}] "))
                    .unwrap_or_default();
                println!("{shortcut}{}", t.name);
                if !t.prefix.is_empty() {
                    println!("  prefix: {}", t.prefix);
                }
                if let Some(pid) = t.target_project_id
                    && let Ok(Some(p)) = atrium_core::db::read::project_by_id(conn, pid)
                {
                    println!("  project: {}", p.title);
                }
                if !t.default_tags.is_empty() {
                    println!("  tags: {}", t.default_tags.join(" "));
                }
            }
        }
        Format::Tsv => {
            println!("id\tname\tshortcut\tproject_id\tprefix\tdefault_tags");
            for t in &templates {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}",
                    t.id,
                    t.name,
                    t.shortcut_key.as_deref().unwrap_or(""),
                    t.target_project_id
                        .map(|i| i.to_string())
                        .unwrap_or_default(),
                    t.prefix.replace(['\t', '\n'], " "),
                    t.default_tags.join(",")
                );
            }
        }
        Format::Json => {
            print!("[");
            for (i, t) in templates.iter().enumerate() {
                if i > 0 {
                    print!(",");
                }
                let shortcut = match &t.shortcut_key {
                    Some(k) => format!("\"{}\"", json_escape(k)),
                    None => "null".to_string(),
                };
                let project = match t.target_project_id {
                    Some(p) => p.to_string(),
                    None => "null".to_string(),
                };
                let tags_json = t
                    .default_tags
                    .iter()
                    .map(|tag| format!("\"{}\"", json_escape(tag)))
                    .collect::<Vec<_>>()
                    .join(",");
                print!(
                    "{{\"id\":{},\"name\":\"{}\",\"shortcut\":{},\"target_project_id\":{},\"prefix\":\"{}\",\"default_tags\":[{}]}}",
                    t.id,
                    json_escape(&t.name),
                    shortcut,
                    project,
                    json_escape(&t.prefix),
                    tags_json
                );
            }
            println!("]");
        }
    }
    Ok(())
}

/// `template add NAME [...]`. Resolves project name to id via
/// the standard prefix-match. Validates shortcut at parse time
/// (worker also validates; we just want a clear error message
/// before the worker round-trip).
pub fn run_template_add(
    runtime: &tokio::runtime::Runtime,
    handle: &atrium_core::WorkerHandle,
    conn: &Connection,
    args: TemplateArgs,
    format: Format,
) -> CliResult<()> {
    let name = args
        .name
        .ok_or_else(|| CliError::Args("name required".into()))?;
    let target_project_id = match args.project.as_deref() {
        Some(s) => Some(resolve_project_id(conn, s)?),
        None => None,
    };
    let new = atrium_core::NewQuickEntryTemplate {
        name,
        shortcut_key: args.shortcut,
        target_project_id,
        prefix: args.prefix.unwrap_or_default(),
        default_tags: args.tags,
    };
    let template = runtime
        .block_on(async { handle.create_quick_entry_template(new).await })
        .map_err(CliError::from)?;
    if matches!(format, Format::Human) {
        println!("created template: {}", template.name);
    }
    Ok(())
}

/// `template edit NAME [--rename NEW] [...]`. Looks the template
/// up by case-insensitive name match; rejects on no-match (less
/// surprising than silent no-op).
pub fn run_template_edit(
    runtime: &tokio::runtime::Runtime,
    handle: &atrium_core::WorkerHandle,
    conn: &Connection,
    name: &str,
    args: TemplateArgs,
    format: Format,
) -> CliResult<()> {
    let target = find_template_by_name(conn, name)?;
    let mut update = atrium_core::QuickEntryTemplateUpdate::new(target.id);
    if let Some(rename) = args.rename {
        update = update.name(rename);
    }
    if let Some(shortcut) = args.shortcut {
        update = if shortcut.eq_ignore_ascii_case("none") {
            update.shortcut_key(None)
        } else {
            update.shortcut_key(Some(shortcut))
        };
    }
    if let Some(project) = args.project {
        update = if project.eq_ignore_ascii_case("none") || project.eq_ignore_ascii_case("inbox") {
            update.target_project_id(None)
        } else {
            let pid = resolve_project_id(conn, &project)?;
            update.target_project_id(Some(pid))
        };
    }
    if let Some(prefix) = args.prefix {
        update = update.prefix(prefix);
    }
    if !args.tags.is_empty() {
        update = update.default_tags(args.tags);
    }
    runtime
        .block_on(async { handle.update_quick_entry_template(update).await })
        .map_err(CliError::from)?;
    if matches!(format, Format::Human) {
        println!("updated template: {name}");
    }
    Ok(())
}

/// `template remove NAME`. Case-insensitive lookup.
pub fn run_template_remove(
    runtime: &tokio::runtime::Runtime,
    handle: &atrium_core::WorkerHandle,
    conn: &Connection,
    name: &str,
) -> CliResult<()> {
    let target = find_template_by_name(conn, name)?;
    runtime
        .block_on(async { handle.delete_quick_entry_template(target.id).await })
        .map_err(CliError::from)?;
    println!("removed template: {name}");
    Ok(())
}

fn find_template_by_name(
    conn: &Connection,
    name: &str,
) -> CliResult<atrium_core::QuickEntryTemplate> {
    let templates =
        atrium_core::db::read::list_quick_entry_templates(conn).map_err(CliError::from)?;
    templates
        .into_iter()
        .find(|t| t.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| CliError::Args(format!("no template matches name {name:?}")))
}

/// Resolve a project name (case-insensitive prefix match) to id.
/// Returns the only match, or errors on zero or multiple matches.
fn resolve_project_id(conn: &Connection, name: &str) -> CliResult<i64> {
    let projects = atrium_core::db::read::list_projects(conn).map_err(CliError::from)?;
    let needle = name.to_ascii_lowercase();
    let matches: Vec<_> = projects
        .iter()
        .filter(|p| p.title.to_ascii_lowercase().contains(&needle))
        .collect();
    match matches.len() {
        0 => Err(CliError::Args(format!("no project matches {name:?}"))),
        1 => Ok(matches[0].id),
        _ => Err(CliError::Args(format!(
            "multiple projects match {name:?}: {}",
            matches
                .iter()
                .map(|p| p.title.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}
