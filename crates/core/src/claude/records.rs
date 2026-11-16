//! Claude record types and the usage fold shared by turns.

use crate::js::value::{JsValue, defined};
use crate::sessions::model::{Turn, TurnField, TurnShape};

/// Model id Claude Code writes on harness-made messages.
pub const SYNTHETIC_MODEL: &str = "<synthetic>";

/// Record types skipped on purpose, so they are not counted as unknown.
pub const KNOWN_IGNORED_TYPES: [&str; 23] = [
    "api-request",
    "last-prompt",
    "queue-operation",
    "file-history-snapshot",
    "mode",
    "permission-mode",
    "session-owner",
    "custom-title",
    "tools-used",
    "agent-name",
    "api-request-blob",
    "api-request-shape",
    "atis-latch",
    "file-history-delta",
    "pr-link",
    "agent-color",
    "agent-setting",
    "tag",
    "worktree-state",
    "content-replacement",
    "memory-mode",
    "progress",
    "hook_progress",
];

/// `system` subtypes the parser knows; others are counted as unknown.
pub const KNOWN_SYSTEM_SUBTYPES: [&str; 8] = [
    "compact_boundary",
    "stop_hook_summary",
    "turn_duration",
    "informational",
    "away_summary",
    "local_command",
    "api_error",
    "hook_progress",
];

fn num(v: Option<&JsValue>) -> Option<f64> {
    v.and_then(JsValue::as_num)
}

/// A number above 0.
fn positive(v: Option<&JsValue>) -> Option<f64> {
    num(v).filter(|n| *n > 0.0)
}

/// The thinking share of a call's output tokens, when usage reports it.
fn thinking_tokens_of(usage: Option<&JsValue>) -> Option<f64> {
    num(usage
        .and_then(|u| u.get("output_tokens_details"))
        .and_then(|d| d.get("thinking_tokens")))
}

/// Server tool counts a call reports, only the ones it used.
fn server_tool_counts(usage: Option<&JsValue>) -> (Option<f64>, Option<f64>) {
    let counts = usage.and_then(|u| defined(u.get("server_tool_use")));
    (
        positive(counts.and_then(|c| c.get("web_search_requests"))),
        positive(counts.and_then(|c| c.get("web_fetch_requests"))),
    )
}

/// Max of two optional counts; `None` only when both are.
fn max_of(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (None, b) => b,
        (a, None) => a,
        (Some(a), Some(b)) => Some(a.max(b)),
    }
}

/// `Math.max(a, b)`: NaN wins.
fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

/// Set one late field, recording it as appended when it was unset.
fn set_late(t: &mut Turn, field: TurnField, value: Option<f64>) {
    let slot = match field {
        TurnField::ThinkingTokens => &mut t.thinking_tokens,
        TurnField::WebSearchRequests => &mut t.web_search_requests,
        TurnField::WebFetchRequests => &mut t.web_fetch_requests,
        TurnField::CostUsd => &mut t.cost_usd,
    };
    if slot.is_none() && value.is_some() && !t.appended.contains(&field) {
        t.appended.push(field);
    }
    *slot = value;
}

/// Fold a later streamed line of the same request: output-side counts grow
/// as the response streams, so keep the max; the rest stays as first seen.
pub fn merge_streamed_usage(t: &mut Turn, usage: Option<&JsValue>) {
    let output = num(usage.and_then(|u| u.get("output_tokens"))).unwrap_or(0.0);
    t.output_tokens = js_max(t.output_tokens, output);
    let (search, fetch) = server_tool_counts(usage);
    let thinking = max_of(t.thinking_tokens, thinking_tokens_of(usage));
    set_late(t, TurnField::ThinkingTokens, thinking);
    let search = max_of(t.web_search_requests, search);
    set_late(t, TurnField::WebSearchRequests, search);
    let fetch = max_of(t.web_fetch_requests, fetch);
    set_late(t, TurnField::WebFetchRequests, fetch);
}

/// Where a new turn comes from.
pub struct NewTurn<'a> {
    pub request_id: &'a str,
    pub model: &'a str,
    pub ts: Option<&'a str>,
    pub usage: Option<&'a JsValue>,
    pub effort: Option<&'a str>,
}

/// A turn from its first line, or `None` when its prompt holds no context
/// or its model is not real.
pub fn new_turn(args: &NewTurn<'_>) -> Option<Turn> {
    let usage = args.usage;
    let count = |key: &str| num(usage.and_then(|u| u.get(key))).unwrap_or(0.0);
    let input = count("input_tokens");
    let cache_read = count("cache_read_input_tokens");
    let cache_creation = count("cache_creation_input_tokens");
    let context = input + cache_read + cache_creation;
    if !(context > 0.0 && args.model != SYNTHETIC_MODEL) {
        return None;
    }
    let split = usage.and_then(|u| defined(u.get("cache_creation")));
    let (search, fetch) = server_tool_counts(usage);
    Some(Turn {
        shape: TurnShape::Claude,
        request_id: args.request_id.to_owned(),
        model: args.model.to_owned(),
        context_tokens: context,
        input_tokens: input,
        cache_read_tokens: cache_read,
        cache_creation_tokens: cache_creation,
        output_tokens: count("output_tokens"),
        cache_creation_1h_tokens: num(split.and_then(|s| s.get("ephemeral_1h_input_tokens"))),
        cache_creation_5m_tokens: num(split.and_then(|s| s.get("ephemeral_5m_input_tokens"))),
        event_indexes: Vec::new(),
        ts: args.ts.map(str::to_owned),
        thinking_tokens: thinking_tokens_of(usage),
        web_search_requests: search,
        web_fetch_requests: fetch,
        effort: args.effort.map(str::to_owned),
        cost_usd: None,
        appended: Vec::new(),
    })
}

/// Pre/post token counts of a `compact_boundary` line.
pub fn compact_meta_of(o: &JsValue) -> (Option<f64>, Option<f64>) {
    let m = defined(o.get("compactMetadata"));
    (
        num(m.and_then(|m| m.get("preTokens"))),
        num(m.and_then(|m| m.get("postTokens"))),
    )
}

/// True when a `cost-state` line bills a model under its 1M-context id.
pub fn has_one_million_key(o: &JsValue) -> bool {
    o.get("modelUsage")
        .and_then(JsValue::as_obj)
        .is_some_and(|u| u.keys().any(|k| k.ends_with("[1m]")))
}

/// Count `key` once more, keeping first-seen order.
pub fn bump(counts: &mut Vec<(String, f64)>, key: &str) {
    match counts.iter_mut().find(|(k, _)| k == key) {
        Some(slot) => slot.1 += 1.0,
        None => counts.push((key.to_owned(), 1.0)),
    }
}
