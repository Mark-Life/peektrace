use peektrace_protocol::view::PromptOrigin;

use super::*;
use crate::model::fixtures::kind;

fn ev(k: EventKind) -> TimelineEvent {
    kind(k)
}

fn tool(k: EventKind, id: &str) -> TimelineEvent {
    TimelineEvent {
        tool_use_id: Some(id.into()),
        ..kind(k)
    }
}

fn all(events: &[TimelineEvent]) -> Vec<usize> {
    (0..events.len()).collect()
}

fn plan(events: &[TimelineEvent], cross: Option<usize>) -> Vec<ChatNode> {
    build_chat_plan(events, &all(events), &vec![0; events.len()], cross)
}

fn types(nodes: &[ChatNode]) -> Vec<&'static str> {
    nodes
        .iter()
        .map(|n| match n {
            ChatNode::Message(_) => "message",
            ChatNode::Tool { .. } => "tool",
            ChatNode::Run(_) => "run",
            ChatNode::Band(_) => "band",
            ChatNode::Notice(_) => "notice",
            ChatNode::Turn(_) => "turn",
            ChatNode::DumbZone => "dumbzone",
        })
        .collect()
}

#[test]
fn lanes() {
    use EventKind as K;
    let expect = [
        (K::UserPrompt, Lane::User),
        (K::AssistantText, Lane::Agent),
        (K::AssistantThinking, Lane::Agent),
        (K::ToolCall, Lane::Agent),
        (K::ToolResult, Lane::Agent),
        (K::Attachment, Lane::Center),
        (K::SystemPrompt, Lane::Center),
        (K::System, Lane::Center),
        (K::Compaction, Lane::Center),
        (K::Summary, Lane::Center),
        (K::Meta, Lane::Center),
    ];
    for (k, lane) in expect {
        assert_eq!(lane_of(&ev(k)), lane, "{k:?}");
    }
    let side = TimelineEvent {
        is_sidechain: Some(true),
        ..ev(K::UserPrompt)
    };
    assert_eq!(lane_of(&side), Lane::Agent);
    let origin = |o| TimelineEvent {
        prompt_origin: Some(o),
        ..ev(K::UserPrompt)
    };
    assert_eq!(lane_of(&origin(PromptOrigin::Command)), Lane::User);
    assert_eq!(
        lane_of(&origin(PromptOrigin::TaskNotification)),
        Lane::Center
    );
}

#[test]
fn chip_labels() {
    let t = |title: &str| TimelineEvent {
        title: title.into(),
        ..ev(EventKind::Attachment)
    };
    assert_eq!(chip_label(&t("file: main.rs")), "main.rs");
    assert_eq!(chip_label(&t("edited_text_file: a.md")), "a.md");
    assert_eq!(chip_label(&t("hook output")), "hook output");
    let blank = TimelineEvent {
        attachment_type: Some("todo_reminder".into()),
        ..t("  ")
    };
    assert_eq!(chip_label(&blank), "todo reminder");
}

#[test]
fn clamp_counts_units() {
    let e = |n: usize| TimelineEvent {
        body: "x".repeat(n),
        ..ev(EventKind::AssistantText)
    };
    assert!(!needs_clamp(&e(CLAMP_CHARS)));
    assert!(needs_clamp(&e(CLAMP_CHARS + 1)));
}

#[test]
fn calls_pair_with_results() {
    let nodes = plan(
        &[
            tool(EventKind::ToolCall, "t1"),
            tool(EventKind::ToolResult, "t1"),
        ],
        None,
    );
    assert_eq!(
        nodes,
        [ChatNode::Tool {
            call: Some(0),
            result: Some(1)
        }]
    );
    let lone = plan(&[tool(EventKind::ToolResult, "t1")], None);
    assert_eq!(
        lone,
        [ChatNode::Tool {
            call: None,
            result: Some(0)
        }]
    );
    let other = plan(
        &[
            tool(EventKind::ToolCall, "t1"),
            tool(EventKind::ToolResult, "other"),
        ],
        None,
    );
    assert_eq!(other.len(), 2);
    assert_eq!(
        other[0],
        ChatNode::Tool {
            call: Some(0),
            result: None
        }
    );
}

#[test]
fn chips_cluster_and_bands_split() {
    use EventKind as K;
    let p = plan(
        &[
            ev(K::Attachment),
            ev(K::Attachment),
            ev(K::Meta),
            ev(K::AssistantText),
        ],
        None,
    );
    assert_eq!(types(&p), ["run", "message"]);
    let p = plan(
        &[ev(K::Attachment), ev(K::Compaction), ev(K::Attachment)],
        None,
    );
    assert_eq!(types(&p), ["run", "band", "run"]);
    let p = plan(&[ev(K::SystemPrompt), ev(K::Attachment)], None);
    assert_eq!(types(&p), ["band", "run"]);
    let origin = |o| TimelineEvent {
        prompt_origin: Some(o),
        ..ev(K::UserPrompt)
    };
    let p = plan(
        &[
            origin(PromptOrigin::Human),
            origin(PromptOrigin::TaskNotification),
            origin(PromptOrigin::Scheduled),
        ],
        None,
    );
    assert_eq!(types(&p), ["message", "notice", "notice"]);
    let p = plan(
        &[ev(K::Attachment), ev(K::System), ev(K::AssistantText)],
        None,
    );
    assert_eq!(types(&p), ["run", "message"]);
}

#[test]
fn turn_rules_and_dumb_zone() {
    use EventKind as K;
    let events = [
        ev(K::Attachment),
        ev(K::UserPrompt),
        ev(K::AssistantText),
        ev(K::UserPrompt),
    ];
    let nodes = build_chat_plan(&events, &all(&events), &[0, 1, 1, 2], None);
    let turns: Vec<_> = nodes
        .iter()
        .filter(|n| matches!(n, ChatNode::Turn(_)))
        .collect();
    assert_eq!(turns, [&ChatNode::Turn(1), &ChatNode::Turn(2)]);
    let p = plan(&[ev(K::UserPrompt), ev(K::AssistantText)], Some(1));
    assert_eq!(types(&p), ["message", "dumbzone", "message"]);
}

#[test]
fn ids_skip_folded_results() {
    use EventKind as K;
    let events = [
        ev(K::UserPrompt),
        tool(K::ToolCall, "t1"),
        tool(K::ToolResult, "t1"),
        ev(K::Attachment),
    ];
    assert_eq!(
        chat_ids_of(&events, &all(&events)),
        ["chat:0", "chat:1", "chat:3"]
    );
}

#[test]
fn call_lookup() {
    use EventKind as K;
    let events = [
        ev(K::UserPrompt),
        tool(K::ToolCall, "a"),
        tool(K::ToolResult, "a"),
        tool(K::ToolResult, "orphan"),
    ];
    let p = build_chat_plan(&events, &all(&events), &[1, 1, 1, 1], None);
    let at = |id| plan_index_of_call(&events, &p, id).map(|i| p[i].clone());
    assert_eq!(
        at("a"),
        Some(ChatNode::Tool {
            call: Some(1),
            result: Some(2)
        })
    );
    assert_eq!(
        at("orphan"),
        Some(ChatNode::Tool {
            call: None,
            result: Some(3)
        })
    );
    assert_eq!(at("missing"), None);
    let only = build_chat_plan(&events, &[2, 3], &[1, 1, 1, 1], None);
    let i = plan_index_of_call(&events, &only, "a");
    assert_eq!(
        i.map(|i| only[i].clone()),
        Some(ChatNode::Tool {
            call: None,
            result: Some(2)
        })
    );
}

#[test]
fn keys_follow_events() {
    let r = |id: &str| TimelineEvent {
        request_id: Some(id.into()),
        ..ev(EventKind::AssistantText)
    };
    let events = [
        TimelineEvent {
            kind: EventKind::UserPrompt,
            ..r("r1")
        },
        r("r1"),
        r("r2"),
    ];
    let turns = [1, 1, 2];
    let full = chat_node_keys(&build_chat_plan(&events, &all(&events), &turns, None));
    assert_eq!(full, ["turn1", "n0", "n1", "turn2", "n2"]);
    let filtered = chat_node_keys(&build_chat_plan(&events, &[1, 2], &turns, None));
    assert_eq!(filtered, ["turn1", "n1", "turn2", "n2"]);
    let twice = [r("r1"), r("r2"), r("r1")];
    let keys = chat_node_keys(&build_chat_plan(&twice, &all(&twice), &[1, 2, 1], None));
    assert_eq!(keys, ["turn1", "n0", "turn2", "n1", "turn1~1", "n2"]);
}

fn style(body: &str) -> TimelineEvent {
    TimelineEvent {
        attachment_type: Some("output_style".into()),
        body: body.into(),
        title: "output_style".into(),
        ..ev(EventKind::Attachment)
    }
}

fn repeats(events: &[TimelineEvent]) -> Vec<bool> {
    plan(events, None)
        .into_iter()
        .flat_map(|n| match n {
            ChatNode::Run(items) => items.into_iter().map(|i| i.repeat).collect(),
            _ => Vec::new(),
        })
        .collect()
}

#[test]
fn repeat_chips() {
    let reply = || ev(EventKind::AssistantText);
    let s = || style("Style: concise");
    assert_eq!(
        repeats(&[s(), reply(), s(), reply(), s()]),
        [false, true, true]
    );
    assert_eq!(
        repeats(&[s(), reply(), style("other"), reply(), style("other")]),
        [false, false, true]
    );
    let image = TimelineEvent {
        attachment_type: Some("image".into()),
        ..s()
    };
    assert_eq!(repeats(&[image.clone(), reply(), image]), [false, false]);
    let side = TimelineEvent {
        is_sidechain: Some(true),
        ..s()
    };
    assert_eq!(repeats(&[s(), reply(), side]), [false, false]);
}

fn items(repeat: &[bool]) -> Vec<RunItem> {
    repeat
        .iter()
        .enumerate()
        .map(|(pos, &repeat)| RunItem { pos, repeat })
        .collect()
}

#[test]
fn run_segments() {
    let run = items(&[false; 4]);
    assert_eq!(segments_of(&run, |_| false), [Segment::Chips(run.clone())]);
    let s = segments_of(&run, |i| i.pos == 1);
    assert_eq!(
        s,
        [
            Segment::Chips(vec![run[0]]),
            Segment::Card(run[1]),
            Segment::Chips(run[2..].to_vec())
        ]
    );
    let s = segments_of(&run, |i| i.pos == 0 || i.pos == 1);
    assert_eq!(
        s,
        [
            Segment::Card(run[0]),
            Segment::Card(run[1]),
            Segment::Chips(run[2..].to_vec())
        ]
    );
}

#[test]
fn chip_windows() {
    let run = items(&[false, true, false, true]);
    let w = chip_window(&run, false, false, |_| false);
    assert_eq!((w.shown.len(), w.folded, w.hidden), (2, 2, 0));
    let w = chip_window(&run, false, false, |i| i.pos == 1);
    assert_eq!((w.shown.len(), w.folded), (4, 0));
    let w = chip_window(&run, false, false, |i| i.pos == 0);
    assert_eq!(w.folded, 2);
    let long = items(&[false; 9]);
    let w = chip_window(&long, false, false, |_| false);
    assert_eq!((w.shown.len(), w.hidden), (CHIP_LIMIT, 3));
    let w = chip_window(&long, false, true, |_| false);
    assert_eq!((w.shown.len(), w.hidden), (9, 0));
    let mostly = items(&[false, true, true, true, true, true, true, true, false]);
    let w = chip_window(&mostly, false, true, |_| false);
    assert_eq!((w.shown.len(), w.folded, w.hidden), (9, 0, 0));
}

#[test]
fn visible_rows_skip_system_events() {
    use EventKind as K;
    let events = [
        ev(K::Attachment),
        ev(K::System),
        ev(K::UserPrompt),
        ev(K::System),
        ev(K::AssistantText),
    ];
    let rows = visible_rows(&events);
    assert_eq!(rows, [0, 2, 4]);
    let nodes = build_chat_plan(&events, &rows, &vec![0; events.len()], None);
    assert_eq!(types(&nodes), ["run", "message", "message"]);
    assert_eq!(
        nodes[0],
        ChatNode::Run(vec![RunItem {
            pos: 0,
            repeat: false
        }])
    );
    assert_eq!(chat_ids_of(&events, &rows), ["chat:0", "chat:2", "chat:4"]);
}

#[test]
fn first_spawn_follows_the_transcript_not_the_subagent_list() {
    use EventKind as K;
    let agent = |id: Option<&str>| SubagentRef {
        id: "a".into(),
        agent_type: None,
        description: None,
        tool_use_id: id.map(Into::into),
        turns: 0,
        peak_context_tokens: 0.0,
    };
    let events = [
        tool(K::ToolCall, "other"),
        tool(K::ToolCall, "early"),
        tool(K::ToolCall, "late"),
    ];
    let list = [agent(Some("late")), agent(None), agent(Some("early"))];
    assert_eq!(first_spawn(&events, &list), Some("early"));
    assert_eq!(first_spawn(&events, &[agent(Some("gone"))]), None);
}
