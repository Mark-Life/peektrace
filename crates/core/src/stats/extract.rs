//! One parsed transcript as fact rows: a session row, one row per tool call
//! joined to its result, and one usage row per model call. Every derived
//! string is redacted here, before it reaches a shard.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use super::detectors::causes::{CauseHit, detect_cause, is_shell_tool, spec};
use super::detectors::failures::{benign_filter, is_piped, scan_output};
use super::detectors::similarity::FOCUS_SEP;
use super::detectors::timing::{classify_job, is_backgrounded, is_capped};
use super::detectors::waiting::{declared_delay_sec, wait_shape};
use super::normalize::{error_signature, normalize_command};
use super::rank::{round, round_to};
use super::redact::Redactor;
use super::schema::{BashPart, CallFact, Fact, FactBase, SessionFact, UsageFact};
use super::shell_lex::ran_bins;
use super::walk::TranscriptRef;
use crate::js::regex::LazyRe;
use crate::js::time::{LocalZone, parse_date};
use crate::js::units;
use crate::js::value::{JsObject, JsValue};
use crate::js_re;
use crate::sessions::model::{EventKind, ParsedSession, TimelineEvent};

const CMD_MAX: usize = 400;
const ARG_MAX: usize = 200;
const CAUSE_ARG_MAX: usize = 80;
const FOCUS_MAX: usize = 200;
const FOCUS_SEGMENTS: usize = 3;

const ARG_KEYS: [&str; 9] = [
    "file_path",
    "path",
    "pattern",
    "url",
    "query",
    "subagent_type",
    "notebook_path",
    "command",
    "cmd",
];

static SHELL_FLAG: LazyRe = js_re!(r"^-{1,2}[lc]{1,2}$");

struct PendingCall<'a> {
    id: &'a str,
    input: &'a str,
    req: Option<&'a str>,
    side: bool,
    tool: &'a str,
    ts: Option<&'a str>,
}

fn as_object(text: &str) -> Option<JsObject> {
    if !text.starts_with('{') {
        return None;
    }
    match JsValue::parse(text)? {
        JsValue::Obj(o) => Some(o),
        _ => None,
    }
}

/// The shell line of a call: a string `command`, or an argv array.
fn command_of(input: Option<&JsObject>) -> String {
    match input.and_then(|i| i.get("command")) {
        Some(JsValue::Str(s)) => s.clone(),
        Some(JsValue::Arr(items)) => {
            let parts: Vec<&str> = items.iter().filter_map(JsValue::as_str).collect();
            if parts.len() > 1 && SHELL_FLAG.is_match(parts[1]) {
                parts.last().copied().unwrap_or("").to_owned()
            } else {
                parts.join(" ")
            }
        }
        _ => String::new(),
    }
}

fn arg_of(input: Option<&JsObject>) -> Option<&str> {
    let input = input?;
    ARG_KEYS.iter().find_map(|k| {
        input
            .get(k)
            .and_then(JsValue::as_str)
            .filter(|s| !s.is_empty())
    })
}

#[expect(clippy::cast_precision_loss, reason = "epoch ms fit f64 exactly")]
fn duration_of(call: Option<&str>, result: Option<&str>, local: &dyn LocalZone) -> Option<f64> {
    let (call, result) = (
        call.filter(|s| !s.is_empty())?,
        result.filter(|s| !s.is_empty())?,
    );
    let a = parse_date(call, local)?;
    let b = parse_date(result, local)?;
    Some(round_to(((b - a) as f64 / 1000.0).max(0.0), 3))
}

fn focus_of(hit: Option<&CauseHit>, redact: &Redactor) -> Option<String> {
    let focus = hit?.focus.as_deref()?;
    let parts: Vec<String> = focus
        .split(FOCUS_SEP)
        .take(FOCUS_SEGMENTS)
        .map(|s| redact.line_max(s, FOCUS_MAX))
        .filter(|s| !s.is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join(FOCUS_SEP))
}

struct Ctx<'a> {
    base: FactBase,
    redact: &'a Redactor,
    local: &'a dyn LocalZone,
}

#[expect(clippy::cast_precision_loss, reason = "counts fit f64")]
fn call_fact(
    ctx: &Ctx<'_>,
    seq: usize,
    call: &PendingCall<'_>,
    result: Option<&TimelineEvent>,
    ctx_tok: Option<f64>,
) -> CallFact {
    let redact = ctx.redact;
    let output = result.map_or("", |r| r.body.as_str());
    let err = result.is_some_and(|r| r.is_error == Some(true));
    let scan = scan_output(call.tool, output, err);
    let input = as_object(call.input);
    let arg = arg_of(input.as_ref());
    let shell = is_shell_tool(call.tool);
    let cmd = if shell {
        command_of(input.as_ref())
    } else {
        String::new()
    };
    let scrub = |s: &str| redact.text(s);
    let hit = detect_cause(call.tool, &cmd, output, &scrub);
    let failed = scan.failed || hit.as_ref().is_some_and(|h| spec(h.cause).failing);
    let sig = if failed || err {
        let text = hit
            .as_ref()
            .and_then(|h| h.line.as_deref())
            .unwrap_or(output);
        error_signature(text, call.tool)
    } else {
        None
    };
    let dur_sec = duration_of(call.ts, result.and_then(|r| r.ts.as_deref()), ctx.local);
    let bash = shell.then(|| {
        let norm = normalize_command(&cmd);
        let (job, present) = classify_job(&cmd);
        let cmd_line = redact.line_max(&cmd, CMD_MAX);
        let bg = is_backgrounded(&cmd)
            || input
                .as_ref()
                .and_then(|i| i.get("run_in_background"))
                .is_some_and(JsValue::is_true);
        Box::new(BashPart {
            ran: ran_bins(&cmd_line),
            cmd: cmd_line,
            bin: norm.bin,
            family: redact.text(&norm.family),
            job,
            present,
            wait: wait_shape(&cmd),
            declared_delay_sec: round(declared_delay_sec(&cmd)),
            piped: is_piped(&cmd),
            bg,
            capped: is_capped(dur_sec),
        })
    });
    let benign = shell
        .then(|| benign_filter(&cmd, output, hit.is_some()))
        .flatten()
        .map(str::to_owned);
    CallFact {
        base: ctx.base.clone(),
        seq: seq as f64,
        tool_use_id: Some(call.id.to_owned()),
        tool: call.tool.to_owned(),
        ts: call.ts.map(str::to_owned),
        dur_sec,
        err,
        failed,
        fail_reason: scan.reason.map(str::to_owned),
        detector: None,
        benign,
        sig: sig.map(|s| redact.line(&s)),
        arg: arg.map(|a| redact.line_max(a, ARG_MAX)),
        out_chars: units::len(output) as f64,
        side: call.side,
        req: call.req.map(str::to_owned),
        ctx_tok,
        cause: hit.as_ref().map(|h| h.cause),
        cause_arg: hit
            .as_ref()
            .and_then(|h| h.arg.as_deref())
            .map(|a| redact.line_max(a, CAUSE_ARG_MAX)),
        focus: focus_of(hit.as_ref(), redact),
        bash,
    }
}

/// What one transcript's facts are built from.
pub struct ExtractInput<'a> {
    pub r#ref: &'a TranscriptRef,
    pub parsed: &'a ParsedSession,
    pub project: &'a str,
    pub redact: &'a Redactor,
    pub local: &'a dyn LocalZone,
}

#[expect(clippy::cast_precision_loss, reason = "counts fit f64")]
fn session_fact(input: &ExtractInput<'_>, base: FactBase, tool_calls: usize) -> SessionFact {
    let p = input.parsed;
    let mut f = SessionFact {
        base,
        started_at: p.started_at.clone(),
        ended_at: p.ended_at.clone(),
        title: input.redact.nullable(p.title.as_deref()),
        cwd: input.redact.nullable(p.cwd.as_deref()),
        source: input.r#ref.source.as_ref().map(|s| s.id.clone()),
        models: p.models.clone(),
        turns: p.turns.len() as f64,
        user_messages: p.events.iter().filter(|e| e.is_user_authored()).count() as f64,
        tool_calls: tool_calls as f64,
        input_tokens: 0.0,
        output_tokens: 0.0,
        cache_read_tokens: 0.0,
        cache_creation_tokens: 0.0,
        max_context_tokens: 0.0,
        first_turn_cache_creation: p.turns.first().map_or(0.0, |t| t.cache_creation_tokens),
    };
    for t in &p.turns {
        f.input_tokens += t.input_tokens;
        f.output_tokens += t.output_tokens;
        f.cache_read_tokens += t.cache_read_tokens;
        f.cache_creation_tokens += t.cache_creation_tokens;
        f.max_context_tokens = f.max_context_tokens.max(t.context_tokens);
    }
    f
}

/// Facts of one transcript: the session row, call rows in emit order, then
/// one usage row per turn.
pub fn extract_facts(input: &ExtractInput<'_>) -> Vec<Fact> {
    let r = input.r#ref;
    let base = FactBase {
        sid: r.id.clone(),
        agent: r.agent,
        project: input.project.to_owned(),
        kind: r.kind,
        parent: r.parent.clone(),
        run: r.run.clone(),
    };
    let ctx = Ctx {
        base: base.clone(),
        redact: input.redact,
        local: input.local,
    };
    let ctx_by_req: HashMap<&str, f64> = input
        .parsed
        .turns
        .iter()
        .map(|t| (t.request_id.as_str(), t.context_tokens))
        .collect();
    let ctx_of = |req: Option<&str>| req.and_then(|q| ctx_by_req.get(q).copied());
    let mut pending: IndexMap<&str, PendingCall<'_>> = IndexMap::new();
    let mut matched: HashSet<&str> = HashSet::new();
    let mut calls: Vec<CallFact> = Vec::new();
    for e in &input.parsed.events {
        let Some(id) = e.tool_use_id.as_deref().filter(|s| !s.is_empty()) else {
            continue;
        };
        if e.kind == EventKind::ToolCall {
            pending.insert(
                id,
                PendingCall {
                    id,
                    tool: e.tool_name.as_deref().unwrap_or("tool"),
                    input: &e.body,
                    req: e.request_id.as_deref(),
                    ts: e.ts.as_deref(),
                    side: e.is_sidechain == Some(true),
                },
            );
            continue;
        }
        if e.kind != EventKind::ToolResult || matched.contains(id) {
            continue;
        }
        if let Some(call) = pending.shift_remove(id) {
            matched.insert(id);
            calls.push(call_fact(
                &ctx,
                calls.len(),
                &call,
                Some(e),
                ctx_of(call.req),
            ));
        }
    }
    for call in pending.values() {
        calls.push(call_fact(&ctx, calls.len(), call, None, ctx_of(call.req)));
    }
    let session = session_fact(input, base, calls.len());
    let usage = input.parsed.turns.iter().map(|t| {
        Fact::Usage(UsageFact {
            sid: r.id.clone(),
            agent: r.agent,
            ts: t.ts.clone(),
            req: t.request_id.clone(),
            model: t.model.clone(),
            input_tokens: t.input_tokens,
            output_tokens: t.output_tokens,
            cache_read_tokens: t.cache_read_tokens,
            cache_creation_tokens: t.cache_creation_tokens,
        })
    });
    std::iter::once(Fact::Session(session))
        .chain(calls.into_iter().map(Fact::Call))
        .chain(usage)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{arg_of, as_object, command_of};

    fn command(input: &str) -> String {
        command_of(as_object(input).as_ref())
    }

    #[test]
    fn an_argv_command_runs_its_script_or_joins_its_words() {
        let cases = [
            (r#"{"command":["bash","-lc","ls x"]}"#, "ls x"),
            (r#"{"command":["zsh","-c","ls"]}"#, "ls"),
            (r#"{"command":["bash","-l","-c","ls"]}"#, "ls"),
            (r#"{"command":["ls","-la"]}"#, "ls -la"),
            (r#"{"command":["bash","-lc"]}"#, "-lc"),
            (r#"{"command":[1,"bash","-lc","x"]}"#, "x"),
            (r#"{"cmd":"ls"}"#, ""),
        ];
        for (input, want) in cases {
            assert_eq!(command(input), want, "{input}");
        }
    }

    #[test]
    fn arg_falls_back_to_cmd() {
        let input = as_object(r#"{"cmd":"jq . x.json"}"#);
        assert_eq!(arg_of(input.as_ref()), Some("jq . x.json"));
        let argv = as_object(r#"{"command":["ls"],"cmd":""}"#);
        assert_eq!(arg_of(argv.as_ref()), None);
    }
}
