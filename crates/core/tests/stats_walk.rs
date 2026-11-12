//! Stats discovery on disk: nested transcripts, depth-0 files, symlinks.
#![cfg(unix)]

mod common;

use common::{TempDir, claude_roots};
use peektrace_core::Agent;
use peektrace_core::stats::schema::Kind;
use peektrace_core::stats::walk::list_transcripts;

#[test]
fn nested_walk_follows_symlinks_and_skips_journals() {
    let t = TempDir::new("walk");
    t.write("projects/-w/main.jsonl", "{}\n");
    t.write("projects/-w/notes.txt", "x\n");
    t.write("projects/-w/s1/subagents/agent-1.jsonl", "{}\n");
    t.write("projects/-w/s1/journal.jsonl", "{}\n");
    t.write(
        "projects/-w/s1/subagents/workflows/wf_a/agent-2.jsonl",
        "{}\n",
    );
    t.write("elsewhere/linked/agent-3.jsonl", "{}\n");
    t.write("elsewhere/agent-4.jsonl", "{}\n");
    std::os::unix::fs::symlink(t.join("elsewhere/linked"), t.join("projects/-w/s2"))
        .expect("dir link");
    std::os::unix::fs::symlink(
        t.join("elsewhere/agent-4.jsonl"),
        t.join("projects/-w/s1/agent-4.jsonl"),
    )
    .expect("file link");
    std::os::unix::fs::symlink(t.join("gone"), t.join("projects/-w/s1/dead.jsonl"))
        .expect("dead link");
    let roots = claude_roots(&t.str(), &t.join("projects"));
    let got: Vec<(String, Kind)> = list_transcripts(Agent::Claude, &roots)
        .into_iter()
        .map(|r| (r.path.replace(&t.join("projects/-w/"), ""), r.kind))
        .collect();
    assert_eq!(
        got,
        [
            ("main.jsonl".to_owned(), Kind::Main),
            ("s1/agent-4.jsonl".to_owned(), Kind::Subagent),
            ("s1/subagents/agent-1.jsonl".to_owned(), Kind::Subagent),
            (
                "s1/subagents/workflows/wf_a/agent-2.jsonl".to_owned(),
                Kind::WorkflowAgent
            ),
            ("s2/agent-3.jsonl".to_owned(), Kind::Subagent),
        ]
    );
}
