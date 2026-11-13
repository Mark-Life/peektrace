//! Ports of `chat-lane.test.ts`, `chat-context-run.test.ts` and the chat-plan
//! cases of `history-jump.test.ts` that `tests.rs` does not already hold.

use peektrace_protocol::view::PromptOrigin;

use super::*;
use crate::model::fixtures::kind;

fn ev(k: EventKind) -> TimelineEvent {
    kind(k)
}

fn all(events: &[TimelineEvent]) -> Vec<usize> {
    (0..events.len()).collect()
}

fn plan(events: &[TimelineEvent]) -> Vec<ChatNode> {
    build_chat_plan(events, &all(events), &[], None)
}

// chat-lane.test.ts

#[test]
fn a_harness_prompt_is_centred_a_persons_stays_right() {
    let prompt = |o| TimelineEvent {
        prompt_origin: Some(o),
        ..ev(EventKind::UserPrompt)
    };
    for o in [
        PromptOrigin::Human,
        PromptOrigin::Sdk,
        PromptOrigin::Command,
    ] {
        assert_eq!(lane_of(&prompt(o)), Lane::User, "{o:?}");
    }
    for o in [
        PromptOrigin::TaskNotification,
        PromptOrigin::Scheduled,
        PromptOrigin::Harness,
    ] {
        assert_eq!(lane_of(&prompt(o)), Lane::Center, "{o:?}");
    }
}

#[test]
fn no_title_falls_back_to_the_badge_word() {
    let e = TimelineEvent {
        attachment_type: Some("todo".into()),
        ..ev(EventKind::Attachment)
    };
    assert_eq!(chip_label(&e), "todo");
}

#[test]
fn adjacent_context_injections_merge_into_one_cluster() {
    use EventKind as K;
    let nodes = plan(&[
        ev(K::Attachment),
        ev(K::Attachment),
        ev(K::Meta),
        ev(K::AssistantText),
    ]);
    assert_eq!(nodes.len(), 2);
    assert!(matches!(&nodes[0], ChatNode::Run(items) if items.len() == 3));
    assert_eq!(nodes[1], ChatNode::Message(3));
}

fn style(attachment_type: &str, body: &str, title: &str) -> TimelineEvent {
    TimelineEvent {
        attachment_type: Some(attachment_type.into()),
        body: body.into(),
        title: title.into(),
        ..ev(EventKind::Attachment)
    }
}

fn concise() -> TimelineEvent {
    style("output_style", "Style: concise", "output_style")
}

fn reply(n: usize) -> TimelineEvent {
    TimelineEvent {
        body: format!("reply {n}"),
        ..ev(EventKind::AssistantText)
    }
}

fn repeats_of(events: &[TimelineEvent]) -> Vec<bool> {
    plan(events)
        .into_iter()
        .flat_map(|n| match n {
            ChatNode::Run(items) => items.into_iter().map(|i| i.repeat).collect(),
            _ => Vec::new(),
        })
        .collect()
}

#[test]
fn a_changed_body_is_fresh_then_the_new_baseline() {
    let verbose = || style("output_style", "Style: verbose", "output_style");
    let events = [concise(), reply(1), verbose(), reply(2), verbose()];
    assert_eq!(repeats_of(&events), [false, false, true]);
}

#[test]
fn the_same_label_under_another_attachment_type_is_fresh() {
    let other = style("other_type", "Style: concise", "output_style");
    assert_eq!(repeats_of(&[concise(), reply(1), other]), [false, false]);
}

#[test]
fn different_images_with_the_same_label_never_fold() {
    let image = || style("image", "[image: image/png]", "image");
    let events = [image(), reply(1), image(), reply(2), image()];
    assert_eq!(repeats_of(&events), [false, false, false]);
}

#[test]
fn a_run_keeps_its_items_and_order_when_some_repeat() {
    let other = TimelineEvent {
        title: "other".into(),
        ..concise()
    };
    let nodes = plan(&[concise(), reply(1), concise(), other]);
    let last = nodes.iter().rev().find_map(|n| match n {
        ChatNode::Run(items) => Some(items.iter().map(|i| i.pos).collect::<Vec<_>>()),
        _ => None,
    });
    assert_eq!(last, Some(vec![2, 3]));
}

// chat-context-run.test.ts

fn item(pos: usize, repeat: bool) -> RunItem {
    RunItem { pos, repeat }
}

/// A run of six in order, with `open` positions split out as cards.
fn shape(open: &[usize]) -> Vec<String> {
    let run: Vec<RunItem> = (0..6).map(|p| item(p, false)).collect();
    segments_of(&run, |i| open.contains(&i.pos))
        .into_iter()
        .map(|s| match s {
            Segment::Card(i) => format!("[{}]", i.pos),
            Segment::Chips(items) => items
                .iter()
                .map(|i| i.pos.to_string())
                .collect::<Vec<_>>()
                .join(","),
        })
        .collect()
}

#[test]
fn a_closed_run_is_one_row_of_chips() {
    assert_eq!(shape(&[]), ["0,1,2,3,4,5"]);
}

#[test]
fn an_opened_chip_splits_the_run_around_it() {
    assert_eq!(shape(&[3]), ["0,1,2", "[3]", "4,5"]);
    assert_eq!(shape(&[1, 3]), ["0", "[1]", "2", "[3]", "4,5"]);
}

#[test]
fn adjacent_and_edge_cards_leave_no_empty_chip_rows() {
    assert_eq!(shape(&[0, 1]), ["[0]", "[1]", "2,3,4,5"]);
    assert_eq!(shape(&[5]), ["0,1,2,3,4", "[5]"]);
}

fn fold_items() -> Vec<RunItem> {
    vec![item(0, false), item(1, true), item(2, true)]
}

fn positions(w: &ChipWindow) -> Vec<usize> {
    w.shown.iter().map(|i| i.pos).collect()
}

#[test]
fn repeats_fold_into_a_count_and_leave_fresh_chips_in_order() {
    let w = chip_window(&fold_items(), false, false, |_| false);
    assert_eq!(positions(&w), [0]);
    assert_eq!(w.folded, 2);
}

#[test]
fn a_run_of_only_repeats_folds_to_nothing_visible() {
    let w = chip_window(&[item(0, true), item(1, true)], false, false, |_| false);
    assert_eq!(positions(&w), Vec::<usize>::new());
    assert_eq!(w.folded, 2);
}

#[test]
fn show_expand_all_or_an_opened_repeat_reveal_every_chip() {
    let items = fold_items();
    for w in [
        chip_window(&items, false, true, |_| false),
        chip_window(&items, true, false, |_| false),
        chip_window(&items, false, false, |i| i.pos == 2),
    ] {
        assert_eq!(positions(&w), [0, 1, 2]);
        assert_eq!(w.folded, 0);
    }
}

#[test]
fn an_opened_fresh_chip_does_not_reveal_repeats() {
    assert_eq!(
        chip_window(&fold_items(), false, false, |i| i.pos == 0).folded,
        2
    );
}

fn window_of(flags: &str, show_all: bool) -> (usize, usize, Vec<usize>) {
    let run: Vec<RunItem> = flags
        .chars()
        .enumerate()
        .map(|(i, c)| item(i, c == 'R'))
        .collect();
    let w = chip_window(&run, false, show_all, |_| false);
    (w.folded, w.hidden, positions(&w))
}

#[test]
fn one_click_on_unchanged_shows_every_chip() {
    assert_eq!(window_of("FRFFFFF", false), (1, 0, vec![0, 2, 3, 4, 5, 6]));
    assert_eq!(
        window_of("FRFFFFF", true),
        (0, 0, vec![0, 1, 2, 3, 4, 5, 6])
    );
}

#[test]
fn a_mostly_folded_run_needs_one_click() {
    assert_eq!(window_of("FFFRRRRRR", true), (0, 0, (0..9).collect()));
}

// history-jump.test.ts, "chat plan": three turns; turn 2 holds a compaction
// and the spawning call.

const TURNS: [usize; 7] = [1, 1, 2, 2, 2, 3, 3];

fn jump_events() -> Vec<TimelineEvent> {
    use EventKind as K;
    let e = |k: K, request: &str, tool: Option<&str>| TimelineEvent {
        request_id: Some(request.into()),
        tool_use_id: tool.map(Into::into),
        tool_name: tool.map(|_| "Task".into()),
        ..ev(k)
    };
    vec![
        e(K::UserPrompt, "r1", None),
        e(K::AssistantText, "r1", None),
        e(K::Compaction, "r2", None),
        e(K::ToolCall, "r2", Some("spawn")),
        e(K::ToolResult, "r2", Some("spawn")),
        e(K::UserPrompt, "r3", None),
        e(K::ToolCall, "r3", Some("late")),
    ]
}

fn jump_plan(events: &[TimelineEvent], rows: &[usize], cross: Option<usize>) -> Vec<ChatNode> {
    build_chat_plan(events, rows, &TURNS, cross)
}

fn without_turn(n: usize) -> Vec<usize> {
    (0..TURNS.len()).filter(|&p| TURNS[p] != n).collect()
}

#[test]
fn a_turn_lands_on_its_rule() {
    let events = jump_events();
    let p = jump_plan(&events, &all(&events), None);
    let i = plan_index_of_turn(&p, 2).unwrap_or(usize::MAX);
    assert_eq!(p.get(i), Some(&ChatNode::Turn(2)));
}

#[test]
fn a_turn_that_crosses_into_the_dumb_zone_lands_on_the_divider() {
    let events = jump_events();
    let p = jump_plan(&events, &all(&events), Some(2));
    let i = plan_index_of_turn(&p, 2).unwrap_or(usize::MAX);
    assert_eq!(p.get(i), Some(&ChatNode::DumbZone));
    assert_eq!(p.get(i + 1), Some(&ChatNode::Turn(2)));
}

#[test]
fn a_filtered_away_turn_falls_through_to_the_next_shown_one() {
    let events = jump_events();
    let p = jump_plan(&events, &without_turn(2), None);
    let i = plan_index_of_turn(&p, 2).unwrap_or(usize::MAX);
    assert_eq!(p.get(i), Some(&ChatNode::Turn(3)));
    let p = jump_plan(&events, &without_turn(3), None);
    assert_eq!(plan_index_of_turn(&p, 3), None);
}

#[test]
fn an_interleaved_turn_lands_on_its_first_rule() {
    let events = jump_events();
    let p = jump_plan(&events, &[0, 5, 1], None);
    assert_eq!(p.iter().filter(|n| **n == ChatNode::Turn(1)).count(), 2);
    assert_eq!(plan_index_of_turn(&p, 1), Some(0));
}

#[test]
fn compaction_and_call_targets_find_their_nodes() {
    let events = jump_events();
    let p = jump_plan(&events, &all(&events), None);
    let c = plan_index_of_compaction(&events, &p).unwrap_or(usize::MAX);
    assert!(matches!(p.get(c), Some(ChatNode::Band(_))));
    let call = plan_index_of_call(&events, &p, "spawn").unwrap_or(usize::MAX);
    assert!(matches!(p.get(call), Some(ChatNode::Tool { .. })));
    let rows: Vec<usize> = all(&events)
        .into_iter()
        .filter(|&p| events[p].kind != EventKind::Compaction)
        .collect();
    let p = jump_plan(&events, &rows, None);
    assert_eq!(plan_index_of_compaction(&events, &p), None);
}
