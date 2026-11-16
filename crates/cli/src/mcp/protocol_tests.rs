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

/// The TS server's entries, byte for byte: one with a positive int and
/// required fields, one with a non-negative int and every string kind.
#[test]
fn listing_matches_the_ts_server() {
    let entry = |name: &str| tools::find(name).map(|t| t.listing().stringify(false));
    assert_eq!(
        entry("sessions_list").as_deref(),
        Some(
            r#"{"name":"sessions_list","description":"List local coding-agent sessions (Claude Code, Codex, Pi, OpenCode), newest first: id, agent, project, model, message count, estimated cost, size, update time and title. Use an id with sessions_analyze.","inputSchema":{"type":"object","required":[],"properties":{"agent":{"type":"string","enum":["claude","codex","pi","opencode","all"],"description":"Agent to scope to (default all)"},"project":{"type":"string","description":"Project name (Claude/Pi slug, or cwd basename for Codex/OpenCode)"},"since":{"type":"string","description":"Only sessions updated since: ISO date (2026-01-31, UTC) or relative 7d, 12h, 2w"},"cwd":{"type":"string","description":"Only sessions run in this directory or below; `~` is the home dir, relative paths resolve against the server's cwd"},"limit":{"type":"integer","description":"Max sessions, newest first (default 30, 0 = all)","title":"nonNegative","minimum":0},"format":{"type":"string","enum":["text","json"],"description":"text (default): compact tab-separated tables; json: the same payload as the CLI's --json"}},"additionalProperties":false},"annotations":{"readOnlyHint":true,"openWorldHint":false}}"#
        )
    );
    assert_eq!(
        entry("stats_drill").as_deref(),
        Some(
            r#"{"name":"stats_drill","description":"The calls behind one stats_report row. A DRILL cell `--cause K` maps to {table:\"cause\", key:\"K\"}; likewise --fail and --time.","inputSchema":{"type":"object","required":["table","key"],"properties":{"table":{"type":"string","enum":["cause","fail","time","cluster","wait"],"description":"Table the key comes from"},"key":{"type":"string","description":"Key from the DRILL cell, without the --table prefix"},"offset":{"type":"integer","description":"First hit to return (default 0)","title":"nonNegative","minimum":0},"pageSize":{"type":"integer","description":"Hits per page (default 200)","title":"positive","exclusiveMinimum":0},"days":{"type":"integer","description":"History window in days (default 60)","title":"positive","exclusiveMinimum":0},"limit":{"type":"integer","description":"Rows kept per table (default 20)","title":"positive","exclusiveMinimum":0},"agent":{"type":"string","enum":["claude","codex","pi","opencode","all"],"description":"Agent to scope to (default all)"},"format":{"type":"string","enum":["text","json"],"description":"text (default): compact tab-separated tables; json: the same payload as the CLI's --json"}},"additionalProperties":false},"annotations":{"readOnlyHint":true,"openWorldHint":false}}"#
        )
    );
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
    let r = parsed(r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"nope"}}"#);
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
