//! A session's cost comes from the agent's own record first; our estimate is
//! the fallback and the breakdown. Synthetic transcripts only.

mod common;

use common::{analyze_default, jsonl, parse_text};
use peektrace_core::js::value::JsValue;
use peektrace_core::pi::parse_pi_session;
use peektrace_core::sessions::cost::cost_summary;
use peektrace_core::sessions::model::{AnalyzedSession, SessionCost, SubagentRef, TurnsCost};
use peektrace_core::sessions::read::{cost_from_json, parsed_from_json};
use peektrace_core::sessions::write::{analyzed_json, parsed_json};
use serde_json::{Value, json};

/// 2026-01-01T00:00:00Z in epoch ms.
const T0_MS: i64 = 1_767_225_600_000;

fn close(a: Option<f64>, b: f64) -> bool {
    a.is_some_and(|a| (a - b).abs() < 1e-6)
}

/// One opus-5-5 call worth $4 (1M input tokens) and the `cost-state` lines.
fn claude(model: &str, cost_states: &[Value]) -> Vec<Value> {
    let mut rows = vec![
        json!({"type":"user","timestamp":"2026-01-01T00:00:00Z","message":{"content":"hi"}}),
        json!({"type":"assistant","timestamp":"2026-01-01T00:00:01Z","requestId":"r1",
            "message":{"model":model,"usage":{"input_tokens":1_000_000,"output_tokens":0,
            "cache_read_input_tokens":0,"cache_creation_input_tokens":0},
            "content":[{"type":"text","text":"ok"}]}}),
    ];
    rows.extend_from_slice(cost_states);
    rows
}

fn cost_state(start_ms: i64, total: f64) -> Value {
    json!({"type":"cost-state","sessionId":"s","totalCostUSD":total,"startTime":start_ms,
        "modelUsage":{"claude-opus-5-5":{"inputTokens":1,"costUSD":total}},
        "hasUnknownModelCost":false})
}

fn subagent(usd: f64) -> SubagentRef {
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
        turns: 1,
        peak_context_tokens: 10.0,
        cost: TurnsCost {
            cost_usd: Some(usd),
            unpriced_models: None,
        },
        unreadable: false,
    }
}

fn analyzed(rows: &[Value], subagent_usd: Option<f64>) -> AnalyzedSession {
    let mut p = parse_text(&jsonl(rows));
    p.subagents = subagent_usd.into_iter().map(subagent).collect();
    analyze_default(p)
}

fn cost(a: &AnalyzedSession) -> &SessionCost {
    a.cost.as_ref().expect("cost")
}

#[test]
fn claude_record_is_the_total_and_subagents_are_not_added_again() {
    // Two processes: the record is the last line of each, summed.
    let rows = claude(
        "claude-opus-5-5",
        &[
            cost_state(T0_MS - 1000, 2.0),
            cost_state(T0_MS - 1000, 4.0),
            cost_state(T0_MS + 3_600_000, 0.2),
        ],
    );
    let a = analyzed(&rows, Some(0.3));
    let c = cost(&a);
    assert!(c.from_agent);
    assert!(close(c.total_usd, 4.2), "{c:?}");
    assert!(close(c.recorded_usd, 4.2));
    assert!(close(c.subagents_usd, 0.3), "subagents stay as a breakdown");
    assert!(
        close(c.estimate_usd, 4.3),
        "estimate: $4 main + $0.30 subagent"
    );
    assert_eq!(c.estimate_gap, None, "2.4% apart is under the 5% limit");
    assert!(!c.estimate_warning);
}

/// Warning and quiet note for one $4 opus-5-5 call against a record.
fn lines_for(recorded: f64) -> (Option<String>, Option<String>) {
    let a = analyzed(
        &claude("claude-opus-5-5", &[cost_state(T0_MS - 1000, recorded)]),
        None,
    );
    let s = cost_summary(cost(&a), "Claude Code");
    (s.warning, s.estimate_note)
}

#[test]
fn an_estimate_above_the_record_warns() {
    assert_eq!(
        lines_for(3.5),
        (
            Some("estimate from tokens ~$4.00 (14% higher)".to_owned()),
            None
        )
    );
}

#[test]
fn an_estimate_far_below_the_record_warns() {
    assert_eq!(
        lines_for(7.0),
        (
            Some("estimate from tokens ~$4.00 (43% lower)".to_owned()),
            None
        )
    );
}

#[test]
fn an_estimate_somewhat_below_the_record_shows_quietly() {
    let a = analyzed(
        &claude("claude-opus-5-5", &[cost_state(T0_MS - 1000, 5.0)]),
        None,
    );
    let c = cost(&a);
    assert!(
        close(c.estimate_gap, -0.2),
        "the gap is still recorded: {c:?}"
    );
    assert!(!c.estimate_warning);
    assert_eq!(
        lines_for(5.0),
        (
            None,
            Some(
                "Estimate from transcript: ~$4.00. Transcripts miss some billed calls.".to_owned()
            )
        )
    );
}

#[test]
fn a_missing_price_card_shows_as_an_estimate_gap() {
    let rows = claude("claude-example-9-9", &[cost_state(T0_MS - 1000, 10.0)]);
    let a = analyzed(&rows, Some(1.0));
    let c = cost(&a);
    assert!(close(c.total_usd, 10.0));
    assert!(close(c.estimate_usd, 1.0));
    assert!(close(c.estimate_gap, -0.9), "{c:?}");
    assert_eq!(
        c.unpriced_models.as_deref(),
        Some(&["claude-example-9-9".to_owned()][..])
    );
    assert!(c.estimate_warning);
}

#[test]
fn an_unpriced_model_warns_even_with_a_small_gap() {
    // $4 priced call plus an unpriced one; the record is within 5%.
    let mut rows = claude("claude-opus-5-5", &[cost_state(T0_MS - 1000, 4.1)]);
    rows.insert(
        2,
        json!({"type":"assistant","timestamp":"2026-01-01T00:00:02Z","requestId":"r2",
            "message":{"model":"claude-example-9-9","usage":{"input_tokens":10,"output_tokens":0,
            "cache_read_input_tokens":0,"cache_creation_input_tokens":0},
            "content":[{"type":"text","text":"ok"}]}}),
    );
    let a = analyzed(&rows, None);
    let c = cost(&a);
    assert_eq!(c.estimate_gap, None, "{c:?}");
    assert!(c.estimate_warning);
    assert_eq!(
        cost_summary(c, "Claude Code").warning.as_deref(),
        Some("estimate from tokens ~$4.00 (2% lower)")
    );
}

#[test]
fn a_record_from_a_resumed_process_only_is_not_the_total() {
    let rows = claude("claude-opus-5-5", &[cost_state(T0_MS + 3_600_000, 1.5)]);
    let a = analyzed(&rows, Some(0.5));
    let c = cost(&a);
    assert!(!c.from_agent);
    assert!(c.record_partial);
    assert!(
        close(c.total_usd, 4.5),
        "the estimate stays the total: {c:?}"
    );
    assert!(close(c.recorded_usd, 1.5));
    assert_eq!(c.estimate_gap, None);
}

#[test]
fn without_a_record_the_estimate_is_the_total() {
    let a = analyzed(&claude("claude-fable-5-2", &[]), Some(0.5));
    let c = cost(&a);
    assert!(!c.from_agent);
    assert!(close(c.total_usd, 8.5), "fable-5-2 input is $8/M: {c:?}");
    assert_eq!((c.recorded_usd, c.estimate_usd), (None, None));
}

#[test]
fn the_record_survives_the_json_round_trip() {
    let rows = claude("claude-example-9-9", &[cost_state(T0_MS + 3_600_000, 1.5)]);
    let p = parse_text(&jsonl(&rows));
    let text = parsed_json(&p, false);
    assert!(
        text.contains(
            r#""nativeCostUsd":1.5,"nativeCostIncludesSubagents":true,"nativeCostPartial":true"#
        ),
        "{text}"
    );
    let back = JsValue::parse(&text).as_ref().and_then(parsed_from_json);
    assert_eq!(back.map(|b| b.native_cost), Some(p.native_cost));

    let a = analyzed(
        &claude("claude-example-9-9", &[cost_state(T0_MS, 10.0)]),
        None,
    );
    let json = JsValue::parse(&analyzed_json(&a)).expect("json");
    let read = json.get("cost").and_then(cost_from_json);
    assert_eq!(read.as_ref(), a.cost.as_ref());
}

fn pi_call(id: &str, recorded: f64) -> String {
    json!({"type": "message", "id": id, "timestamp": "2026-01-01T00:00:01Z", "message": {
        "role": "assistant", "model": "claude-opus-5-5",
        "usage": {"input": 1_000_000, "output": 0, "cacheRead": 0, "cacheWrite": 0,
            "cost": {"total": recorded, "totalFromProvider": false}},
        "content": [{"type": "text", "text": "ok"}]}})
    .to_string()
}

#[test]
fn pi_record_counts_only_when_every_call_has_one_above_zero() {
    let pi = |calls: &[(&str, f64)]| {
        let text = calls
            .iter()
            .map(|(id, usd)| pi_call(id, *usd))
            .collect::<Vec<_>>()
            .join("\n");
        analyze_default(parse_pi_session(&text, "/synthetic/p.jsonl", "p"))
    };
    let a = pi(&[("m1", 3.9), ("m2", 3.9)]);
    let c = cost(&a);
    assert!(c.from_agent);
    assert!(close(c.total_usd, 7.8));
    assert!(close(c.estimate_usd, 8.0));

    // $0 means Pi had no price, not a free call.
    let a = pi(&[("m1", 0.0)]);
    let c = cost(&a);
    assert!(!c.from_agent && c.recorded_usd.is_none());
    assert!(close(c.total_usd, 4.0));

    let a = pi(&[("m1", 3.9), ("m2", 0.0)]);
    let c = cost(&a);
    assert!(!c.from_agent && c.record_partial);
    assert!(close(c.total_usd, 8.0));
    assert!(close(c.recorded_usd, 3.9));
}

#[test]
fn a_zero_record_is_no_record() {
    let rows = claude("claude-opus-5-5", &[cost_state(T0_MS - 1000, 0.0)]);
    let a = analyzed(&rows, None);
    let c = cost(&a);
    assert!(!c.from_agent && c.recorded_usd.is_none());
    assert!(close(c.total_usd, 4.0));
}

#[test]
fn a_record_from_another_session_is_ignored() {
    let mut other = cost_state(T0_MS - 1000, 9.0);
    other["sessionId"] = json!("another-session");
    let a = analyzed(&claude("claude-opus-5-5", &[other]), None);
    assert!(cost(&a).recorded_usd.is_none());
}
