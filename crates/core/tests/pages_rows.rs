//! Session pages built from hand-made events: the cases the inspector tests
//! check on its side of `/history/index` and `/history/search`.

use peektrace_core::sessions::model::{
    AnalyzedSession, EventKind, EventShape, ParsedSession, TimelineEvent, Turn, TurnTokens,
};
use peektrace_core::sessions::pages::SessionPages;
use serde_json::{Value, json};

fn evt(kind: EventKind, title: &str, preview: &str, body: &str) -> TimelineEvent {
    TimelineEvent::new(EventShape::AssistantBlock, 0, kind).text(
        title.into(),
        preview.into(),
        body.into(),
        0.0,
    )
}

fn req(mut e: TimelineEvent, id: &str) -> TimelineEvent {
    e.request_id = Some(id.into());
    e
}

fn session(events: Vec<TimelineEvent>, turns: &[&str], cross: i64) -> SessionPages {
    let events = events
        .into_iter()
        .enumerate()
        .map(|(i, mut e)| {
            e.index = i;
            e
        })
        .collect();
    let turns = turns
        .iter()
        .map(|r| Turn::new((*r).into(), "m".into(), TurnTokens::default(), None))
        .collect();
    SessionPages::new(AnalyzedSession {
        parsed: ParsedSession {
            events,
            turns,
            ..ParsedSession::default()
        },
        dumb_zone_cross_turn: cross,
        ..AnalyzedSession::default()
    })
}

fn index(p: &SessionPages) -> Value {
    serde_json::from_str(p.index()).expect("index json")
}

fn column(p: &SessionPages, key: &str) -> Value {
    Value::Array(
        index(p)["rows"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|r| r[key].clone())
            .collect(),
    )
}

/// `history-search.test.tsx`: three lines of text, one tool call, and a
/// `system` event first.
fn zephyr() -> SessionPages {
    use EventKind::{AssistantText, System, ToolCall};
    session(
        vec![
            evt(System, "boot", "", "boot"),
            evt(AssistantText, "intro", "intro", "intro"),
            evt(
                AssistantText,
                "first line",
                "first line",
                "first line\nsecond line\nthe Zephyr valve leaks",
            ),
            evt(ToolCall, "Bash", "ls", r#"{"cmd":"ls zephyr"}"#),
            evt(AssistantText, "done", "done", "done"),
        ],
        &[],
        -1,
    )
}

#[test]
fn search_matches_text_past_the_first_line_ignoring_case() {
    let p = zephyr();
    assert_eq!(p.search("zephyr"), [2, 3]);
    assert_eq!(p.search("ZEPHYR VALVE"), [2]);
}

#[test]
fn search_keeps_system_events_for_the_client_to_drop() {
    let p = zephyr();
    assert_eq!(p.search("boot"), [0]);
    assert_eq!(p.search("o"), [0, 1, 2, 4]);
}

/// `chat-lane.test.ts` "index rows": output styles repeat, one long body.
fn chat_lane() -> SessionPages {
    use EventKind::{AssistantText, Attachment, ToolCall, ToolResult, UserPrompt};
    let style = |b: &str, r: &str| {
        let mut e = req(evt(Attachment, "", "", b), r);
        e.attachment_type = Some("output_style".into());
        e
    };
    let tool = |k: EventKind, b: &str| {
        let mut e = req(evt(k, "", "", b), "r1");
        e.tool_use_id = Some("t".into());
        e
    };
    let long = "x".repeat(900);
    session(
        vec![
            evt(UserPrompt, "", "", "go"),
            style("A", "r1"),
            req(evt(AssistantText, "", "", &long), "r1"),
            tool(ToolCall, "{}"),
            tool(ToolResult, "out"),
            style("A", "r2"),
            req(evt(AssistantText, "", "", "reply"), "r2"),
            style("B", "r2"),
            req(evt(AssistantText, "", "", "end"), "r3"),
            style("B", "r3"),
        ],
        &["r1", "r2", "r3"],
        1,
    )
}

#[test]
fn body_ids_name_the_first_equal_body() {
    let p = chat_lane();
    assert_eq!(column(&p, "bodyId"), json!([0, 1, 2, 3, 4, 1, 6, 7, 8, 7]));
    assert_eq!(
        column(&p, "bodyLen"),
        json!([2, 1, 900, 2, 3, 1, 5, 1, 3, 1])
    );
}

#[test]
fn rows_carry_the_turn_of_each_event() {
    let p = chat_lane();
    assert_eq!(column(&p, "turn"), json!([0, 1, 1, 1, 1, 2, 2, 2, 3, 3]));
    assert_eq!(index(&p)["crossEvent"], json!(5));
}

#[test]
fn rows_keep_what_jumps_and_chips_read() {
    let p = chat_lane();
    let rows = index(&p)["rows"].as_array().cloned().unwrap_or_default();
    assert_eq!(rows[1]["kind"], json!("attachment"));
    assert_eq!(rows[3]["toolUseId"], json!("t"));
    assert_eq!(rows[4]["kind"], json!("tool-result"));
    for r in &rows {
        for k in ["body", "preview", "requestId", "ts"] {
            assert!(r.get(k).is_none(), "{k} in {r}");
        }
    }
}

#[test]
fn body_length_counts_utf16_units() {
    use EventKind::AssistantText;
    let p = session(
        vec![
            evt(AssistantText, "", "", "héllo 👋"),
            evt(AssistantText, "", "", &"x".repeat(2000)),
        ],
        &[],
        -1,
    );
    assert_eq!(column(&p, "bodyLen"), json!([8, 2000]));
}

#[test]
fn an_interleaved_turn_switches_back() {
    use EventKind::AssistantText;
    let p = session(
        vec![
            req(evt(AssistantText, "", "", "a"), "r1"),
            req(evt(AssistantText, "", "", "b"), "r2"),
            req(evt(AssistantText, "", "", "c"), "r1"),
            evt(AssistantText, "", "", "d"),
            req(evt(AssistantText, "", "", "e"), "unknown"),
        ],
        &["r1", "r2"],
        -1,
    );
    assert_eq!(column(&p, "turn"), json!([1, 2, 1, 1, 1]));
    assert_eq!(index(&p)["crossEvent"], json!(-1));
}

#[test]
fn a_repeated_request_id_maps_to_its_last_turn() {
    // `new Map(turns.map(...))` keeps the last entry per key.
    use EventKind::AssistantText;
    let p = session(
        vec![req(evt(AssistantText, "", "", "a"), "r")],
        &["r", "s", "r"],
        -1,
    );
    assert_eq!(column(&p, "turn"), json!([3]));
}

#[test]
fn a_cross_turn_past_the_turns_matches_an_event_with_no_request() {
    // `turns[n]?.requestId` is `undefined`, which equals an unset `requestId`.
    use EventKind::AssistantText;
    let events = || {
        vec![
            req(evt(AssistantText, "", "", "a"), "r1"),
            evt(AssistantText, "", "", "b"),
        ]
    };
    assert_eq!(
        index(&session(events(), &["r1"], 5))["crossEvent"],
        json!(1)
    );
    let none = session(vec![req(evt(AssistantText, "", "", "a"), "r1")], &["r1"], 5);
    assert_eq!(index(&none)["crossEvent"], json!(-1));
}

#[test]
fn no_turns_and_no_events() {
    let p = session(Vec::new(), &[], -1);
    let i = index(&p);
    assert_eq!(i["rows"], json!([]));
    assert_eq!(i["total"], json!(0));
    assert_eq!(i["crossEvent"], json!(-1));
    assert_eq!(p.search("a"), Vec::<usize>::new());
}
