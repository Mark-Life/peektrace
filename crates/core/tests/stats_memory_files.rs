//! Memory files read from disk: symlinks followed and deduped, imports,
//! per-agent project files and covers.

mod common;

use common::rules::*;
use peektrace_core::Agent;
use peektrace_core::stats::detectors::rules::RuleCover;

#[test]
fn follows_symlinks_dedupes_and_collapses_home() {
    let t = fixture();
    let docs = read_docs(&t, &[t.path("u/proj")], ".claude", &CLAUDE_CODEX);
    let shape: Vec<(Agent, bool)> = docs
        .iter()
        .map(|d| (d.cover.agent, d.cover.dir.is_some()))
        .collect();
    assert_eq!(
        shape,
        [
            (Agent::Claude, false),
            (Agent::Claude, false),
            (Agent::Codex, false),
            (Agent::Codex, false),
            (Agent::Codex, true),
        ]
    );
    assert_eq!(
        docs[2].display_path,
        "~/.codex/AGENTS.md → ~/.agents/AGENTS.md"
    );
    assert_eq!(docs[2].cover, cover(Agent::Codex, None));
    assert_eq!(
        docs[0].display_path,
        "~/.claude/CLAUDE.md → ~/.agents/AGENTS.md"
    );
    assert_eq!(docs[0].cover, cover(Agent::Claude, None));
    assert_eq!(docs[0].mtime_ms, Some(ms("2026-08-01T12:00:00.000Z")));
    assert_eq!(docs[4].display_path, "~/proj/AGENTS.md");
    assert_eq!(docs[4].cover, codex_cover(&t.path("u/proj"), false, &[]));
}

#[test]
fn claude_global_and_project_rules_dirs_are_read() {
    let t = fixture();
    t.write("u/.claude/rules/zsh.md", "- Quote every glob.");
    t.write("u/.claude/rules/notes.txt", "- Not a rule file.");
    t.write("u/proj/.claude/rules/shell/pkill.md", "- pkill -x only.");
    let docs = read_docs(&t, &[t.path("u/proj/app")], ".claude", &[Agent::Claude]);
    let rules: Vec<(&str, &RuleCover)> = docs
        .iter()
        .filter(|d| d.display_path.contains("/rules/"))
        .map(|d| (d.display_path.as_str(), &d.cover))
        .collect();
    assert_eq!(
        rules,
        [
            ("~/.claude/rules/zsh.md", &cover(Agent::Claude, None)),
            (
                "~/proj/.claude/rules/shell/pkill.md",
                &cover(Agent::Claude, Some(&t.path("u/proj")))
            ),
        ]
    );
}

#[test]
fn codex_walks_up_only_to_the_repo_root() {
    let t = fixture();
    t.mkdir("u/work/repo/src/.git");
    t.mkdir("u/work/loose/sub");
    t.write("u/work/AGENTS.md", "- Above every repo.");
    t.write("u/work/loose/AGENTS.md", "- Above the cwd.");
    let in_repo = t.path("u/work/repo/src");
    let loose = t.path("u/work/loose/sub");
    let docs = read_docs(&t, &[in_repo.clone(), loose], ".claude", &[Agent::Codex]);
    assert!(docs.iter().all(|d| d.cover.dir.is_none()));
    let pi = read_docs(&t, &[in_repo], ".claude", &[Agent::Pi]);
    assert!(pi.iter().any(|d| d.display_path == "~/work/AGENTS.md"));
}

#[test]
fn a_codex_file_reaches_its_repo_but_not_a_nested_one() {
    let t = fixture();
    t.mkdir("u/nest/outer/.git");
    let inner = t.mkdir("u/nest/outer/vendor/inner");
    t.mkdir("u/nest/outer/vendor/inner/.git");
    t.write("u/nest/outer/AGENTS.md", "- Outer rules.");
    t.write("u/nest/loose/AGENTS.md", "- Loose rules.");
    let docs = read_docs(
        &t,
        &[
            t.path("u/nest/outer"),
            inner.clone(),
            t.path("u/nest/loose"),
        ],
        ".claude",
        &[Agent::Codex],
    );
    let covers: Vec<&RuleCover> = docs
        .iter()
        .filter(|d| d.cover.dir.is_some())
        .map(|d| &d.cover)
        .collect();
    assert_eq!(
        covers,
        [
            &codex_cover(&t.path("u/nest/loose"), true, &[]),
            &codex_cover(&t.path("u/nest/outer"), false, &[inner]),
        ]
    );
}

#[test]
fn each_claude_sources_global_file_reaches_its_own_sessions() {
    let t = fixture();
    t.write("u/.claude-work/CLAUDE.md", "- Work rules.");
    let claude = [
        source("claude", t.path("u/.claude"), t.path("u/projects")),
        source(
            "work",
            t.path("u/.claude-work"),
            t.path("u/.claude-work/projects"),
        ),
    ];
    let docs = read_with(&t, &claude, &[], &[Agent::Claude]);
    let got: Vec<(&str, Option<&str>)> = docs
        .iter()
        .map(|d| {
            (
                d.display_path.as_str(),
                d.cover.source.as_ref().map(|s| s.0.as_str()),
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("~/.claude/CLAUDE.md → ~/.agents/AGENTS.md", Some("claude")),
            ("~/.agents/skills/rm.md", Some("claude")),
            ("~/.claude-work/CLAUDE.md", Some("work")),
        ]
    );
}

#[test]
fn a_repo_outside_the_home_keeps_its_root_file() {
    let t = fixture();
    let repo = t.mkdir("outside/repo");
    let pkg = t.mkdir("outside/repo/packages/app");
    t.mkdir("outside/repo/.git");
    t.write("outside/repo/CLAUDE.md", "- Repo-root Claude rules.");
    t.write("outside/repo/AGENTS.md", "- Repo-root Codex rules.");
    let docs = read_docs(&t, &[pkg], ".claude", &CLAUDE_CODEX);
    let own: Vec<(Agent, Option<&str>)> = docs
        .iter()
        .filter(|d| d.real_path.starts_with(&repo))
        .map(|d| (d.cover.agent, d.cover.dir.as_deref()))
        .collect();
    assert!(own.contains(&(Agent::Claude, Some(repo.as_str()))));
    assert!(own.contains(&(Agent::Codex, Some(repo.as_str()))));
}

#[test]
fn opencode_reads_its_global_file_else_claudes() {
    let t = fixture();
    let fallback = read_docs(&t, &[], ".claude", &[Agent::Opencode]);
    let got: Vec<(&str, &RuleCover)> = fallback
        .iter()
        .map(|d| (d.display_path.as_str(), &d.cover))
        .collect();
    assert_eq!(
        got,
        [
            (
                "~/.claude/CLAUDE.md → ~/.agents/AGENTS.md",
                &cover(Agent::Opencode, None)
            ),
            ("~/.agents/skills/rm.md", &cover(Agent::Opencode, None)),
        ]
    );
    t.write("u/.config/opencode/AGENTS.md", "- OpenCode global rules.");
    let own = read_docs(&t, &[], ".claude", &[Agent::Opencode]);
    let paths: Vec<&str> = own.iter().map(|d| d.display_path.as_str()).collect();
    assert_eq!(paths, ["~/.config/opencode/AGENTS.md"]);
}

#[test]
fn each_agent_reads_only_its_own_project_files() {
    let t = fixture();
    let docs = read_docs(
        &t,
        &[t.path("u/proj/app")],
        ".claude",
        &[Agent::Claude, Agent::Codex, Agent::Pi],
    );
    let shown: Vec<(&str, Agent)> = docs
        .iter()
        .filter(|d| d.cover.dir.is_some())
        .map(|d| (d.display_path.as_str(), d.cover.agent))
        .collect();
    assert_eq!(
        shown,
        [
            ("~/proj/AGENTS.md", Agent::Codex),
            ("~/proj/AGENTS.md", Agent::Pi),
            ("~/proj/app/CLAUDE.local.md", Agent::Claude),
            ("~/proj/app/AGENTS.override.md", Agent::Codex),
            ("~/proj/app/AGENTS.override.md", Agent::Pi),
        ]
    );
}

#[test]
fn pis_global_context_file_is_read() {
    let t = fixture();
    let docs = read_docs(&t, &[], ".claude", &[Agent::Pi]);
    let got: Vec<(&str, &RuleCover)> = docs
        .iter()
        .map(|d| (d.display_path.as_str(), &d.cover))
        .collect();
    assert_eq!(got, [("~/.pi/agent/AGENTS.md", &cover(Agent::Pi, None))]);
}

#[test]
fn an_import_keeps_its_importers_reach_a_code_span_is_not_one() {
    let t = fixture();
    let docs = read_docs(&t, &[], ".claude", &CLAUDE_CODEX);
    assert_eq!(docs[1].display_path, "~/.agents/skills/rm.md");
    assert_eq!(docs[1].cover, cover(Agent::Claude, None));
    assert!(!docs.iter().any(|d| d.display_path.contains("missing")));
}

#[test]
fn ancestors_and_claude_local_of_a_project_dir_are_read() {
    let t = fixture();
    let docs = read_docs(&t, &[t.path("u/proj/app")], ".claude", &CLAUDE_CODEX);
    let shown: Vec<(&str, Option<&str>)> = docs
        .iter()
        .map(|d| (d.display_path.as_str(), d.cover.dir.as_deref()))
        .collect();
    let app = t.path("u/proj/app");
    let proj = t.path("u/proj");
    assert!(shown.contains(&("~/proj/app/CLAUDE.local.md", Some(app.as_str()))));
    assert!(shown.contains(&("~/proj/AGENTS.md", Some(proj.as_str()))));
}

#[test]
fn a_moved_claude_home_still_finds_the_users_home() {
    let t = fixture();
    let docs = read_docs(&t, &[t.path("u/proj/app")], ".claude-work", &CLAUDE_CODEX);
    let shown: Vec<&str> = docs.iter().map(|d| d.display_path.as_str()).collect();
    assert!(shown.contains(&"~/proj/AGENTS.md"));
    assert!(shown.contains(&"~/.agents/skills/rm.md"));
}
