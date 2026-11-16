//! Golden analysis, redaction and session cost. Ported from `analyze` and
//! the Claude cases of `cost` TS tests.
#![allow(clippy::float_cmp)]

mod common;

use common::{TempDir, analyze_default, fixture, jsonl, parse_text, read_fixture};
use peektrace_core::claude::{ParseArgs, parse_claude_session};
use peektrace_core::js::time::Utc;
use peektrace_core::roots::{RootsEnv, claude_sources};
use peektrace_core::sessions::analyze::{AnalyzeOptions, analyze};
use peektrace_core::sessions::cost::turns_cost;
use peektrace_core::sessions::model::{
    AnalyzedSession, EventKind, ParsedSession, SubagentRef, Turn, TurnShape, TurnsCost,
};
use peektrace_core::sessions::redact::{redact_session, redact_text};
use peektrace_core::{Agent, AgentRoots, ListFilter, list_sessions};
use serde_json::{Value, json};

const SESSION_ID: &str = "11111111-1111-4111-8111-111111111111";

fn golden() -> Value {
    serde_json::from_str(&read_fixture("sessions/golden.json")).expect("golden")
}

fn golden_parsed() -> ParsedSession {
    let rel = format!("sessions/projects/-Users-demo-proj/{SESSION_ID}.jsonl");
    let path = fixture(&rel);
    let text = read_fixture(&rel);
    parse_claude_session(&ParseArgs {
        text: &text,
        path: &path,
        session_id: SESSION_ID,
        ..ParseArgs::default()
    })
}

fn num(v: &Value) -> f64 {
    v.as_f64().expect("number")
}

#[test]
fn peak_context_excludes_sidechain_turns() {
    let g = golden();
    let a = analyze_default(golden_parsed());
    assert_eq!(a.peak_context_tokens, num(&g["peakContextTokens"]));
    assert_eq!(a.peak_context_tokens, 1150.0);
    assert_eq!(Some(a.peak_turn_index as u64), g["peakTurnIndex"].as_u64());
    assert_eq!(Some(a.turn_count as u64), g["turnCount"].as_u64());
    assert_eq!(a.turn_count, 3);
}

#[test]
fn system_overhead_and_totals_match_golden() {
    let g = golden();
    let a = analyze_default(golden_parsed());
    assert_eq!(a.system_overhead_tokens, num(&g["systemOverheadTokens"]));
    assert_eq!(a.final_context_tokens, num(&g["finalContextTokens"]));
    assert_eq!(a.total_output_tokens, num(&g["totalOutputTokens"]));
    assert_eq!(a.total_output_tokens, 1400.0);
}

fn golden_budget(g: &Value) -> Vec<(String, f64)> {
    g["budget"]
        .as_array()
        .expect("budget")
        .iter()
        .map(|b| {
            (
                b["key"].as_str().unwrap_or("").to_owned(),
                num(&b["tokens"]),
            )
        })
        .collect()
}

#[test]
fn budget_at_peak_matches_golden_exactly() {
    let a = analyze_default(golden_parsed());
    let budget: Vec<_> = a
        .budget
        .iter()
        .map(|b| (b.key.as_str().to_owned(), b.tokens))
        .collect();
    assert_eq!(budget, golden_budget(&golden()));
}

#[test]
fn thinking_band_is_recovered_from_output_tokens() {
    let a = analyze_default(golden_parsed());
    let thinking = a
        .budget
        .iter()
        .find(|b| b.key.as_str() == "thinking")
        .map(|b| b.tokens);
    let want = golden_budget(&golden())
        .into_iter()
        .find(|(k, _)| k == "thinking")
        .map(|(_, t)| t);
    assert_eq!(thinking, want);
    assert!(thinking.unwrap_or(0.0) > 0.0);
}

#[test]
fn per_turn_snapshots_track_real_context_size() {
    let a = analyze_default(golden_parsed());
    let ctx: Vec<_> = a.snapshots.iter().map(|s| s.ctx).collect();
    let want: Vec<_> = golden()["snapshots"]
        .as_array()
        .expect("snapshots")
        .iter()
        .map(|s| num(&s["ctx"]))
        .collect();
    assert_eq!(ctx, want);
}

#[test]
fn default_window_is_one_million_and_inferred() {
    let a = analyze_default(golden_parsed());
    assert_eq!(a.context_window, 1_000_000.0);
    assert!(a.context_window_inferred);
    let overridden = analyze(
        golden_parsed(),
        AnalyzeOptions {
            dumb_zone_fraction: None,
            on_disk_context_files: Vec::new(),
            window: Some(200_000.0),
            local: &Utc,
        },
    );
    assert_eq!(overridden.context_window, 200_000.0);
    assert!(!overridden.context_window_inferred);
}

#[test]
fn redact_text_masks_a_planted_anthropic_key() {
    let out =
        redact_text(r#"API_KEY = "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234""#);
    assert!(out.contains("[REDACTED:anthropic-key]"));
    assert!(!out.contains("sk-ant-api03-AAAA"));
}

#[test]
fn redact_session_masks_the_secret_in_the_tool_result() {
    let mut a = analyze_default(golden_parsed());
    redact_session(&mut a);
    let r = common::first_of(&a.parsed, EventKind::ToolResult);
    assert!(r.body.contains("[REDACTED:anthropic-key]"));
    assert!(!r.body.contains("sk-ant-api03"));
}

// cost

fn usage(input: u32, output: u32) -> Value {
    json!({"input_tokens":input,"output_tokens":output,"cache_read_input_tokens":0,"cache_creation_input_tokens":0})
}

fn claude_rows(u: &Value, model: &str) -> Vec<Value> {
    vec![
        json!({"type":"user","timestamp":"2026-01-01T00:00:00Z","message":{"content":"hi"}}),
        json!({"type":"assistant","timestamp":"2026-01-01T00:00:01Z","requestId":"r1",
            "message":{"model":model,"usage":u,"content":[{"type":"text","text":"ok"}]}}),
    ]
}

fn claude_session(u: &Value, model: &str) -> ParsedSession {
    parse_text(&jsonl(&claude_rows(u, model)))
}

fn close(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < 1e-6)
}

fn total(a: &AnalyzedSession) -> Option<f64> {
    a.cost.as_ref().and_then(|c| c.total_usd)
}

fn subagent(cost_usd: Option<f64>, unreadable: bool) -> SubagentRef {
    SubagentRef {
        id: "agent-a".to_owned(),
        path: "/synthetic/agent-a.jsonl".to_owned(),
        agent_type: None,
        description: None,
        tool_use_id: None,
        parent_agent_id: None,
        workflow_run_id: None,
        workflow_phase: None,
        workflow_phase_index: None,
        tool_use_id_linked: false,
        turns: usize::from(!unreadable),
        peak_context_tokens: if unreadable { 0.0 } else { 10.0 },
        cost: TurnsCost {
            cost_usd,
            unpriced_models: None,
        },
        unreadable,
    }
}

#[test]
fn analyze_prices_each_turn_and_totals_the_session() {
    let a = analyze_default(claude_session(&usage(1_000_000, 0), "claude-opus-4-8"));
    assert!(close(a.parsed.turns[0].cost_usd, 5.0));
    assert!(close(total(&a), 5.0));
    let cost = a.cost.as_ref().expect("cost");
    assert!(!cost.from_agent);
    assert_eq!(cost.unpriced_models, None);
}

#[test]
fn the_total_includes_subagent_cost() {
    let mut p = claude_session(&usage(1_000_000, 0), "claude-opus-4-8");
    p.subagents = vec![subagent(Some(0.5), false)];
    let a = analyze_default(p);
    assert!(close(total(&a), 5.5));
    assert!(close(a.cost.and_then(|c| c.subagents_usd), 0.5));
}

#[test]
fn an_unreadable_subagent_is_counted() {
    let mut p = claude_session(&usage(1_000_000, 0), "claude-opus-4-8");
    p.subagents = vec![subagent(None, true)];
    let a = analyze_default(p);
    assert!(close(total(&a), 5.0));
    assert_eq!(a.cost.and_then(|c| c.unreadable_subagents), Some(1));
}

#[test]
fn inline_sidechain_turns_are_priced_into_the_subagent_share() {
    let mut rows = claude_rows(&usage(1_000_000, 0), "claude-sonnet-4-5");
    rows.push(
        json!({"type":"assistant","requestId":"r9","isSidechain":true,
        "message":{"model":"claude-sonnet-4-5","usage":usage(1_000_000, 0)}}),
    );
    let p = parse_text(&jsonl(&rows));
    assert_eq!(p.turns.len(), 1);
    let a = analyze_default(p);
    assert!(close(total(&a), 6.0));
    assert!(close(a.cost.and_then(|c| c.subagents_usd), 3.0));
}

fn turn(model: &str, input: f64) -> Turn {
    Turn {
        shape: TurnShape::Claude,
        request_id: "r".to_owned(),
        model: model.to_owned(),
        context_tokens: input,
        input_tokens: input,
        cache_read_tokens: 0.0,
        cache_creation_tokens: 0.0,
        output_tokens: 0.0,
        cache_creation_1h_tokens: None,
        cache_creation_5m_tokens: None,
        event_indexes: Vec::new(),
        ts: None,
        thinking_tokens: None,
        web_search_requests: None,
        web_fetch_requests: None,
        effort: None,
        cost_usd: None,
        appended: Vec::new(),
    }
}

#[test]
fn a_subagents_cost_fields_come_from_its_turns() {
    let c = turns_cost(&[
        turn("claude-opus-4-8", 1_000_000.0),
        turn("example-model-1", 5.0),
    ]);
    assert_eq!(
        c,
        TurnsCost {
            cost_usd: Some(5.0),
            unpriced_models: Some(vec!["example-model-1".to_owned()]),
        }
    );
}

#[test]
fn an_unknown_model_leaves_the_total_absent_and_names_it() {
    let a = analyze_default(claude_session(&usage(1000, 0), "example-model-1"));
    let cost = a.cost.expect("cost");
    assert_eq!(cost.total_usd, None);
    assert_eq!(
        cost.unpriced_models,
        Some(vec!["example-model-1".to_owned()])
    );
}

#[test]
fn a_synthetic_zero_usage_turn_does_not_mark_unpriced() {
    let mut p = claude_session(&usage(1000, 0), "claude-sonnet-4-6");
    let synthetic = Turn {
        request_id: "r2".to_owned(),
        event_indexes: Vec::new(),
        ..turn("<synthetic>", 0.0)
    };
    p.turns.push(synthetic);
    let a = analyze_default(p);
    let cost = a.cost.expect("cost");
    assert_eq!(cost.unpriced_models, None);
    assert!(close(cost.total_usd, 0.003));
}

// header cost (Claude)

fn header_cost(text: &str) -> (Option<f64>, Option<Vec<String>>) {
    let dir = TempDir::new("cost");
    dir.write("demo/s.jsonl", text);
    let env = RootsEnv {
        home: "/nonexistent-home".to_owned(),
        claude_projects: Some(dir.str()),
        ..RootsEnv::default()
    };
    let roots = AgentRoots {
        claude: claude_sources(&env, &[]),
    };
    let filter = ListFilter {
        agent: Some(Agent::Claude),
        project: None,
    };
    let listing = list_sessions(&roots, &filter);
    let h = listing.headers.first().expect("header");
    (h.cost_usd, h.unpriced_models.clone())
}

#[test]
fn header_counts_a_streamed_request_once_and_inline_sidechains() {
    let assistant = |output: u32, text: &str| {
        json!({"type":"assistant","requestId":"r1","message":{"model":"claude-opus-4-8",
            "usage":usage(1_000_000, output),"content":[{"type":"text","text":text}]}})
    };
    let text = jsonl(&[
        assistant(10, "a"),
        assistant(1_000_000, "b"),
        json!({"type":"assistant","requestId":"r9","isSidechain":true,
            "message":{"model":"claude-opus-4-8","usage":usage(1_000_000, 0)}}),
    ]);
    let (cost, unpriced) = header_cost(&text);
    assert!(close(cost, 5.0 + 25.0 + 5.0));
    assert_eq!(unpriced, None);
}

#[test]
fn header_flags_an_unpriced_model() {
    let (cost, unpriced) = header_cost(&jsonl(&claude_rows(&usage(1000, 0), "example-model-1")));
    assert_eq!(cost, None);
    assert_eq!(unpriced, Some(vec!["example-model-1".to_owned()]));
}

#[test]
fn header_cost_matches_analyze_on_the_same_transcript() {
    let mut split = usage(2000, 300);
    split["cache_read_input_tokens"] = json!(50_000);
    split["cache_creation_input_tokens"] = json!(4000);
    split["cache_creation"] =
        json!({"ephemeral_1h_input_tokens":3000,"ephemeral_5m_input_tokens":1000});
    let mut more = split.clone();
    more["output_tokens"] = json!(900);
    let mut rows = claude_rows(&split, "claude-opus-4-8");
    rows.extend([
        json!({"type":"assistant","requestId":"r1","message":{"model":"claude-opus-4-8","usage":more}}),
        json!({"type":"assistant","requestId":"r2","isSidechain":true,
            "message":{"model":"claude-haiku-4-5","usage":usage(7000, 50)}}),
        json!({"type":"assistant","requestId":"r3","message":{"model":"<synthetic>","usage":usage(0, 0)}}),
    ]);
    let text = jsonl(&rows);
    let a = analyze_default(parse_text(&text));
    let (cost, _) = header_cost(&text);
    let want = total(&a).expect("total");
    assert!(
        cost.is_some_and(|c| (c - want).abs() < 1e-10),
        "{cost:?} {want}"
    );
}
