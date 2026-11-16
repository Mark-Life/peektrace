//! A cached session summary follows every file its analysis read, with no
//! watcher running: context files and subagent transcripts made or changed
//! after the first request show on the next one.

mod common;

use std::path::{Path, PathBuf};

use common::{Options, TestServer, fixtures, start, temp_dir};
use serde_json::Value;

const ID: &str = "33333333-3333-4333-8333-333333333333";

struct Setup {
    server: TestServer,
    work: PathBuf,
    projects: PathBuf,
    session_dir: PathBuf,
}

fn setup(name: &str) -> Setup {
    let root = temp_dir(name);
    let work = root.join("work");
    let projects = root.join("projects");
    let dir = projects.join("-w");
    std::fs::create_dir_all(&work).expect("mkdir work");
    std::fs::create_dir_all(&dir).expect("mkdir projects");
    let cwd = work.to_string_lossy();
    let line = format!(
        r#"{{"type":"user","cwd":"{cwd}","timestamp":"2026-06-01T10:00:00.000Z","message":{{"content":"hi"}}}}"#
    );
    std::fs::write(dir.join(format!("{ID}.jsonl")), line + "\n").expect("write");
    let env = peektrace_core::roots::RootsEnv {
        home: root.to_string_lossy().into_owned(),
        claude_projects: Some(projects.to_string_lossy().into_owned()),
        codex_sessions: Some("/nonexistent-codex".to_owned()),
        pi_sessions: Some("/nonexistent-pi".to_owned()),
        opencode_data: Some("/nonexistent-opencode".to_owned()),
        ..peektrace_core::roots::RootsEnv::default()
    };
    let roots =
        peektrace_core::AgentRoots::resolve(&env, &peektrace_core::roots::Settings::default());
    let server = start(&Options {
        name,
        roots: Some(roots),
        ..Options::default()
    });
    Setup {
        server,
        work,
        projects,
        session_dir: dir.join(ID),
    }
}

fn summary(s: &TestServer) -> Value {
    let a = s.get(&format!("/api/sessions/{ID}/summary"));
    assert_eq!(a.status, 200, "{}", a.body);
    a.json()
}

/// `bytes` of the context file at `path`, if the summary lists it.
fn context_bytes(sum: &Value, path: &Path) -> Option<f64> {
    let want = path.to_string_lossy();
    sum["onDiskContextFiles"]
        .as_array()?
        .iter()
        .find(|f| f["path"].as_str() == Some(want.as_ref()))
        .and_then(|f| f["bytes"].as_f64())
}

#[test]
fn context_file_edits_and_new_files_show() {
    let s = setup("history-fresh-context");
    let claude_md = s.work.join("CLAUDE.md");
    std::fs::write(&claude_md, "short\n").expect("write");
    assert_eq!(context_bytes(&summary(&s.server), &claude_md), Some(6.0));
    std::fs::write(&claude_md, "a longer project instruction file\n").expect("rewrite");
    assert_eq!(context_bytes(&summary(&s.server), &claude_md), Some(34.0));

    let slugs = peektrace_core::sessions::context_files::memory_slugs(&s.work.to_string_lossy());
    let memory = s.projects.join(&slugs[0]).join("memory");
    std::fs::create_dir_all(&memory).expect("mkdir memory");
    let memory_md = memory.join("MEMORY.md");
    std::fs::write(&memory_md, "remember this\n").expect("memory");
    assert_eq!(context_bytes(&summary(&s.server), &memory_md), Some(13.0));
}

#[test]
fn a_subagent_written_later_shows() {
    let s = setup("history-fresh-subagent");
    let count = |sum: &Value| sum["subagents"].as_array().map(Vec::len);
    assert_eq!(count(&summary(&s.server)), Some(0));
    let from = PathBuf::from(fixtures(
        "sessions/projects/-Users-demo-proj/11111111-1111-4111-8111-111111111111/subagents",
    ));
    let to = s.session_dir.join("subagents");
    std::fs::create_dir_all(&to).expect("mkdir subagents");
    for name in ["agent-1.jsonl", "agent-1.meta.json"] {
        std::fs::copy(from.join(name), to.join(name)).expect("copy");
    }
    assert_eq!(count(&summary(&s.server)), Some(1));
}
