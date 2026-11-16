//! Stats passes over Codex, Pi and `OpenCode` sessions: how each agent's
//! calls become facts, its project key, and what the fact cache keeps.

mod common;

use common::{TempDir, create_db};
use peektrace_core::js::time::Utc;
use peektrace_core::roots::{RootsEnv, Settings};
use peektrace_core::stats::cache::StatsCache;
use peektrace_core::stats::pass::{PassArgs, PassEnv, PassResult, run_pass, scope_of};
use peektrace_core::stats::schema::{CallFact, Fact};
use peektrace_core::stats::service::{ReportRequest, StatsEnv, report};
use peektrace_core::{Agent, AgentRoots};
use rusqlite::params;
use serde_json::{Value, json};

const CWD: &str = "/home/demo/work.app";
const T0: &str = "2026-10-20T09:00:";
const CODEX: &str = ".codex/sessions/2026/10/20/rollout-2026-10-20T09-00-00-cx1.jsonl";
const PI_SLUG: &str = "--home-demo-work--";

fn ts(sec: u32) -> String {
    format!("{T0}{sec:02}.000Z")
}

fn jsonl(rows: &[Value]) -> String {
    rows.iter().map(|r| r.to_string() + "\n").collect()
}

/// A rollout whose calls are `(name, arguments, output)`.
fn codex_rollout(calls: &[(&str, Value, &str)]) -> String {
    let mut rows = vec![
        json!({"timestamp": ts(0), "type": "session_meta", "payload": {"id": "cx1", "cwd": CWD}}),
        json!({"timestamp": ts(0), "type": "turn_context", "payload": {"model": "gpt-5.5", "cwd": CWD}}),
        json!({"timestamp": ts(0), "type": "response_item", "payload": {"type": "message", "role": "user",
            "content": [{"type": "input_text", "text": "run the checks"}]}}),
    ];
    for (i, (name, args, out)) in (1u32..).zip(calls) {
        rows.push(json!({"timestamp": ts(i * 3), "type": "response_item", "payload": {
            "type": "function_call", "name": name, "call_id": format!("c{i}"), "arguments": args.to_string()}}));
        rows.push(
            json!({"timestamp": ts(i * 3 + 1), "type": "response_item", "payload": {
            "type": "function_call_output", "call_id": format!("c{i}"), "output": out}}),
        );
    }
    jsonl(&rows)
}

/// A Pi session whose calls are `(tool, arguments, output, isError)`.
fn pi_session(calls: &[(&str, Value, &str, bool)]) -> String {
    let mut rows = vec![
        json!({"type": "session", "version": 3, "id": "pi1", "timestamp": ts(0), "cwd": CWD}),
        json!({"type": "message", "id": "u0", "parentId": null, "timestamp": ts(0),
            "message": {"role": "user", "content": "fix it"}}),
    ];
    let mut parent = "u0".to_owned();
    for (i, (tool, args, out, err)) in (1u32..).zip(calls) {
        rows.push(json!({"type": "message", "id": format!("a{i}"), "parentId": parent, "timestamp": ts(i * 3),
            "message": {"role": "assistant", "model": "claude-sonnet-4-5",
                "usage": {"input": 3, "cacheRead": 100, "cacheWrite": 0, "output": 5},
                "content": [{"type": "toolCall", "id": format!("t{i}"), "name": tool, "arguments": args}]}}));
        rows.push(json!({"type": "message", "id": format!("r{i}"), "parentId": format!("a{i}"), "timestamp": ts(i * 3 + 1),
            "message": {"role": "toolResult", "toolCallId": format!("t{i}"), "toolName": tool,
                "content": [{"type": "text", "text": out}], "isError": err}}));
        parent = format!("r{i}");
    }
    jsonl(&rows)
}

/// An `OpenCode` DB with one session whose one `bash` part failed.
fn write_opencode_db(data: &str) {
    let db = create_db(std::path::Path::new(&format!("{data}/opencode.db")));
    db.execute(
        "INSERT INTO session (id, directory, title, time_created, time_updated) VALUES (?, ?, ?, ?, ?)",
        params!["ses_a", CWD, "oc", 1_792_400_000_000_i64, 1_792_400_010_000_i64],
    )
    .expect("session");
    let msg = json!({"role": "assistant", "modelID": "claude-sonnet-4-5", "providerID": "anthropic",
        "tokens": {"input": 3, "output": 5, "reasoning": 0, "cache": {"read": 10, "write": 0}},
        "time": {"created": 1_792_400_001_000_i64}});
    db.execute(
        "INSERT INTO message (id, session_id, time_created, data) VALUES (?, ?, ?, ?)",
        params!["m1", "ses_a", 1_792_400_001_000_i64, msg.to_string()],
    )
    .expect("message");
    let part = json!({"type": "tool", "tool": "bash", "callID": "call_1", "state": {"status": "error",
        "input": {"command": "jqq . x.json"}, "output": "zsh:1: command not found: jqq",
        "time": {"start": 1_792_400_001_000_i64, "end": 1_792_400_002_000_i64}}});
    db.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, data) VALUES (?, ?, ?, ?, ?)",
        params!["p1", "m1", "ses_a", 1_792_400_001_000_i64, part.to_string()],
    )
    .expect("part");
}

struct Corpus {
    dir: TempDir,
    roots: AgentRoots,
}

impl Corpus {
    fn new(tag: &str) -> Self {
        let dir = TempDir::new(tag);
        dir.mkdir("home/.claude/projects");
        dir.mkdir("oc");
        let roots = Self::roots_of(&dir);
        Self { dir, roots }
    }

    fn roots_of(dir: &TempDir) -> AgentRoots {
        let env = RootsEnv {
            home: dir.join("home"),
            opencode_data: Some(dir.join("oc")),
            ..RootsEnv::default()
        };
        AgentRoots::resolve(&env, &Settings::default())
    }

    fn write(&self, rel: &str, body: &str) -> String {
        self.dir.write(&format!("home/{rel}"), body)
    }

    fn cache(&self) -> StatsCache {
        StatsCache::new(self.dir.join("pdir/stats-rs"))
    }

    fn pass(&self, agents: Option<&[Agent]>) -> (PassResult, Vec<Fact>) {
        let cache = self.cache();
        let env = PassEnv {
            roots: &self.roots,
            cache: &cache,
            local: &Utc,
            progress: None,
        };
        let scope = scope_of(agents);
        let args = PassArgs {
            scope: &scope,
            force: false,
            from_ms: 0.0,
            to_ms: f64::MAX,
            only: None,
        };
        let mut facts = Vec::new();
        let r = run_pass(&env, &args, &mut |f| facts.push(f.clone()));
        (r, facts)
    }

    fn indexed(&self) -> Vec<String> {
        self.cache().load().keys().cloned().collect()
    }
}

fn calls(facts: &[Fact]) -> Vec<&CallFact> {
    facts.iter().filter_map(Fact::as_call).collect()
}

fn projects(facts: &[Fact]) -> Vec<&str> {
    let mut out: Vec<&str> = facts
        .iter()
        .filter_map(|f| match f {
            Fact::Session(s) => Some(s.base.project.as_str()),
            Fact::Call(c) => Some(c.base.project.as_str()),
            Fact::Usage(_) => None,
        })
        .collect();
    out.dedup();
    out
}

#[test]
fn codex_argv_shell_calls_become_bash_facts_and_are_cached() {
    let c = Corpus::new("stats-agents-codex");
    let path = c.write(
        CODEX,
        &codex_rollout(&[
            (
                "shell",
                json!({"command": ["bash", "-lc", "jqq . x.json"]}),
                "Exit code 127\nzsh:1: command not found: jqq",
            ),
            ("local_shell", json!({"command": ["ls", "-la"]}), "ok"),
            (
                "exec_command",
                json!({"cmd": "jq . x.json"}),
                "exited with code 2",
            ),
        ]),
    );
    let (r, facts) = c.pass(Some(&[Agent::Codex]));
    assert_eq!((r.transcripts, r.extracted, r.cached), (1, 1, 0));
    assert_eq!(projects(&facts), ["-home-demo-work-app"]);
    let calls = calls(&facts);
    let shape: Vec<(&str, Option<&str>, bool, Option<&str>)> = calls
        .iter()
        .map(|f| {
            (
                f.tool.as_str(),
                f.bash.as_ref().map(|b| b.cmd.as_str()),
                f.err,
                f.arg.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        shape,
        [
            ("shell", Some("jqq . x.json"), true, None),
            ("local_shell", Some("ls -la"), false, None),
            ("exec_command", None, true, Some("jq . x.json")),
        ]
    );
    assert_eq!(c.indexed(), [path]);
    let (again, _) = c.pass(Some(&[Agent::Codex]));
    assert_eq!((again.extracted, again.cached), (0, 1));
}

#[test]
fn pi_keeps_its_slug_and_a_lowercase_edit_is_an_unclassified_cause() {
    let c = Corpus::new("stats-agents-pi");
    c.write(
        &format!(".pi/agent/sessions/{PI_SLUG}/2026-10-20T09-00-00-000Z_pi1.jsonl"),
        &pi_session(&[
            (
                "edit",
                json!({"path": "/w/a.ts", "oldText": "x", "newText": "y"}),
                "Could not find the exact text",
                true,
            ),
            (
                "edit",
                json!({"path": "/w/a.ts", "oldText": "x\n", "newText": "y"}),
                "ok",
                false,
            ),
            ("bash", json!({"command": "ls"}), "a.ts", false),
        ]),
    );
    let (_, facts) = c.pass(Some(&[Agent::Pi]));
    assert_eq!(projects(&facts), [PI_SLUG]);
    let tools: Vec<(&str, bool, bool)> = calls(&facts)
        .iter()
        .map(|f| (f.tool.as_str(), f.err, f.bash.is_some()))
        .collect();
    assert_eq!(
        tools,
        [
            ("edit", true, false),
            ("edit", false, false),
            ("bash", false, true)
        ]
    );
    let env = StatsEnv {
        roots: &c.roots,
        cache: c.cache(),
        local: &Utc,
        now_ms: 1_792_500_000_000.0,
        progress: None,
    };
    let rep = report(
        &env,
        &ReportRequest {
            agents: Some(vec![Agent::Pi]),
            days: Some(36500.0),
            force: false,
            limit: None,
        },
    )
    .expect("report");
    let json = rep.to_json(false);
    assert!(json.contains("\"cause:unclassified:edit\""), "{json}");
}

#[test]
fn opencode_keeps_the_raw_directory_and_is_extracted_every_pass() {
    let c = Corpus::new("stats-agents-oc");
    write_opencode_db(&c.dir.join("oc"));
    let (r, facts) = c.pass(Some(&[Agent::Opencode]));
    assert_eq!((r.transcripts, r.extracted, r.cached), (1, 1, 0));
    assert_eq!(projects(&facts), [CWD]);
    let call = calls(&facts)[0];
    assert_eq!(
        (
            call.tool.as_str(),
            call.err,
            call.bash.as_ref().map(|b| b.cmd.as_str())
        ),
        ("bash", true, Some("jqq . x.json"))
    );
    assert_eq!(c.indexed(), Vec::<String>::new());
    let (again, _) = c.pass(Some(&[Agent::Opencode]));
    assert_eq!((again.extracted, again.cached), (1, 0));
}

#[test]
fn an_opencode_session_gone_after_listing_is_an_empty_session_not_skipped() {
    let c = Corpus::new("stats-agents-oc-gone");
    write_opencode_db(&c.dir.join("oc"));
    let cache = c.cache();
    let env = PassEnv {
        roots: &c.roots,
        cache: &cache,
        local: &Utc,
        progress: None,
    };
    let scope = [Agent::Opencode];
    let args = PassArgs {
        scope: &scope,
        force: false,
        from_ms: 0.0,
        to_ms: f64::MAX,
        only: None,
    };
    let mut refs = peektrace_core::stats::pass::pass_refs(&env, &args);
    refs[0].path = format!("{}#ses_gone", c.dir.join("oc"));
    refs[0].id = "ses_gone".to_owned();
    let mut facts = Vec::new();
    let r = peektrace_core::stats::pass::fold_refs(&env, &args, &refs, &mut |f| {
        facts.push(f.clone());
    });
    assert_eq!((r.extracted, r.skipped.len()), (1, 0));
    assert!(matches!(facts.as_slice(), [Fact::Session(s)] if s.base.sid == "ses_gone"));
}

#[test]
fn a_claude_only_pass_keeps_other_agents_in_the_index() {
    let c = Corpus::new("stats-agents-narrow");
    let codex = c.write(
        CODEX,
        &codex_rollout(&[("shell", json!({"command": ["ls"]}), "ok")]),
    );
    let claude = c.write(
        ".claude/projects/-w/sess-1.jsonl",
        &jsonl(&[json!({"type": "user", "cwd": "/w", "timestamp": ts(0),
            "message": {"content": "hello"}})]),
    );
    let (all, _) = c.pass(None);
    assert_eq!(all.transcripts, 2);
    let mut both = vec![claude.clone(), codex.clone()];
    both.sort();
    let mut indexed = c.indexed();
    indexed.sort();
    assert_eq!(indexed, both);
    let (narrow, _) = c.pass(Some(&[Agent::Claude]));
    assert_eq!((narrow.transcripts, narrow.cached), (1, 1));
    let mut indexed = c.indexed();
    indexed.sort();
    assert_eq!(indexed, both);
}
