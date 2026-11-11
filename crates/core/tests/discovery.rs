//! Codex and Pi roots and file discovery, ported from the TS tests
//! `agents-walkers.test.ts`, the Codex/Pi cases of `agents.test.ts`, and the
//! unreadable-row case of `swallowed-errors.test.ts`.

mod common;

use std::fs;
use std::path::Path;

use common::temp_dir;
use peektrace_core::listing::session_files;
use peektrace_core::roots::{RootEntry, RootsEnv, Settings, Source, sources};
use peektrace_core::{Agent, AgentRoots, ListFilter, list_sessions};

const CODEX_ID: &str = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const PI_ID: &str = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const HOME: &str = "/home/u";

fn write(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, body).expect("write");
}

fn roots_at(base: &Path) -> AgentRoots {
    let env = RootsEnv {
        home: "/nonexistent-home".to_owned(),
        claude_projects: Some(base.join("claude").to_string_lossy().into_owned()),
        codex_sessions: Some(base.join("codex").to_string_lossy().into_owned()),
        pi_sessions: Some(base.join("pi").to_string_lossy().into_owned()),
        ..RootsEnv::default()
    };
    AgentRoots::resolve(&env, &Settings::default())
}

fn sorted_ids(agent: Agent, roots: &AgentRoots) -> (Vec<String>, Vec<String>) {
    let refs = session_files(agent, roots, None);
    let mut ids: Vec<_> = refs.iter().map(|r| r.id.as_str().to_owned()).collect();
    ids.sort();
    let slugs = refs.iter().map(|r| r.slug.as_str().to_owned()).collect();
    (ids, slugs)
}

// agents-walkers.test.ts

fn walker_tree() -> std::path::PathBuf {
    let base = temp_dir("walkers");
    let codex = base.join("codex/2026/01/02");
    write(
        &codex.join(format!("rollout-2026-01-02T00-00-00-{CODEX_ID}.jsonl")),
        "{\"type\":\"session_meta\"}\n",
    );
    write(&codex.join("rollout-demo.jsonl"), "{}\n");
    write(&codex.join("notes.txt"), "ignored");
    let pi = base.join("pi/--demo-project--");
    write(
        &pi.join(format!("2026-01-02T00-00-00-000Z_{PI_ID}.jsonl")),
        "{\"type\":\"session\"}\n",
    );
    write(&pi.join("plain.jsonl"), "{}\n");
    base
}

#[test]
fn codex_walks_the_date_tree_keeps_only_rollout_files() {
    let base = walker_tree();
    let (ids, slugs) = sorted_ids(Agent::Codex, &roots_at(&base));
    let mut want = [CODEX_ID, "rollout-demo"];
    want.sort_unstable();
    assert_eq!(ids, want);
    assert!(slugs.iter().all(String::is_empty));
    let _ = fs::remove_dir_all(&base);
}

#[test]
fn pi_lists_slug_dirs_id_from_after_the_underscore() {
    let base = walker_tree();
    let (ids, slugs) = sorted_ids(Agent::Pi, &roots_at(&base));
    assert!(slugs.iter().all(|s| s == "--demo-project--"));
    let mut want = [PI_ID, "plain"];
    want.sort_unstable();
    assert_eq!(ids, want);
    let _ = fs::remove_dir_all(&base);
}

// agents.test.ts, Codex and Pi

fn env() -> RootsEnv {
    RootsEnv {
        home: HOME.to_owned(),
        ..RootsEnv::default()
    }
}

fn default_of(agent: Agent, e: &RootsEnv) -> Source {
    sources(agent, e, &[]).remove(0)
}

#[test]
fn defaults_to_the_agent_dir_under_home_when_no_env_is_set() {
    let claude = default_of(Agent::Claude, &env());
    assert_eq!(claude.home, "/home/u/.claude");
    assert_eq!(claude.projects_root, "/home/u/.claude/projects");
    let codex = default_of(Agent::Codex, &env());
    assert_eq!(codex.home, "/home/u/.codex");
    assert_eq!(codex.projects_root, "/home/u/.codex/sessions");
    assert_eq!(
        default_of(Agent::Pi, &env()).projects_root,
        "/home/u/.pi/agent/sessions"
    );
}

#[test]
fn honours_codex_home() {
    let e = RootsEnv {
        codex_home: Some("/custom/codex".to_owned()),
        ..env()
    };
    assert_eq!(
        default_of(Agent::Codex, &e).projects_root,
        "/custom/codex/sessions"
    );
}

#[test]
fn derives_codex_and_pi_extra_roots_per_layout() {
    let entry = |path: &str, label: Option<&str>| RootEntry {
        path: path.to_owned(),
        label: label.map(str::to_owned),
    };
    let codex = sources(Agent::Codex, &env(), &[entry("/work/.codex", Some("work"))]);
    assert_eq!(codex[1].projects_root, "/work/.codex/sessions");
    let pi = sources(
        Agent::Pi,
        &env(),
        &[entry("/work/pi-extra", None), entry("  ", None)],
    );
    let roots: Vec<_> = pi.iter().map(|s| s.projects_root.as_str()).collect();
    assert_eq!(
        roots,
        [
            "/home/u/.pi/agent/sessions",
            "/work/pi-extra/agent/sessions"
        ]
    );
    assert_eq!(pi[1].label, "pi-extra");
}

// swallowed-errors.test.ts: an unreadable row with no slug keeps an empty
// project. A Codex ref has no slug, so its unreadable row has project "" and
// is kept under any `--project`.

#[cfg(unix)]
#[test]
fn an_unreadable_codex_row_keeps_an_empty_project() {
    use std::os::unix::fs::PermissionsExt as _;
    let base = temp_dir("unreadable");
    let file = base.join("codex/2026/01/02/rollout-locked.jsonl");
    write(&file, "{}\n");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).expect("chmod");
    if fs::read(&file).is_ok() {
        // Running as root: every file is readable.
        let _ = fs::remove_dir_all(&base);
        return;
    }
    let listing = list_sessions(
        &roots_at(&base),
        &ListFilter {
            agent: None,
            project: Some("anything".to_owned()),
        },
    );
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).expect("chmod");
    let rows: Vec<_> = listing
        .unreadable
        .iter()
        .map(|u| (u.agent, u.project.as_str()))
        .collect();
    assert_eq!(rows, [(Agent::Codex, "")]);
    let _ = fs::remove_dir_all(&base);
}
