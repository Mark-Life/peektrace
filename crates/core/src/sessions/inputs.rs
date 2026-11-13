//! The files and dirs one analysis reads, so a cache can stamp them and tell
//! when the analysis went stale. Missing paths are listed too: creating one
//! can change the analysis.

use std::collections::HashSet;
use std::fs;

use super::context_files::{
    AGENTS_FILES, CLAUDE_FILES, LOCAL_FILE, MAX_RULE_DEPTH, RULES_DIR, memory_root, memory_slugs,
    project_dirs,
};
use super::resolve::slash_join;
use super::service::FullParse;
use crate::model::Agent;
use crate::opencode;
use crate::paths::real_path;

/// `dir` and every entry under it, dirs included.
fn tree(dir: &str, depth: usize, seen: &mut HashSet<String>, out: &mut Vec<String>) {
    out.push(dir.to_owned());
    if depth > MAX_RULE_DEPTH || !seen.insert(real_path(dir)) {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for entry in rd.filter_map(Result::ok) {
        let path = slash_join(&[dir, &entry.file_name().to_string_lossy()]);
        if fs::metadata(&path).is_ok_and(|m| m.is_dir()) {
            tree(&path, depth + 1, seen, out);
        } else {
            out.push(path);
        }
    }
}

fn tree_paths(dir: &str) -> Vec<String> {
    let mut out = Vec::new();
    tree(dir, 0, &mut HashSet::new(), &mut out);
    out
}

/// A Claude transcript, its session dir and its whole subagents tree, so a
/// subagent added or rewritten later shows.
fn claude_transcript(path: &str) -> Vec<String> {
    let session_dir = path.strip_suffix(".jsonl");
    std::iter::once(path.to_owned())
        .chain(session_dir.map(str::to_owned))
        .chain(
            session_dir
                .into_iter()
                .flat_map(|d| tree_paths(&slash_join(&[d, "subagents"]))),
        )
        .collect()
}

/// Every instruction file `gather_on_disk_context_files` may read for a
/// session run in `cwd`, whichever exist now, plus the rules dirs.
pub fn context_inputs(cwd: Option<&str>, home: &str, projects_root: &str) -> Vec<String> {
    let global = std::iter::once(slash_join(&[home, "CLAUDE.md"]))
        .chain(tree_paths(&slash_join(&[home, "rules"])));
    let Some(cwd) = cwd.filter(|c| !c.is_empty()) else {
        return global.collect();
    };
    let project = project_dirs(cwd).into_iter().flat_map(|dir| {
        CLAUDE_FILES
            .iter()
            .chain(&AGENTS_FILES)
            .chain(std::iter::once(&LOCAL_FILE))
            .map(|name| slash_join(&[&dir, name]))
            .chain(tree_paths(&slash_join(&[&dir, RULES_DIR])))
            .collect::<Vec<_>>()
    });
    let memory = memory_slugs(&memory_root(cwd))
        .into_iter()
        .map(|slug| slash_join(&[projects_root, &slug, "memory", "MEMORY.md"]));
    global.chain(project).chain(memory).collect()
}

/// The transcript or store paths of one parsed session.
pub fn transcript_inputs(full: &FullParse, opencode_root: &str) -> Vec<String> {
    let p = &full.parsed;
    match full.agent {
        Agent::Claude => claude_transcript(&p.path),
        Agent::Opencode => opencode::store_paths(&p.path, &p.session_id, opencode_root),
        Agent::Codex | Agent::Pi => vec![p.path.clone()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_subagents_dir_is_still_listed() {
        assert_eq!(
            claude_transcript("/nope/p/s.jsonl"),
            ["/nope/p/s.jsonl", "/nope/p/s", "/nope/p/s/subagents"]
        );
    }

    #[test]
    fn context_lists_every_candidate_up_the_tree() {
        let got = context_inputs(Some("/nope/w"), "/h", "/r");
        for want in [
            "/h/CLAUDE.md",
            "/h/rules",
            "/nope/CLAUDE.md",
            "/nope/w/.claude/AGENTS.md",
            "/nope/w/CLAUDE.local.md",
            "/nope/w/.claude/rules",
            "/r/-nope-w/memory/MEMORY.md",
        ] {
            assert!(got.iter().any(|g| g == want), "{want} missing from {got:?}");
        }
    }
}
