//! Ports of the chat cases of `apps/inspector/test/transcript-render.test.tsx`:
//! one of every event kind, rendered through Rune's `Runtime`.

use peektrace_protocol::view::{AnalyzedSession, EventKind, PromptOrigin, TimelineEvent};
use rune::{Cx, El, ElementInfo, Env, Runtime, ScrollSpec, col};

use crate::model::chat::{build_chat_plan, chat_ids_of, chat_node_keys};
use crate::model::fixtures::{analysis, kind, turn};
use crate::state::{Detail, Route, Selected, Ui};
use crate::theme::theme;

fn evt(index: usize, k: EventKind, body: &str, request: &str) -> TimelineEvent {
    TimelineEvent {
        index,
        body: body.into(),
        request_id: (!request.is_empty()).then(|| request.to_owned()),
        ..kind(k)
    }
}

/// One of every kind, plus a paired tool run, an orphan result, a cluster
/// of attachments, and a sidechain.
fn events() -> Vec<TimelineEvent> {
    use EventKind as K;
    vec![
        evt(0, K::UserPrompt, "hello", "r1"),
        evt(1, K::AssistantText, "sure", "r1"),
        TimelineEvent {
            tokens_est: 900.0,
            ..evt(2, K::AssistantThinking, "", "r1")
        },
        TimelineEvent {
            tool_name: Some("Read".into()),
            tool_use_id: Some("t1".into()),
            ..evt(3, K::ToolCall, r#"{"file_path":"/a/b.ts"}"#, "r1")
        },
        TimelineEvent {
            tool_use_id: Some("t1".into()),
            ..evt(4, K::ToolResult, "file contents", "r1")
        },
        TimelineEvent {
            is_error: Some(true),
            tool_use_id: Some("orphan".into()),
            ..evt(5, K::ToolResult, "boom", "r1")
        },
        TimelineEvent {
            loaded_category: Some("claude-md".into()),
            title: "file: CLAUDE.md".into(),
            tokens_est: 1200.0,
            ..evt(6, K::Attachment, "# project rules", "")
        },
        TimelineEvent {
            loaded_category: Some("skills".into()),
            title: "skill_listing (12 skills)".into(),
            ..evt(7, K::Attachment, "skills", "")
        },
        evt(8, K::System, "ignored", ""),
        evt(9, K::Compaction, "summary of history", ""),
        evt(10, K::Summary, "running notes", ""),
        TimelineEvent {
            title: "meta".into(),
            ..evt(11, K::Meta, "note", "")
        },
        TimelineEvent {
            is_sidechain: Some(true),
            ..evt(12, K::UserPrompt, "sub work", "r2")
        },
        evt(13, K::AssistantText, &"x".repeat(1200), "r2"),
        TimelineEvent {
            title: "System prompt".into(),
            tokens_est: 2000.0,
            ..evt(
                14,
                K::SystemPrompt,
                "## preamble\n\nYou are a coding agent.",
                "",
            )
        },
        TimelineEvent {
            prompt_origin: Some(PromptOrigin::TaskNotification),
            title: "Task notification".into(),
            ..evt(
                15,
                K::UserPrompt,
                "<task-notification>\n<task-id>b7</task-id>\n<status>completed</status>\n</task-notification>",
                "",
            )
        },
    ]
}

fn session() -> AnalyzedSession {
    AnalyzedSession {
        compaction_indexes: vec![9],
        dumb_zone_cross_turn: 1,
        dumb_zone_fraction: 0.5,
        dumb_zone_turns: 1,
        events: events(),
        models: vec!["claude-opus-4".into()],
        session_id: "s".into(),
        tool_call_count: 1,
        turn_count: 2,
        turns: vec![turn("r1"), turn("r2")],
        user_message_count: 2,
        ..analysis()
    }
}

/// The rows without the `system` event, the TS turn numbers (no carry) and
/// the crossing at event 9, as the TS test passes them.
fn state(open: bool) -> Ui {
    let a = session();
    let rows: Vec<usize> = (0..a.events.len())
        .filter(|&p| a.events[p].kind != EventKind::System)
        .collect();
    let turns: Vec<usize> = a
        .events
        .iter()
        .map(|e| match e.request_id.as_deref() {
            Some("r1") => 1,
            Some("r2") => 2,
            _ => 0,
        })
        .collect();
    let mut d = Detail::new("s".into(), a, 1);
    d.plan = build_chat_plan(&d.a.events, &rows, &turns, Some(9));
    d.keys = chat_node_keys(&d.plan);
    d.ids = chat_ids_of(&d.a.events, &rows);
    let mut s = Ui::new(None, 0.0, true);
    s.route = Route::Sessions;
    s.selected = Some(Selected {
        id: "s".into(),
        parent: None,
    });
    if open {
        s.open = d.ids.iter().cloned().collect();
    }
    s.detail = Some(d);
    s
}

fn view(s: &Ui, _cx: &mut Cx<Ui>) -> El {
    let t = theme(true);
    let Some(d) = &s.detail else {
        return rune::el();
    };
    col()
        .w_full()
        .h_full()
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(super::rows(d, &t))
}

fn render(open: bool) -> Runtime<Ui> {
    let mut rt = Runtime::new(state(open), view, (1440.0, 6000.0), Env::new(true, 1.0));
    for _ in 0..6 {
        rt.frame(1.0 / 60.0);
    }
    rt
}

fn texts(els: &[ElementInfo]) -> String {
    els.iter()
        .filter_map(|e| e.text.clone().or_else(|| e.label.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_chat_id(k: &str) -> bool {
    k.strip_prefix("chat:")
        .is_some_and(|n| n.chars().all(|c| c.is_ascii_digit()))
}

/// Chat nodes drawn as their own block (not chips in a run).
fn blocks(els: &[ElementInfo]) -> Vec<String> {
    els.iter()
        .filter(|e| e.role != "button")
        .filter_map(|e| e.key.clone().filter(|k| is_chat_id(k)))
        .collect()
}

fn chips(els: &[ElementInfo]) -> usize {
    els.iter()
        .filter(|e| e.role == "button" && e.key.as_deref().is_some_and(is_chat_id))
        .count()
}

#[test]
fn chat_renders_every_event_kind() {
    let rt = render(false);
    let els = rt.elements();
    let all = texts(&els);
    assert!(els.iter().any(|e| e.key.as_deref() == Some("dumbzone")));
    assert!(
        els.iter()
            .any(|e| e.role == "heading" && e.key.as_deref() == Some("turn1"))
    );
    // 15 non-system rows, minus the result folded into its call, minus the
    // three context injections that render as chips.
    assert_eq!(blocks(&els).len(), 11, "{:?}", blocks(&els));
    assert!(all.contains("system prompt · ~2K"), "{all}");
    assert!(!all.contains("You are a coding agent."));
    assert_eq!(chips(&els), 3);
    let mut runs: Vec<&str> = els
        .iter()
        .filter_map(|e| e.key.as_deref().filter(|k| k.starts_with("run")))
        .collect();
    runs.dedup();
    assert_eq!(runs, ["run6", "run11"]);
    // The paired card shows both token counts; the orphan has no call.
    assert!(all.contains("~0 → ~0"));
    assert!(all.contains("— → ~0"));
    assert!(all.lines().any(|l| l == "sidechain"));
    assert!(all.lines().any(|l| l == "CLAUDE.md"));
}

#[test]
fn chat_renders_expanded() {
    let all = texts(&render(true).elements());
    assert!(all.contains("file contents"), "{all}");
    assert!(all.contains("running notes"), "{all}");
    assert!(all.contains("You are a coding agent."), "{all}");
}

/// Toggles with an open state, minus tool cards and chips (as TS leaves
/// out its collapsible triggers and chip buttons).
fn toggles(els: &[ElementInfo]) -> Vec<bool> {
    let cards: Vec<usize> = els
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            e.key
                .as_deref()
                .is_some_and(|k| ["chat:3", "chat:5"].contains(&k))
        })
        .map(|(i, _)| i)
        .collect();
    els.iter()
        .filter(|e| {
            e.role == "button"
                && !e.key.as_deref().is_some_and(is_chat_id)
                && !e.parent.is_some_and(|p| cards.contains(&p))
        })
        .filter_map(|e| e.expanded)
        .collect()
}

#[test]
fn collapsed_chat_toggles_say_closed() {
    let rt = render(false);
    let els = rt.elements();
    assert!(!els.iter().any(|e| e.expanded == Some(true)));
    // Three bands, the thinking strip, the notice, Show all.
    assert_eq!(toggles(&els), [false; 6]);
    // Only Show all keeps its body mounted while closed.
    let all = texts(&els);
    assert!(all.contains(&"x".repeat(1200)));
    for hidden in [
        "summary of history",
        "running notes",
        "You are a coding agent.",
        "task-id",
    ] {
        assert!(!all.contains(hidden), "{hidden}");
    }
}

#[test]
fn expanded_chat_toggles_say_open_and_show_their_bodies() {
    let rt = render(true);
    let els = rt.elements();
    assert!(!els.iter().any(|e| e.expanded == Some(false)));
    assert_eq!(toggles(&els), [true; 6]);
    let all = texts(&els);
    for shown in [
        "summary of history",
        "running notes",
        "You are a coding agent.",
        "task-id",
        "b7",
        "completed",
        &"x".repeat(1200),
    ] {
        assert!(all.contains(shown), "{shown}");
    }
    // The notification reads as a field table, not its tags.
    assert!(!all.contains("<task-id>"));
}

#[test]
fn the_chat_mounts_as_a_list_before_the_page_has_scrolled() {
    let rt = render(false);
    let els = rt.elements();
    assert!(
        els.iter()
            .any(|e| e.key.as_deref() == Some(super::ROWS_KEY))
    );
    assert_ne!(blocks(&els), Vec::<String>::new());
}
