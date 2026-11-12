//! Stats helpers: synthetic fact rows and a synthetic Claude transcript
//! built from tool-call steps. Every string here is made up.

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
