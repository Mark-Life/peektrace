//! `peektrace memory ls|show|rm`: Claude memory files. `ls` with no project
//! lists every project with memory; with one it lists that vault. `rm` is
//! refused up front under `--read-only`.

use peektrace_core::Agent;
use peektrace_core::memory::json::{delete_json, entry_json, projects_json, vault_json};
use peektrace_core::memory::{
    DeleteResult, MemoryCtx, MemoryEntry, MemoryVault, ProjectSummary, delete, get_vault,
    list_projects,
};

use crate::args::{MemoryLsArgs, MemoryNameArgs};
use crate::ls::{Context, Globals, UserError};
use crate::render::{bytes, table};
use crate::single::{Output, agent_roots};

fn printed(stdout: String, warnings: Vec<String>) -> Output {
    Output {
        stdout: Some(stdout),
        write: None,
        warnings,
        unreadable: None,
        notes: Vec::new(),
    }
}

fn yes_no(b: bool) -> String {
    if b { "yes" } else { "no" }.to_owned()
}

fn size_of(n: usize) -> String {
    bytes(u64::try_from(n).unwrap_or(u64::MAX))
}

fn projects_table(projects: &[ProjectSummary], pretty: bool) -> String {
    let rows: Vec<Vec<String>> = projects
        .iter()
        .map(|p| {
            vec![
                p.slug.clone(),
                p.file_count.to_string(),
                yes_no(p.has_index),
            ]
        })
        .collect();
    table(&["PROJECT", "FILES", "INDEX"], &rows, pretty)
}

fn vault_table(v: &MemoryVault, pretty: bool) -> String {
    let rows: Vec<Vec<String>> = v
        .entries
        .iter()
        .map(|e| {
            vec![
                e.slug.clone(),
                e.kind().unwrap_or("-").to_owned(),
                yes_no(e.in_index),
                size_of(e.size),
                e.description().unwrap_or("-").to_owned(),
            ]
        })
        .collect();
    format!(
        "vault {} ({}) — {} entries\n{}",
        v.slug,
        v.state.as_str(),
        v.entries.len(),
        table(
            &["NAME", "TYPE", "INDEX", "SIZE", "DESCRIPTION"],
            &rows,
            pretty
        )
    )
}

fn entry_text(e: &MemoryEntry, pretty: bool) -> String {
    let row = |k: &str, v: String| vec![k.to_owned(), v];
    let meta = table(
        &["FIELD", "VALUE"],
        &[
            row("name", e.name().unwrap_or(&e.slug).to_owned()),
            row("type", e.kind().unwrap_or("-").to_owned()),
            row("description", e.description().unwrap_or("-").to_owned()),
            row("size", size_of(e.size)),
            row("modified", e.mtime.clone()),
            row("in index", yes_no(e.in_index)),
            row("links", e.links.len().to_string()),
        ],
        pretty,
    );
    format!("{meta}\n\n{}", e.body)
}

fn delete_text(r: &DeleteResult) -> String {
    let dangling = if r.dangling.is_empty() {
        "no dangling references left behind.".to_owned()
    } else {
        let refs: Vec<String> = r
            .dangling
            .iter()
            .map(|d| format!("  {} -> {}", d.from, d.target))
            .collect();
        format!("now-dangling references:\n{}", refs.join("\n"))
    };
    format!("Deleted {}; {dangling}", r.slug)
}

/// `memory ls [project]`.
pub fn run_ls(args: &MemoryLsArgs, globals: Globals, ctx: &Context) -> Output {
    let (roots, warnings) = agent_roots(ctx);
    let stdout = match &args.project {
        None => {
            let projects = list_projects(&roots.claude);
            if globals.json {
                projects_json(&projects)
            } else {
                projects_table(&projects, globals.pretty)
            }
        }
        Some(slug) => {
            let vault = get_vault(&roots.claude, slug);
            if globals.json {
                vault_json(&vault)
            } else {
                vault_table(&vault, globals.pretty)
            }
        }
    };
    printed(stdout, warnings)
}

fn not_found(args: &MemoryNameArgs) -> String {
    format!(
        "Memory \"{}\" not found in project \"{}\".",
        args.name, args.project
    )
}

/// `memory show <project> <name>`. An unknown name prints a note and exits 0.
pub fn run_show(args: &MemoryNameArgs, globals: Globals, ctx: &Context) -> Output {
    let (roots, warnings) = agent_roots(ctx);
    let vault = get_vault(&roots.claude, &args.project);
    match vault.entries.iter().find(|e| e.slug == args.name) {
        None => Output {
            stdout: None,
            notes: vec![not_found(args)],
            ..printed(String::new(), warnings)
        },
        Some(e) if globals.json => printed(entry_json(e), warnings),
        Some(e) => printed(entry_text(e, globals.pretty), warnings),
    }
}

/// `memory rm <project> <name>`.
pub fn run_rm(
    args: &MemoryNameArgs,
    globals: Globals,
    ctx: &Context,
    tmp_dir: &str,
) -> Result<Output, UserError> {
    if globals.read_only {
        return Err(UserError::Invalid(format!(
            "Refusing to delete \"{}\" in \"{}\": --read-only is set (no write performed).",
            args.name, args.project
        )));
    }
    let (roots, warnings) = agent_roots(ctx);
    let mem = MemoryCtx::new(&roots, tmp_dir);
    let result = delete(&mem, Agent::Claude, &args.project, &args.name)
        .map_err(|e| UserError::Invalid(e.to_string()))?;
    let stdout = if globals.json {
        delete_json(&result)
    } else {
        delete_text(&result)
    };
    Ok(printed(stdout, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use peektrace_core::memory::DanglingRef;

    #[test]
    fn delete_messages() {
        let clean = DeleteResult {
            slug: "a".to_owned(),
            dangling: vec![],
        };
        assert_eq!(
            delete_text(&clean),
            "Deleted a; no dangling references left behind."
        );
        let broken = DeleteResult {
            slug: "b".to_owned(),
            dangling: vec![DanglingRef {
                from: "a".to_owned(),
                target: "b".to_owned(),
                line: 1,
            }],
        };
        assert_eq!(
            delete_text(&broken),
            "Deleted b; now-dangling references:\n  a -> b"
        );
    }
}
