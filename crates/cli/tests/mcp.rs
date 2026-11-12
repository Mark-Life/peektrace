//! `peektrace mcp` over a real process (TS `mcp-stdio.test.ts` and
//! `mcp-tools.test.ts`): one synthetic home, every request in one script,
//! replies looked up by id.

mod common;

use std::collections::HashMap;

use common::{Home, claude_lines, claude_path};
use serde_json::{Value, json};

const ID: &str = "44444444-4444-4444-8444-444444444444";
const OLDER: &str = "55555555-5555-4555-8555-555555555555";

fn home() -> Home {
    let home = Home::new("mcp");
    home.write(
        &claude_path("-home-demo-work", ID),
        &claude_lines("2026-03-01T10:00:00.000Z", 20_000, "/home/demo/work"),
    );
    home.write(
        &claude_path("-home-demo-other", OLDER),
        &claude_lines("2026-02-01T10:00:00.000Z", 10_000, "/home/demo/other"),
    );
    home
}

fn call(id: &str, name: &str, args: &Value) -> String {
    json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":args}})
        .to_string()
}

/// Run `lines` through one server; replies by id.
fn serve(home: &Home, lines: &[String]) -> HashMap<String, Value> {
    let input = format!("{}\n", lines.join("\n"));
    let r = home.run_stdin(&["mcp"], input.as_bytes(), &[]);
    assert_eq!(r.code, 0, "{}", r.err);
    assert_eq!(r.err, "");
    r.out
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let v: Value = serde_json::from_str(l).expect("reply is JSON");
            (v["id"].to_string().trim_matches('"').to_owned(), v)
        })
        .collect()
}

fn text(r: &Value, block: usize) -> &str {
    r["result"]["content"][block]["text"].as_str().unwrap_or("")
}

fn is_error(r: &Value) -> bool {
    r["result"]["isError"] == json!(true)
}

#[test]
fn initialize_list_and_call_over_stdio() {
    let h = home();
    let lines = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}).to_string(),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}).to_string(),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}).to_string(),
        call("3", "sessions_list", &json!({})),
    ];
    let by_id = serve(&h, &lines);
    assert_eq!(by_id.len(), 3);
    assert_eq!(by_id["1"]["result"]["serverInfo"]["name"], "peektrace");
    let names: Vec<&str> = by_id["2"]["result"]["tools"]
        .as_array()
        .map(|t| t.iter().filter_map(|t| t["name"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(
        names,
        [
            "sessions_list",
            "sessions_search",
            "sessions_analyze",
            "stats_report",
            "stats_drill"
        ]
    );
    assert!(text(&by_id["3"], 0).contains("44444444"));
}

#[test]
fn a_raw_u2028_inside_a_request_still_gets_one_reply_with_its_id() {
    let h = home();
    let line = json!({"jsonrpc":"2.0","id":7,"method":"tools/call",
        "params":{"name":"sessions_list","arguments":{"a\u{2028}b":1}}})
    .to_string();
    assert!(line.contains('\u{2028}'));
    let by_id = serve(&h, &[line]);
    assert_eq!(by_id.len(), 1);
    assert!(is_error(&by_id["7"]));
}

#[test]
fn sessions_list_tool() {
    let h = home();
    let by_id = serve(
        &h,
        &[
            call("table", "sessions_list", &json!({})),
            call("json", "sessions_list", &json!({"format":"json"})),
            call(
                "hidden",
                "sessions_list",
                &json!({"format":"json","limit":1}),
            ),
            call("tilde", "sessions_list", &json!({"cwd":"~/nowhere"})),
            call("since", "sessions_list", &json!({"since":"soon"})),
        ],
    );
    let t = &by_id["table"];
    assert!(!is_error(t));
    assert!(text(t, 0).starts_with("id\tagent\tproject"));
    assert!(text(t, 0).contains("44444444"));
    assert!(text(t, 1).contains("EST COST covers the main transcript only"));

    let rows: Value = serde_json::from_str(text(&by_id["json"], 0)).expect("json");
    let ids: Vec<&str> = rows
        .as_array()
        .map(|r| r.iter().filter_map(|r| r["id"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(ids, [ID, OLDER]);
    assert_eq!(
        by_id["json"]["result"]["content"].as_array().map(Vec::len),
        Some(1)
    );

    let hidden = &by_id["hidden"];
    let rows: Value = serde_json::from_str(text(hidden, 0)).expect("json");
    assert_eq!(rows[0]["id"], ID);
    assert_eq!(text(hidden, 1), "1 more hidden, use limit: 0 to show all");

    assert!(!is_error(&by_id["tilde"]));
    assert!(!text(&by_id["tilde"], 0).contains("44444444"));

    assert!(is_error(&by_id["since"]));
    assert!(text(&by_id["since"], 0).starts_with("Invalid since \"soon\""));
    assert!(!text(&by_id["since"], 0).contains("--"));
}

#[test]
fn sessions_search_tool() {
    let h = home();
    let by_id = serve(
        &h,
        &[
            call("hits", "sessions_search", &json!({"q":"hello"})),
            call("none", "sessions_search", &json!({"q":"zzzz-no-match"})),
            call(
                "json",
                "sessions_search",
                &json!({"q":"hello","limit":1,"format":"json"}),
            ),
            call("blank", "sessions_search", &json!({"q":"  "})),
            call("missing", "sessions_search", &json!({})),
        ],
    );
    let hits = text(&by_id["hits"], 0);
    assert!(hits.starts_with("session\tagent\tproject"));
    assert!(hits.contains("44444444") && hits.contains("55555555") && hits.contains("hello"));
    assert_eq!(text(&by_id["none"], 0), "no matches in 2 sessions");
    let result: Value = serde_json::from_str(text(&by_id["json"], 0)).expect("json");
    assert_eq!(result["sessions"][0]["id"], ID);
    assert_eq!(result["truncated"], true);
    assert_eq!(
        text(&by_id["json"], 1),
        "more sessions match, use limit: 0 to show all"
    );
    assert!(is_error(&by_id["blank"]));
    assert_eq!(text(&by_id["blank"], 0), "Give a non-empty search text.");
    assert!(is_error(&by_id["missing"]));
    assert!(text(&by_id["missing"], 0).ends_with("└─ [\"q\"]\n   └─ is missing"));
}

#[test]
fn sessions_analyze_tool() {
    let h = home();
    let by_id = serve(
        &h,
        &[
            call("latest", "sessions_analyze", &json!({"latest":true})),
            call("none", "sessions_analyze", &json!({})),
            call(
                "json",
                "sessions_analyze",
                &json!({"id":"44444444","format":"json"}),
            ),
        ],
    );
    assert!(!is_error(&by_id["latest"]));
    assert!(text(&by_id["latest"], 0).contains("verdict"));
    assert!(text(&by_id["latest"], 0).contains(ID));
    assert!(is_error(&by_id["none"]));
    assert_eq!(text(&by_id["none"], 0), "Pass id, latest: true, or cwd.");
    let a: Value = serde_json::from_str(text(&by_id["json"], 0)).expect("json");
    assert_eq!(a["sessionId"], ID);
}

#[test]
fn stats_tools() {
    let h = home();
    let by_id = serve(
        &h,
        &[
            call("text", "stats_report", &json!({"days":3650})),
            call(
                "json",
                "stats_report",
                &json!({"days":3650,"format":"json"}),
            ),
            call(
                "drill",
                "stats_drill",
                &json!({"table":"cause","key":"no-such-cause","days":3650}),
            ),
        ],
    );
    let t = text(&by_id["text"], 0);
    assert!(!is_error(&by_id["text"]));
    assert!(t.starts_with("peektrace stats  3650d window"), "{t}");
    assert!(t.contains("drill a row with stats_drill"));
    assert!(!t.contains("peektrace stats drill"));
    let r: Value = serde_json::from_str(text(&by_id["json"], 0)).expect("json");
    assert!(r["schemaVersion"].is_string() || r["schemaVersion"].is_number());
    assert!(is_error(&by_id["drill"]));
    let d = text(&by_id["drill"], 0);
    assert!(
        d.contains("no-such-cause") && d.contains("Run stats_report"),
        "{d}"
    );
}

#[test]
fn remote_is_refused() {
    let h = home();
    let r = h.run_stdin(&["--remote", "http://x", "mcp"], b"", &[]);
    assert_eq!(r.code, 1);
    assert_eq!(
        r.err,
        "mcp runs locally only; remote is not supported yet\n"
    );
}
