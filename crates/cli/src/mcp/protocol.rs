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

/// The `tools/list` result, kept verbatim from the TS server.
fn tool_list() -> &'static JsValue {
    static LIST: OnceLock<JsValue> = OnceLock::new();
    LIST.get_or_init(|| JsValue::parse(include_str!("tools.json")).unwrap_or(JsValue::Null))
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
mod tests {
    use super::*;

    fn reply_of(line: &str) -> Option<String> {
        match handle_line(line) {
            Action::Nothing => None,
            Action::Reply(r) => Some(r),
            Action::Call { id, .. } => Some(format!("call {}", id.stringify(false))),
        }
    }

    fn result(line: &str) -> String {
        reply_of(line).unwrap_or_default()
    }

    /// TS `mcp-protocol.test.ts`.
    #[test]
    fn initialize_echoes_a_known_version_and_falls_back() {
        let r = result(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        );
        assert_eq!(
            r,
            format!(
                r#"{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-06-18","capabilities":{{"tools":{{"listChanged":false}}}},"serverInfo":{{"name":"peektrace","version":"{}"}}}}}}"#,
                crate::VERSION
            )
        );
        for params in [
            r#"{"protocolVersion":"1999-01-01"}"#,
            "null",
            r#"{"protocolVersion":5}"#,
            "[]",
        ] {
            let r = result(&format!(
                r#"{{"jsonrpc":"2.0","id":"s","method":"initialize","params":{params}}}"#
            ));
            assert!(r.contains(r#""protocolVersion":"2025-11-25""#), "{r}");
        }
        let r = result(r#"{"jsonrpc":"2.0","id":2,"method":"initialize"}"#);
        assert!(r.contains(r#""protocolVersion":"2025-11-25""#), "{r}");
    }

    #[test]
    fn notifications_and_blank_lines_get_no_reply() {
        assert_eq!(
            reply_of(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#),
            None
        );
        assert_eq!(reply_of(r#"{"jsonrpc":"2.0","method":"nope"}"#), None);
        assert_eq!(reply_of("   "), None);
        assert_eq!(reply_of(""), None);
    }

    #[test]
    fn ping_and_unknown_methods() {
        assert_eq!(
            result(r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#),
            r#"{"jsonrpc":"2.0","id":3,"result":{}}"#
        );
        assert_eq!(
            result(r#"{"jsonrpc":"2.0","id":1.5,"method":"ping","extra":1}"#),
            r#"{"jsonrpc":"2.0","id":1.5,"result":{}}"#
        );
        assert_eq!(
            result(r#"{"jsonrpc":"2.0","id":5,"method":"nope"}"#),
            r#"{"jsonrpc":"2.0","id":5,"error":{"code":-32601,"message":"Method not found: nope"}}"#
        );
    }

    #[test]
    fn parse_and_envelope_errors() {
        assert_eq!(
            result("not json"),
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"Parse error"}}"#
        );
        let invalid = |id: &str| {
            format!(
                r#"{{"jsonrpc":"2.0","id":{id},"error":{{"code":-32600,"message":"Invalid request"}}}}"#
            )
        };
        assert_eq!(
            result(r#"{"jsonrpc":"1.0","id":6,"method":"ping"}"#),
            invalid("6")
        );
        assert_eq!(
            result(r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#),
            invalid("null")
        );
        assert_eq!(
            result(r#"{"jsonrpc":"2.0","id":true,"method":"ping"}"#),
            invalid("null")
        );
        assert_eq!(
            result(r#"{"jsonrpc":"2.0","id":"b1","method":5}"#),
            invalid("\"b1\"")
        );
        assert_eq!(
            result(r#"[{"jsonrpc":"2.0","id":"batch","method":"ping"}]"#),
            invalid("null")
        );
        assert_eq!(
            result(r#"{"jsonrpc":"2.0","id":"noMethod"}"#),
            invalid("\"noMethod\"")
        );
        assert_eq!(
            result(r#"{"id":"noVersion","method":"ping"}"#),
            invalid("\"noVersion\"")
        );
    }

    #[test]
    fn tool_call_params() {
        let needs = r#""error":{"code":-32602,"message":"tools/call needs a tool name"}"#;
        assert!(
            result(r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{}}"#).contains(needs)
        );
        assert!(
            result(r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":[]}"#).contains(needs)
        );
        assert!(
            result(r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":5}}"#)
                .contains(needs)
        );
        assert!(result(r#"{"jsonrpc":"2.0","id":8,"method":"tools/call"}"#).contains(needs));
        assert_eq!(
            result(r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"bogus"}}"#),
            r#"{"jsonrpc":"2.0","id":7,"error":{"code":-32602,"message":"Unknown tool: bogus"}}"#
        );
        assert_eq!(
            result(
                r#"{"jsonrpc":"2.0","id":"c","method":"tools/call","params":{"name":"sessions_list"}}"#
            ),
            "call \"c\""
        );
    }

    #[test]
    fn lists_five_tools_with_object_schemas() {
        let v = tool_list();
        let tools = v.get("tools").and_then(JsValue::as_arr).unwrap_or_default();
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|t| t.get("name").and_then(JsValue::as_str))
            .collect();
        assert_eq!(names, tools::TOOLS.map(|t| t.name));
        assert!(tools.iter().all(|t| {
            t.get("inputSchema")
                .and_then(|s| s.get("type"))
                .and_then(JsValue::as_str)
                == Some("object")
        }));
    }

    #[test]
    fn a_failure_becomes_an_is_error_result() {
        assert_eq!(
            call_reply(&JsValue::Num(9.0), Err("boom".to_owned())),
            r#"{"jsonrpc":"2.0","id":9,"result":{"content":[{"type":"text","text":"boom"}],"isError":true}}"#
        );
        let ok = Answer {
            text: "t".to_owned(),
            notes: vec!["a".to_owned(), "b".to_owned()],
        };
        assert_eq!(
            call_reply(&JsValue::Str("x".to_owned()), Ok(ok)),
            r#"{"jsonrpc":"2.0","id":"x","result":{"content":[{"type":"text","text":"t"},{"type":"text","text":"a\nb"}]}}"#
        );
    }
    fn field<'a>(v: &'a JsValue, path: &[&str]) -> Option<&'a JsValue> {
        path.iter().try_fold(v, |v, k| v.get(k))
    }

    fn parsed(line: &str) -> JsValue {
        JsValue::parse(&result(line)).unwrap_or(JsValue::Null)
    }

    /// TS `mcp-protocol.test.ts` "initialize" and "envelope", with its own
    /// requests.
    #[test]
    fn ts_initialize_and_envelope_cases() {
        let r = parsed(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
        );
        assert_eq!(r.get("id"), Some(&JsValue::Num(1.0)));
        assert_eq!(
            field(&r, &["result", "protocolVersion"]).and_then(JsValue::as_str),
            Some("2025-03-26")
        );
        assert_eq!(
            field(&r, &["result", "serverInfo", "name"]).and_then(JsValue::as_str),
            Some("peektrace")
        );
        assert_eq!(
            field(&r, &["result", "capabilities"]).map(|c| c.stringify(false)),
            Some(r#"{"tools":{"listChanged":false}}"#.to_owned())
        );
        let r = parsed(
            r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
        );
        assert_eq!(
            field(&r, &["result", "protocolVersion"]).and_then(JsValue::as_str),
            Some(PROTOCOL_VERSIONS[0])
        );

        assert_eq!(
            result(r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#),
            r#"{"jsonrpc":"2.0","id":3,"result":{}}"#
        );
        let r = parsed(r#"{"jsonrpc":"2.0","id":4,"method":"resources/list"}"#);
        assert_eq!(
            field(&r, &["error", "code"]),
            Some(&JsValue::Num(f64::from(METHOD_NOT_FOUND)))
        );
        assert_eq!(r.get("id"), Some(&JsValue::Num(4.0)));
        let r = parsed("{not json");
        assert_eq!(
            field(&r, &["error", "code"]),
            Some(&JsValue::Num(f64::from(PARSE_ERROR)))
        );
        assert_eq!(r.get("id"), Some(&JsValue::Null));
        let r = parsed(r#"{"id":5,"method":"ping"}"#);
        assert_eq!(
            field(&r, &["error", "code"]),
            Some(&JsValue::Num(f64::from(INVALID_REQUEST)))
        );
        assert_eq!(r.get("id"), Some(&JsValue::Num(5.0)));
        let r =
            parsed(r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"nope"}}"#);
        assert_eq!(
            field(&r, &["error", "code"]),
            Some(&JsValue::Num(f64::from(INVALID_PARAMS)))
        );
    }

    /// TS `mcp-protocol.test.ts` "lists the five read-only tools with object
    /// schemas".
    #[test]
    fn listed_tools_are_read_only_object_schemas() {
        let r = parsed(r#"{"jsonrpc":"2.0","id":7,"method":"tools/list"}"#);
        let tools = field(&r, &["result", "tools"])
            .and_then(JsValue::as_arr)
            .unwrap_or_default();
        let names: Vec<&str> = tools
            .iter()
            .filter_map(|t| t.get("name").and_then(JsValue::as_str))
            .collect();
        for want in [
            "sessions_list",
            "sessions_search",
            "sessions_analyze",
            "stats_report",
            "stats_drill",
        ] {
            assert!(names.contains(&want), "{want}");
        }
        for t in tools {
            let name = t.get("name").and_then(JsValue::as_str).unwrap_or("");
            assert!(
                (1..=64).contains(&name.len())
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
                "{name}"
            );
            let schema = t.get("inputSchema");
            assert_eq!(
                schema.and_then(|s| s.get("type")).and_then(JsValue::as_str),
                Some("object")
            );
            assert!(schema.and_then(|s| s.get("$schema")).is_none());
            let props = schema.and_then(|s| s.get("properties"));
            assert!(props.and_then(|p| p.get("redact")).is_none(), "{name}");
            assert!(props.and_then(|p| p.get("force")).is_none(), "{name}");
            assert_eq!(
                t.get("annotations").map(|a| a.stringify(false)),
                Some(r#"{"readOnlyHint":true,"openWorldHint":false}"#.to_owned())
            );
        }
    }
}
