//! The token-usage fold the session parse and the list header share: one
//! model call per `token_count`, skipping a re-emitted cumulative total and
//! the calls a fork replays from its parent.

use super::context::{Base, message_events};
use super::parse::response_event;
use crate::js::value::{JsValue, str_of};
use crate::sessions::model::{TimelineEvent, TurnTokens};

/// Which `token_count` lines count as calls.
#[derive(Debug, Default)]
pub struct UsageFold {
    /// A fork still replaying its parent's history (no own output yet).
    pub inherited: bool,
    /// A `session_meta` named this session already.
    named: bool,
    /// The last cumulative usage seen, as JSON text.
    last_total: Option<String>,
}

/// A number field, 0 when absent or not a number.
fn num(v: Option<&JsValue>) -> f64 {
    v.and_then(JsValue::as_num).unwrap_or(0.0)
}

impl UsageFold {
    /// A `session_meta` payload: the first one of a fork starts the replay.
    pub fn session_meta(&mut self, payload: &JsValue) {
        if !self.named
            && payload
                .get("forked_from_id")
                .and_then(JsValue::as_str)
                .is_some()
        {
            self.inherited = true;
        }
        if !str_of(payload.get("id")).is_empty() {
            self.named = true;
        }
    }

    /// The fork made its own output: its calls count from here.
    pub fn output(&mut self) {
        self.inherited = false;
    }

    /// The `last_token_usage` of a `token_count` `info` that counts as a
    /// call; `None` for a re-emitted total or a replayed call.
    pub fn call<'a>(&mut self, info: &'a JsValue) -> Option<&'a JsValue> {
        let last = super::context::plain(info.get("last_token_usage"))?;
        let total = info.get("total_token_usage").map(|t| t.stringify(false));
        if total.is_some() && total == self.last_total {
            return None;
        }
        if total.is_some() {
            self.last_total = total;
        }
        (!self.inherited).then_some(last)
    }
}

/// A call's tokens, or `None` when its prompt holds no context.
pub fn call_tokens(last: &JsValue) -> Option<TurnTokens> {
    let context = num(last.get("input_tokens"));
    let cache_read = num(last.get("cached_input_tokens"));
    (context > 0.0).then(|| TurnTokens {
        context,
        input: context - cache_read,
        cache_read,
        cache_creation: 0.0,
        output: num(last.get("output_tokens")),
    })
}

/// The timeline events of one `response_item` payload.
pub fn response_item_events(payload: &JsValue, base: Base<'_>) -> Vec<TimelineEvent> {
    if payload.get("type").and_then(JsValue::as_str) == Some("message") {
        message_events(payload, base)
    } else {
        response_event(payload, base).into_iter().collect()
    }
}
