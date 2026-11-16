//! The context window a Claude or Pi session is analyzed against when its
//! transcript does not record one.
#![allow(clippy::float_cmp)]

mod common;

use common::{analyze_default, parse_rows};
use peektrace_core::pi::parse_pi_session;
use serde_json::{Value, json};

const K200: f64 = 200_000.0;
const M1: f64 = 1_000_000.0;

fn assistant(model: &str, input: u64) -> Value {
    json!({
        "type": "assistant", "timestamp": "2026-01-01T00:00:01Z", "requestId": format!("r-{input}"),
        "message": {"model": model, "usage": {"input_tokens": input, "output_tokens": 5},
            "content": [{"type": "text", "text": "ok"}]}
    })
}

fn user() -> Value {
    json!({"type":"user","timestamp":"2026-01-01T00:00:00Z","message":{"content":"hi"}})
}

/// Window and whether it was inferred, for a Claude session of `model`
/// whose calls carry `inputs` tokens each.
fn claude(model: &str, inputs: &[u64]) -> (f64, bool) {
    let mut rows = vec![user()];
    rows.extend(inputs.iter().map(|&n| assistant(model, n)));
    let a = analyze_default(parse_rows(&rows));
    (a.context_window, a.context_window_inferred)
}

#[test]
fn older_claude_models_get_200k() {
    for model in [
        "claude-haiku-4-5-20251001",
        "claude-opus-4-1-20250805",
        "claude-sonnet-4-5-20250929",
        "claude-3-5-sonnet-20241022",
    ] {
        assert_eq!(claude(model, &[11_000]), (K200, true), "{model}");
    }
}

#[test]
fn current_claude_models_get_1m() {
    for model in [
        "claude-opus-4-6",
        "claude-opus-5-5",
        "claude-sonnet-4-6",
        "claude-sonnet-5-5",
        "claude-haiku-5-5",
        "claude-fable-5-2",
    ] {
        assert_eq!(claude(model, &[11_000]), (M1, true), "{model}");
    }
}

#[test]
fn a_call_above_200k_forces_1m() {
    assert_eq!(claude("claude-haiku-4-5", &[11_000, 250_000]), (M1, true));
}

#[test]
fn a_1m_model_usage_key_forces_1m_and_is_not_inferred() {
    let rows = [
        user(),
        assistant("claude-sonnet-4-5", 11_000),
        json!({"type":"cost-state","modelUsage":{"claude-sonnet-4-5[1m]":{"inputTokens":10,"outputTokens":5}}}),
    ];
    let a = analyze_default(parse_rows(&rows));
    assert_eq!((a.context_window, a.context_window_inferred), (M1, false));
}

#[test]
fn a_200k_window_puts_the_dumb_zone_at_80k() {
    let a = analyze_default(parse_rows(&[
        user(),
        assistant("claude-haiku-4-5", 50_000),
        assistant("claude-haiku-4-5", 90_000),
    ]));
    assert_eq!(a.context_window, K200);
    assert_eq!((a.dumb_zone_cross_turn, a.dumb_zone_turns), (1, 1));
}

fn pi(model: &str, input: u64) -> Option<f64> {
    let text = [
        json!({"type":"session","version":3,"id":"s1","cwd":"/tmp","timestamp":"2026-01-01T00:00:00.000Z"}),
        json!({"type":"model_change","id":"m1","parentId":null,"modelId":model}),
        json!({"type":"message","id":"a1","parentId":"m1","message":{"role":"assistant","model":model,
            "content":[{"type":"text","text":"ok"}],"usage":{"input":input,"output":5}}}),
    ]
    .map(|v| v.to_string())
    .join("\n");
    parse_pi_session(&text, "/tmp/pi.jsonl", "s1").native_context_window
}

#[test]
fn pi_shares_the_claude_table() {
    assert_eq!(pi("claude-haiku-4-5", 11_000), Some(K200));
    assert_eq!(pi("claude-opus-4-8", 11_000), Some(M1));
    assert_eq!(pi("claude-haiku-4-5", 250_000), None);
}
