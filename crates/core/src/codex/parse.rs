//! A Codex CLI rollout as a [`ParsedSession`]. Each line is an envelope
//! `{timestamp, type, payload}`. The timeline comes from `response_item`
//! lines plus the system prompt on `session_meta`; `event_msg` lines only
//! drive turns, usage and the context window.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;

use super::context::{Base, agent_message_event, message_events, plain, session_meta_event};
use crate::js::units;
use crate::js::value::{EMPTY_OBJECT, JsValue, defined, str_of};
use crate::sessions::jsonl::parse_rows;
use crate::sessions::measure::measure_tokens;
use crate::sessions::model::{
    EventKind, EventShape, ParsedSession, Provider, TimelineEvent, Turn, TurnTokens,
};

const LINE_CAP: usize = 200;

/// A non-zero shell exit code in tool output, the error heuristic.
/// JS `/i` without the `u` flag folds ASCII letters only.
static ERR_EXIT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i-u:exit(?:ed with)? code:? ?[1-9])")
        .unwrap_or_else(|e| unreachable!("bad pattern: {e}"))
});

/// A number field, 0 when absent or not a number.
fn num(v: Option<&JsValue>) -> f64 {
    v.and_then(JsValue::as_num).unwrap_or(0.0)
}

/// `JSON.stringify(JSON.parse(s), null, 2)`, or `s` when it does not parse.
fn pretty_json(s: String) -> String {
    JsValue::parse(&s).map_or(s, |v| v.stringify(true))
}

fn call_id(payload: &JsValue) -> Option<String> {
    payload
        .get("call_id")
        .and_then(JsValue::as_str)
        .map(str::to_owned)
}

fn tool_call(base: Base<'_>, name: &str, payload: &JsValue, body: String) -> TimelineEvent {
    let name = if name.is_empty() { "tool" } else { name };
    let preview = units::first_line(&units::collapse_ws(&body), LINE_CAP);
    let mut e = base.event(
        EventShape::CodexToolCall,
        EventKind::ToolCall,
        name.to_owned(),
        preview,
        body,
    );
    e.tool_name = Some(name.to_owned());
    e.tool_use_id = call_id(payload);
    e
}

fn tool_result(base: Base<'_>, payload: &JsValue, body: String) -> TimelineEvent {
    let is_error = ERR_EXIT.is_match(&body);
    let title = if is_error {
        "tool_result (error)"
    } else {
        "tool_result"
    };
    let preview = units::first_line(&body, LINE_CAP);
    let mut e = base.event(
        EventShape::CodexToolResult,
        EventKind::ToolResult,
        title.to_owned(),
        preview,
        body,
    );
    e.is_error = Some(is_error);
    e.tool_use_id = call_id(payload);
    e
}

/// `JSON.stringify(v ?? fallback)`.
fn json_or(v: Option<&JsValue>, fallback: &str) -> String {
    defined(v).map_or_else(|| fallback.to_owned(), |v| v.stringify(false))
}

/// The timeline event for one non-message `response_item` payload.
fn response_event(payload: &JsValue, base: Base<'_>) -> Option<TimelineEvent> {
    let field = |k: &str| str_of(payload.get(k));
    Some(match str_of(payload.get("type")).as_str() {
        "agent_message" => return agent_message_event(payload, base),
        "reasoning" => {
            let text = payload
                .get("summary")
                .and_then(JsValue::as_arr)
                .unwrap_or_default()
                .iter()
                .filter_map(|s| {
                    let s = plain(Some(s))?;
                    (s.get("type").and_then(JsValue::as_str) == Some("summary_text"))
                        .then(|| s.get("text").and_then(JsValue::as_str))
                        .flatten()
                })
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            let preview = if text.is_empty() {
                "(reasoning encrypted \u{2014} not stored)".to_owned()
            } else {
                units::first_line(&text, LINE_CAP)
            };
            base.event(
                EventShape::CodexText,
                EventKind::AssistantThinking,
                "Thinking".to_owned(),
                preview,
                text,
            )
        }
        "function_call" => tool_call(
            base,
            &field("name"),
            payload,
            pretty_json(field("arguments")),
        ),
        "custom_tool_call" => tool_call(base, &field("name"), payload, field("input")),
        "tool_search_call" => tool_call(
            base,
            "tool_search",
            payload,
            json_or(payload.get("arguments"), "{}"),
        ),
        "web_search_call" => {
            let query =
                plain(payload.get("action")).map_or_else(String::new, |a| str_of(a.get("query")));
            tool_call(base, "web_search", payload, query)
        }
        "function_call_output" | "custom_tool_call_output" => {
            tool_result(base, payload, field("output"))
        }
        "tool_search_output" => tool_result(base, payload, json_or(payload.get("tools"), "[]")),
        _ => return None,
    })
}

/// Session facts gathered while scanning.
#[derive(Default)]
struct Meta {
    cwd: Option<String>,
    ended_at: Option<String>,
    git_branch: Option<String>,
    session_id: Option<String>,
    started_at: Option<String>,
    version: Option<String>,
}

/// Turn attribution across the rollout.
struct Cursor {
    /// Model output events not yet paired with a `token_count`.
    call_events: Vec<usize>,
    current_model: String,
    current_turn_id: Option<String>,
    /// A fork still replaying its parent's history (no own output yet).
    inherited: bool,
    /// The last cumulative usage seen, as JSON text.
    last_total: Option<String>,
    native_window: Option<f64>,
}

struct Scan {
    events: Vec<TimelineEvent>,
    turns: Vec<Turn>,
    models: Vec<String>,
    counters: HashMap<String, u64>,
    meta: Meta,
    cursor: Cursor,
}

impl Scan {
    fn push(&mut self, e: Option<TimelineEvent>) {
        let Some(e) = e else { return };
        let output = e.kind.is_output();
        self.events.push(e);
        if output {
            self.cursor.inherited = false;
            self.cursor.call_events.push(self.events.len() - 1);
        }
    }

    fn session_meta(&mut self, payload: &JsValue) {
        let (meta, cursor) = (&mut self.meta, &mut self.cursor);
        if meta.session_id.is_none()
            && payload
                .get("forked_from_id")
                .and_then(JsValue::as_str)
                .is_some()
        {
            cursor.inherited = true;
        }
        let id = str_of(payload.get("id"));
        if !id.is_empty() {
            meta.session_id = Some(id);
        }
        if let Some(cwd) = payload.get("cwd").and_then(JsValue::as_str) {
            meta.cwd.get_or_insert_with(|| cwd.to_owned());
        }
        if let Some(b) = plain(payload.get("git"))
            .and_then(|g| g.get("branch"))
            .and_then(JsValue::as_str)
        {
            meta.git_branch = Some(b.to_owned());
        }
        if let Some(v) = payload.get("cli_version").and_then(JsValue::as_str) {
            meta.version = Some(v.to_owned());
        }
    }

    fn turn_context(&mut self, payload: &JsValue) {
        if let Some(model) = payload.get("model").and_then(JsValue::as_str) {
            model.clone_into(&mut self.cursor.current_model);
            if !self.models.iter().any(|m| m == model) {
                self.models.push(model.to_owned());
            }
        }
        if let Some(t) = payload.get("turn_id").and_then(JsValue::as_str) {
            self.cursor.current_turn_id = Some(t.to_owned());
        }
        if let Some(cwd) = payload.get("cwd").and_then(JsValue::as_str) {
            self.meta.cwd.get_or_insert_with(|| cwd.to_owned());
        }
    }

    /// One turn per model call from a `token_count` `info`, tagging the
    /// call's output events with its request id. A re-emitted cumulative
    /// total (same JSON text) is skipped.
    fn token_count(&mut self, info: &JsValue, index: usize, ts: Option<&str>) {
        let cursor = &mut self.cursor;
        if let Some(w) = info.get("model_context_window").and_then(JsValue::as_num) {
            cursor.native_window = Some(w);
        }
        let Some(last) = plain(info.get("last_token_usage")) else {
            return;
        };
        let total = info.get("total_token_usage").map(|t| t.stringify(false));
        if total.is_some() && total == cursor.last_total {
            return;
        }
        if total.is_some() {
            cursor.last_total = total;
        }
        if cursor.inherited {
            return;
        }
        let context = num(last.get("input_tokens"));
        let cache_read = num(last.get("cached_input_tokens"));
        let calls = std::mem::take(&mut cursor.call_events);
        if context <= 0.0 {
            return;
        }
        let turn_id = cursor
            .current_turn_id
            .clone()
            .unwrap_or_else(|| format!("tc-{index}"));
        let n = self.counters.entry(turn_id.clone()).or_insert(0);
        *n += 1;
        let request_id = format!("{turn_id}#{n}");
        for &i in &calls {
            if let Some(e) = self.events.get_mut(i) {
                e.request_id = Some(request_id.clone());
            }
        }
        let tokens = TurnTokens {
            context,
            input: context - cache_read,
            cache_read,
            cache_creation: 0.0,
            output: num(last.get("output_tokens")),
        };
        let mut turn = Turn::new(
            request_id,
            self.cursor.current_model.clone(),
            tokens,
            ts.map(str::to_owned),
        );
        for i in calls {
            turn.link(i);
        }
        self.turns.push(turn);
    }

    fn event_msg(&mut self, payload: &JsValue, index: usize, ts: Option<&str>) {
        match str_of(payload.get("type")).as_str() {
            "task_started" => {
                // An aborted call's output never gets a token_count.
                self.cursor.call_events.clear();
                if let Some(t) = payload.get("turn_id").and_then(JsValue::as_str) {
                    self.cursor.current_turn_id = Some(t.to_owned());
                }
                if let Some(w) = payload
                    .get("model_context_window")
                    .and_then(JsValue::as_num)
                {
                    self.cursor.native_window = Some(w);
                }
            }
            "token_count" => {
                if let Some(info) = plain(payload.get("info")) {
                    self.token_count(info, index, ts);
                }
            }
            _ => {}
        }
    }

    /// Fold one row. Rows that are not objects carry nothing.
    fn line(&mut self, row: &JsValue, index: usize) {
        if row.as_obj().is_none() {
            return;
        }
        let ts = row.get("timestamp").and_then(JsValue::as_str);
        if let Some(ts) = ts.filter(|t| !t.is_empty()) {
            self.meta.started_at.get_or_insert_with(|| ts.to_owned());
            self.meta.ended_at = Some(ts.to_owned());
        }
        let payload = plain(row.get("payload")).unwrap_or(&EMPTY_OBJECT);
        let base = Base { index, ts };
        match str_of(row.get("type")).as_str() {
            "session_meta" => {
                self.session_meta(payload);
                self.push(session_meta_event(payload, base));
            }
            "turn_context" => self.turn_context(payload),
            "response_item" => {
                if payload.get("type").and_then(JsValue::as_str) == Some("message") {
                    for e in message_events(payload, base) {
                        self.push(Some(e));
                    }
                } else {
                    self.push(response_event(payload, base));
                }
            }
            "event_msg" => self.event_msg(payload, index, ts),
            _ => {}
        }
    }
}

/// Parse a Codex rollout. Turn usage is each `token_count`'s
/// `last_token_usage` (one call's delta); body sizes are chars/4.
/// `session_id` is used when no `session_meta` names one.
pub fn parse_codex_session(text: &str, path: &str, session_id: &str) -> ParsedSession {
    let mut scan = Scan {
        events: Vec::new(),
        turns: Vec::new(),
        models: Vec::new(),
        counters: HashMap::new(),
        meta: Meta::default(),
        cursor: Cursor {
            call_events: Vec::new(),
            current_model: "unknown".to_owned(),
            current_turn_id: None,
            inherited: false,
            last_total: None,
            native_window: None,
        },
    };
    for (index, row) in parse_rows(text).iter().enumerate() {
        scan.line(row, index);
    }
    let Scan {
        events,
        turns,
        models,
        meta,
        cursor,
        ..
    } = scan;
    let mut parsed = ParsedSession {
        provider: Provider::Codex,
        session_id: meta.session_id.unwrap_or_else(|| session_id.to_owned()),
        path: path.to_owned(),
        models,
        events,
        turns,
        compaction_indexes: Vec::new(),
        subagents: Vec::new(),
        cwd: meta.cwd,
        git_branch: meta.git_branch,
        title: None,
        version: meta.version,
        started_at: meta.started_at,
        ended_at: meta.ended_at,
        native_context_window: cursor.native_window,
        unknown_record_types: None,
        unknown_system_subtypes: None,
        sidechain_cost: None,
        native_cost_usd: None,
    };
    measure_tokens(&mut parsed);
    parsed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes() {
        for (body, err) in [
            ("Exit code 0", false),
            ("exited with code 1", true),
            ("EXIT CODE:9", true),
            ("process exit code 10", true),
            ("exitcode 1", false),
            ("Process exited with code 0", false),
        ] {
            assert_eq!(ERR_EXIT.is_match(body), err, "{body}");
        }
    }

    #[test]
    fn pretty_args() {
        assert_eq!(pretty_json(String::new()), "");
        assert_eq!(pretty_json("5".to_owned()), "5");
        assert_eq!(pretty_json("{oops".to_owned()), "{oops");
        assert_eq!(
            pretty_json(r#"{"b":1,"10":2}"#.to_owned()),
            "{\n  \"10\": 2,\n  \"b\": 1\n}"
        );
    }
}
