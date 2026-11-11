//! Helpers shared by the ported TS session tests, plus throwaway `OpenCode`
//! data dirs and SQLite stores built with the DDL of
//! `packages/core/test/fixtures/opencode-db.ts`.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use peektrace_core::claude::{ParseArgs, parse_claude_session};
use peektrace_core::js::time::Utc;
use peektrace_core::roots;
use peektrace_core::roots::{RootsEnv, Source, claude_sources};
use peektrace_core::sessions::analyze::{AnalyzeOptions, analyze};
use peektrace_core::sessions::model::{AnalyzedSession, EventKind, ParsedSession, TimelineEvent};
use peektrace_core::sessions::service::SessionEnv;
use peektrace_core::{
    Agent, AgentRoots, FileStamp, ListFilter, ProjectSlug, SessionFileRef, SessionHeader,
    SessionId, list_sessions,
};
use rusqlite::{Connection, params};
use serde_json::Value;

/// A path under `packages/core/test/fixtures`.
pub fn fixture(sub: &str) -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/core/test/fixtures")
        .join(sub)
        .to_string_lossy()
        .into_owned()
}

pub fn read_fixture(sub: &str) -> String {
    std::fs::read_to_string(fixture(sub)).expect("read fixture")
}

/// Rows as `rows.map(JSON.stringify).join("\n")`.
pub fn jsonl(rows: &[Value]) -> String {
    rows.iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_text(text: &str) -> ParsedSession {
    parse_claude_session(&ParseArgs {
        text,
        path: "/demo/s.jsonl",
        session_id: "s",
        ..ParseArgs::default()
    })
}

pub fn parse_rows(rows: &[Value]) -> ParsedSession {
    parse_text(&jsonl(rows))
}

pub fn parse_sidechain(text: &str) -> ParsedSession {
    parse_claude_session(&ParseArgs {
        text,
        path: "/demo/s.jsonl",
        session_id: "s",
        include_sidechain_turns: true,
        ..ParseArgs::default()
    })
}

pub fn parse_fixture(sub: &str) -> ParsedSession {
    let path = fixture(sub);
    let text = read_fixture(sub);
    parse_claude_session(&ParseArgs {
        text: &text,
        path: &path,
        session_id: "s1",
        ..ParseArgs::default()
    })
}

pub fn analyze_default(p: ParsedSession) -> AnalyzedSession {
    analyze(
        p,
        AnalyzeOptions {
            dumb_zone_fraction: None,
            on_disk_context_files: Vec::new(),
            window: None,
            local: &Utc,
        },
    )
}

pub fn of_kind(p: &ParsedSession, kind: EventKind) -> Vec<&TimelineEvent> {
    p.events.iter().filter(|e| e.kind == kind).collect()
}

pub fn first_of(p: &ParsedSession, kind: EventKind) -> &TimelineEvent {
    p.events
        .iter()
        .find(|e| e.kind == kind)
        .unwrap_or_else(|| panic!("no {} event", kind.as_str()))
}

/// Claude roots with `projects` as the default projects root.
pub fn sources(home: &str, projects: &str) -> Vec<Source> {
    let env = RootsEnv {
        home: home.to_owned(),
        claude_projects: Some(projects.to_owned()),
        ..RootsEnv::default()
    };
    claude_sources(&env, &[])
}

/// Roots with only the Claude sources of [`sources`].
pub fn claude_roots(home: &str, projects: &str) -> AgentRoots {
    AgentRoots {
        claude: sources(home, projects),
        ..AgentRoots::default()
    }
}

const SESSION_DDL: &str = "CREATE TABLE session (id TEXT PRIMARY KEY, project_id TEXT, directory TEXT, title TEXT, version TEXT, model TEXT, agent TEXT, cost REAL, tokens_input INTEGER, tokens_output INTEGER, tokens_reasoning INTEGER, tokens_cache_read INTEGER, tokens_cache_write INTEGER, time_created INTEGER, time_updated INTEGER)";
const MESSAGE_DDL: &str = "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT)";
const PART_DDL: &str = "CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT)";

pub fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

pub fn create_db(path: &Path) -> Connection {
    let db = Connection::open(path).expect("open");
    for ddl in [SESSION_DDL, MESSAGE_DDL, PART_DDL] {
        db.execute(ddl, []).expect("ddl");
    }
    db
}

/// `writeGoodDb`: one text-only session.
pub fn write_good_db(path: &Path, id: &str, directory: &str) {
    let db = create_db(path);
    db.execute(
        "INSERT INTO session (id, directory, title, time_created, time_updated) VALUES (?, ?, ?, ?, ?)",
        params![id, directory, "Good session", 1000, 2000],
    )
    .expect("session");
    db.execute(
        "INSERT INTO message (id, session_id, time_created, data) VALUES (?, ?, ?, ?)",
        params![
            format!("{id}-m"),
            id,
            1500,
            r#"{"role":"user","time":{"created":1500}}"#
        ],
    )
    .expect("message");
    db.execute(
        "INSERT INTO part (id, message_id, session_id, time_created, data) VALUES (?, ?, ?, ?, ?)",
        params![
            format!("{id}-p"),
            format!("{id}-m"),
            id,
            1501,
            format!(r#"{{"type":"text","text":"hi from {id}"}}"#)
        ],
    )
    .expect("part");
}

pub fn write_json(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, body).expect("write");
}

/// Roots with only `data` as the `OpenCode` data dir.
pub fn opencode_roots(data: &str) -> AgentRoots {
    let env = RootsEnv {
        home: "/nonexistent-home".to_owned(),
        opencode_data: Some(data.to_owned()),
        ..RootsEnv::default()
    };
    AgentRoots {
        opencode: roots::sources(Agent::Opencode, &env, &[]),
        ..AgentRoots::default()
    }
}

/// `OpenCode` list headers under `data`.
pub fn headers(data: &str) -> Vec<SessionHeader> {
    list_sessions(
        &opencode_roots(data),
        &ListFilter {
            agent: Some(Agent::Opencode),
            project: None,
        },
    )
    .headers
}

pub fn ids(refs: &[SessionFileRef]) -> Vec<&str> {
    refs.iter().map(|r| r.id.as_str()).collect()
}

pub fn session_env(roots: &AgentRoots) -> SessionEnv<'_> {
    SessionEnv { roots, local: &Utc }
}

/// A [`temp_dir`] removed on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> Self {
        Self(temp_dir(tag))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn str(&self) -> String {
        self.0.to_string_lossy().into_owned()
    }

    /// Write `body` at `rel`, creating parent dirs; returns the full path.
    pub fn write(&self, rel: &str, body: &str) -> String {
        let p = self.0.join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(&p, body).expect("write");
        p.to_string_lossy().into_owned()
    }

    pub fn mkdir(&self, rel: &str) -> String {
        let p = self.0.join(rel);
        std::fs::create_dir_all(&p).expect("mkdir");
        p.to_string_lossy().into_owned()
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The file ref a header builder takes (`HEADER_ARGS` in the TS tests).
pub fn file_ref(id: &str, slug: &str, path: &str, size_bytes: u64) -> SessionFileRef {
    SessionFileRef {
        id: SessionId::new(id),
        slug: ProjectSlug::new(slug),
        path: path.to_owned(),
        stamp: FileStamp {
            size_bytes,
            mtime_ms: 0,
        },
        source: None,
    }
}

/// A fresh empty dir under the system temp dir, unique per call; the
/// caller removes it.
pub fn temp_dir(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("peektrace-test-{name}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("mkdir");
    dir
}

/// `JSON.stringify(v, null, 2)` for a flat object of string values.
pub fn pretty(pairs: &[(&str, &str)]) -> String {
    let body: Vec<String> = pairs
        .iter()
        .map(|(k, v)| format!("  \"{k}\": \"{v}\""))
        .collect();
    format!("{{\n{}\n}}", body.join(",\n"))
}

// Stats helpers.

use peektrace_core::Agent;
use peektrace_core::claude::{ParseArgs, parse_claude_session};
use peektrace_core::js::time::{Utc, to_iso_string};
use peektrace_core::stats::accumulate::{CorpusSummary, FinishCtx, StatsWindow};
use peektrace_core::stats::extract::{ExtractInput, extract_facts};
use peektrace_core::stats::redact::Redactor;
use peektrace_core::stats::schema::{
    BashPart, CallFact, Fact, FactBase, Job, Kind, SessionFact, UsageFact,
};
use peektrace_core::stats::shell_lex::ran_bins;
use peektrace_core::stats::walk::TranscriptRef;
use serde_json::{Value, json};

pub fn base(sid: &str) -> FactBase {
    FactBase {
        sid: sid.to_owned(),
        agent: Agent::Claude,
        project: "proj".to_owned(),
        kind: Kind::Main,
        parent: None,
        run: None,
    }
}

/// `toolFact({})`: a Read call at 2026-08-01T00:00:00Z, 1 s, clean.
pub fn tool_fact() -> CallFact {
    CallFact {
        base: base("s1"),
        seq: 0.0,
        tool_use_id: None,
        tool: "Read".to_owned(),
        ts: Some("2026-08-01T00:00:00.000Z".to_owned()),
        dur_sec: Some(1.0),
        err: false,
        failed: false,
        fail_reason: None,
        detector: None,
        benign: None,
        sig: None,
        arg: None,
        out_chars: 0.0,
        side: false,
        req: None,
        ctx_tok: None,
        cause: None,
        cause_arg: None,
        focus: None,
        bash: None,
    }
}

/// `bashFact({cmd})`: `ran` follows `cmd`, as extraction would fill it.
pub fn bash_fact(cmd: &str) -> CallFact {
    CallFact {
        tool: "Bash".to_owned(),
        bash: Some(Box::new(BashPart {
            cmd: cmd.to_owned(),
            ran: ran_bins(cmd),
            bin: "true".to_owned(),
            family: "true".to_owned(),
            job: Job::Other,
            present: vec![Job::Other],
            wait: None,
            declared_delay_sec: 0.0,
            piped: false,
            bg: false,
            capped: false,
        })),
        ..tool_fact()
    }
}

/// The bash part of a fact built by `bash_fact`.
pub fn bash_mut(f: &mut CallFact) -> &mut BashPart {
    f.bash.as_deref_mut().expect("a bash fact")
}

pub fn session_fact() -> SessionFact {
    SessionFact {
        base: base("s1"),
        started_at: Some("2026-08-01T00:00:00.000Z".to_owned()),
        ended_at: None,
        title: None,
        cwd: None,
        source: None,
        models: Vec::new(),
        turns: 0.0,
        user_messages: 0.0,
        tool_calls: 0.0,
        input_tokens: 0.0,
        output_tokens: 0.0,
        cache_read_tokens: 0.0,
        cache_creation_tokens: 0.0,
        max_context_tokens: 0.0,
        first_turn_cache_creation: 0.0,
    }
}

pub fn usage_fact() -> UsageFact {
    UsageFact {
        sid: "s1".to_owned(),
        agent: Agent::Claude,
        ts: Some("2026-08-01T00:00:00.000Z".to_owned()),
        req: "req_1".to_owned(),
        model: "claude-sonnet-4-5".to_owned(),
        input_tokens: 100.0,
        output_tokens: 50.0,
        cache_read_tokens: 0.0,
        cache_creation_tokens: 0.0,
    }
}

pub fn call(f: CallFact) -> Fact {
    Fact::Call(f)
}

pub fn window() -> StatsWindow {
    StatsWindow {
        days: 60.0,
        from_iso: String::new(),
        to_iso: String::new(),
    }
}

/// `contextOf()`: an empty corpus, the default limit and sample cap.
pub fn ctx_of<'a>(
    corpus: &'a CorpusSummary,
    window: &'a StatsWindow,
    redact: &'a Redactor,
) -> FinishCtx<'a> {
    FinishCtx {
        corpus,
        window,
        redact,
        limit: 20.0,
        sample_cap: 3,
        caveats: Vec::new(),
    }
}

/// One tool call and its result in a synthetic transcript.
#[derive(Clone)]
pub struct Step {
    /// Seconds after the session start; default 10 per step.
    pub at: Option<i64>,
    /// Context tokens the model call carried; default 10,000.
    pub ctx: Option<i64>,
    pub input: Value,
    pub is_error: bool,
    pub name: String,
    pub output: String,
    /// Request id; a step reusing the previous one is a parallel sibling.
    pub req: Option<String>,
    pub side: bool,
}

pub fn step(name: &str, input: Value, output: &str, is_error: bool) -> Step {
    Step {
        at: None,
        ctx: None,
        input,
        is_error,
        name: name.to_owned(),
        output: output.to_owned(),
        req: None,
        side: false,
    }
}

pub fn bash_step(cmd: &str, output: &str, is_error: bool) -> Step {
    step("Bash", json!({ "command": cmd }), output, is_error)
}

const START_MS: i64 = 1_785_578_400_000; // 2026-08-01T10:00:00.000Z

pub fn transcript_of(steps: &[Step]) -> String {
    let iso = |ms: i64| to_iso_string(ms);
    let mut lines = vec![json!({
        "type": "user",
        "cwd": "/work/proj",
        "timestamp": iso(START_MS),
        "message": { "role": "user", "content": "do the task" },
    })];
    for (i, s) in steps.iter().enumerate() {
        let at = START_MS + s.at.unwrap_or((i64::try_from(i).unwrap_or(0) + 1) * 10) * 1000;
        let id = format!("toolu_{i:03}");
        let ctx = s.ctx.unwrap_or(10_000);
        lines.push(json!({
            "type": "assistant",
            "requestId": s.req.clone().unwrap_or_else(|| format!("req_{i}")),
            "isSidechain": s.side,
            "cwd": "/work/proj",
            "timestamp": iso(at),
            "message": {
                "model": "claude-test",
                "usage": {
                    "input_tokens": 1,
                    "cache_read_input_tokens": ctx - 1,
                    "cache_creation_input_tokens": 0,
                    "output_tokens": 10,
                },
                "content": [{ "type": "tool_use", "id": id, "name": s.name, "input": s.input }],
            },
        }));
        lines.push(json!({
            "type": "user",
            "isSidechain": s.side,
            "timestamp": iso(at + 1000),
            "message": {
                "role": "user",
                "content": [{
                    "type": "tool_result",
                    "tool_use_id": id,
                    "content": s.output,
                    "is_error": s.is_error,
                }],
            },
        }));
    }
    let mut out = lines
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    out.push('\n');
    out
}

pub fn ref_of(sid: &str) -> TranscriptRef {
    TranscriptRef {
        id: sid.to_owned(),
        slug: "proj".to_owned(),
        path: format!("/fixture/proj/{sid}.jsonl"),
        source: None,
        agent: Agent::Claude,
        kind: Kind::Main,
        parent: None,
        run: None,
    }
}

/// Parse `text` as the TS test default does (no sidechain turns, keep the
/// system prompt) and extract its facts.
pub fn facts_of_text(text: &str, sid: &str, redact: bool, skip_system_prompt: bool) -> Vec<Fact> {
    let path = format!("/fixture/proj/{sid}.jsonl");
    let parsed = parse_claude_session(&ParseArgs {
        text,
        path: &path,
        session_id: sid,
        include_sidechain_turns: false,
        skip_system_prompt,
    });
    extract_facts(&ExtractInput {
        r#ref: &ref_of(sid),
        parsed: &parsed,
        project: "proj",
        redact: &Redactor::new(redact),
        local: &Utc,
    })
}

/// `extractFacts(parsedOf(steps, sid))`.
pub fn facts_of(steps: &[Step], sid: &str) -> Vec<Fact> {
    facts_of_text(&transcript_of(steps), sid, true, false)
}

pub fn calls(facts: &[Fact]) -> Vec<&CallFact> {
    facts.iter().filter_map(Fact::as_call).collect()
}
