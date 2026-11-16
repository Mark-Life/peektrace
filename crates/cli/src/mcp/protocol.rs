//! Minimal MCP server protocol: newline-delimited JSON-RPC 2.0. Covers
//! `initialize`, `ping`, `tools/list` and `tools/call`; notifications get no
//! reply. Pure: a line maps to a reply, or to a tool call for the caller to
//! run.

use std::sync::OnceLock;

use peektrace_core::js::value::{JsObject, JsValue};

use super::tools::{self, Answer, Tool};

/// Newest first; `initialize` echoes the client's version when listed.
pub const PROTOCOL_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

const PARSE_ERROR: i32 = -32_700;
const INVALID_REQUEST: i32 = -32_600;
const METHOD_NOT_FOUND: i32 = -32_601;
const INVALID_PARAMS: i32 = -32_602;

/// What to do with one input line.
pub enum Action {
    /// A blank line or a notification.
    Nothing,
    Reply(String),
    /// Run `tool` with `args`, then reply to `id` with [`call_reply`].
    Call {
        id: JsValue,
        tool: &'static Tool,
        args: JsValue,
    },
}

fn envelope(id: &JsValue, key: &str, body: JsValue) -> String {
    JsValue::Obj(JsObject::from_entries([
        ("jsonrpc".to_owned(), JsValue::Str("2.0".to_owned())),
        ("id".to_owned(), id.clone()),
        (key.to_owned(), body),
    ]))
    .stringify(false)
}

pub fn reply(id: &JsValue, result: JsValue) -> String {
    envelope(id, "result", result)
}

pub fn fail(id: &JsValue, code: i32, message: &str) -> String {
    envelope(
        id,
        "error",
        JsValue::Obj(JsObject::from_entries([
            ("code".to_owned(), JsValue::Num(f64::from(code))),
            ("message".to_owned(), JsValue::Str(message.to_owned())),
        ])),
    )
}

fn obj(entries: Vec<(&str, JsValue)>) -> JsValue {
    JsValue::Obj(JsObject::from_entries(
        entries.into_iter().map(|(k, v)| (k.to_owned(), v)),
    ))
}

fn text_block(t: &str) -> JsValue {
    obj(vec![
        ("type", JsValue::Str("text".to_owned())),
        ("text", JsValue::Str(t.to_owned())),
    ])
}

/// The reply to a tool call: its text, plus the notes as a second block;
/// a failure is an `isError` result, never a protocol error.
pub fn call_reply(id: &JsValue, outcome: Result<Answer, String>) -> String {
    let result = match outcome {
        Ok(a) => {
            let notes = (!a.notes.is_empty()).then(|| text_block(&a.notes.join("\n")));
            let content = std::iter::once(text_block(&a.text)).chain(notes).collect();
            obj(vec![("content", JsValue::Arr(content))])
        }
        Err(message) => obj(vec![
            ("content", JsValue::Arr(vec![text_block(&message)])),
            ("isError", JsValue::Bool(true)),
        ]),
    };
    reply(id, result)
}

/// A string or number id.
fn usable_id(v: Option<&JsValue>) -> Option<&JsValue> {
    v.filter(|v| matches!(v, JsValue::Str(_) | JsValue::Num(_)))
}

fn negotiate(params: Option<&JsValue>) -> &'static str {
    let asked = match params {
        Some(p @ JsValue::Obj(_)) => match p.get("protocolVersion") {
            None => None,
            Some(JsValue::Str(s)) => Some(s.as_str()),
            Some(_) => return PROTOCOL_VERSIONS[0],
        },
        _ => None,
    };
    asked
        .and_then(|a| PROTOCOL_VERSIONS.iter().find(|v| **v == a).copied())
        .unwrap_or(PROTOCOL_VERSIONS[0])
}

/// The `tools/list` result, built from the same fields the calls check.
fn tool_list() -> &'static JsValue {
    static LIST: OnceLock<JsValue> = OnceLock::new();
    LIST.get_or_init(|| {
        obj(vec![(
            "tools",
            JsValue::Arr(tools::TOOLS.iter().map(Tool::listing).collect()),
        )])
    })
}

fn dispatch(id: &JsValue, method: &str, params: Option<&JsValue>) -> Action {
    let r = match method {
        "initialize" => reply(
            id,
            obj(vec![
                (
                    "protocolVersion",
                    JsValue::Str(negotiate(params).to_owned()),
                ),
                (
                    "capabilities",
                    obj(vec![(
                        "tools",
                        obj(vec![("listChanged", JsValue::Bool(false))]),
                    )]),
                ),
                (
                    "serverInfo",
                    obj(vec![
                        ("name", JsValue::Str("peektrace".to_owned())),
                        ("version", JsValue::Str(crate::VERSION.to_owned())),
                    ]),
                ),
            ]),
        ),
        "ping" => reply(id, obj(Vec::new())),
        "tools/list" => reply(id, tool_list().clone()),
        "tools/call" => {
            let name = params
                .filter(|p| matches!(p, JsValue::Obj(_)))
                .and_then(|p| p.get("name"));
            let Some(JsValue::Str(name)) = name else {
                return Action::Reply(fail(id, INVALID_PARAMS, "tools/call needs a tool name"));
            };
            let Some(tool) = tools::find(name) else {
                return Action::Reply(fail(id, INVALID_PARAMS, &format!("Unknown tool: {name}")));
            };
            let args = params
                .and_then(|p| p.get("arguments"))
                .cloned()
                .unwrap_or(JsValue::Null);
            return Action::Call {
                id: id.clone(),
                tool,
                args,
            };
        }
        other => fail(id, METHOD_NOT_FOUND, &format!("Method not found: {other}")),
    };
    Action::Reply(r)
}

/// One input line to what the server does with it.
pub fn handle_line(line: &str) -> Action {
    if peektrace_core::js::text::trim(line).is_empty() {
        return Action::Nothing;
    }
    let Some(raw) = JsValue::parse(line) else {
        return Action::Reply(fail(&JsValue::Null, PARSE_ERROR, "Parse error"));
    };
    let invalid = || {
        let id = usable_id(raw.get("id")).cloned().unwrap_or(JsValue::Null);
        Action::Reply(fail(&id, INVALID_REQUEST, "Invalid request"))
    };
    if !matches!(raw, JsValue::Obj(_))
        || raw.get("jsonrpc").and_then(JsValue::as_str) != Some("2.0")
    {
        return invalid();
    }
    let Some(method) = raw.get("method").and_then(JsValue::as_str) else {
        return invalid();
    };
    let id = match raw.get("id") {
        None => return Action::Nothing,
        Some(v) => match usable_id(Some(v)) {
            Some(id) => id,
            None => return invalid(),
        },
    };
    dispatch(id, method, raw.get("params"))
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
