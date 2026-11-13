//! The transcript as a chat script: which side an event sits on, which call
//! owns which result, which neighbours fold into one chip run, and where the
//! turn rules and the dumb-zone band go. Nodes refer to events by their
//! position in `AnalyzedSession::events`.

use std::collections::{HashMap, HashSet};

use peektrace_protocol::view::{EventKind, TimelineEvent};

use super::labels::{event_badge_label, js_trim};
use super::utf16;

/// Body length (UTF-16 units) past which a bubble clamps behind a fade.
pub const CLAMP_CHARS: usize = 800;
/// Chips shown before the rest fold behind `+N more`.
pub const CHIP_LIMIT: usize = 6;
/// Attachment titles that carry a file name after their type prefix.
pub const FILE_TITLE_PREFIXES: [&str; 3] = ["file: ", "edited_text_file: ", "opened_file_in_ide: "];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    User,
    Agent,
    Center,
}

/// A chip of a context run; `repeat` when an earlier chip of the same
/// identity carried this exact body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RunItem {
    pub pos: usize,
    pub repeat: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ChatNode {
    Message(usize),
    Tool {
        call: Option<usize>,
        result: Option<usize>,
    },
    Run(Vec<RunItem>),
    Band(usize),
    Notice(usize),
    Turn(usize),
    DumbZone,
}

/// Which side of the conversation an event reads on.
pub fn lane_of(e: &TimelineEvent) -> Lane {
    match e.kind {
        EventKind::UserPrompt if e.sidechain() => Lane::Agent,
        EventKind::UserPrompt if e.is_user_authored() => Lane::User,
        EventKind::AssistantText
        | EventKind::AssistantThinking
        | EventKind::ToolCall
        | EventKind::ToolResult => Lane::Agent,
        _ => Lane::Center,
    }
}

/// The file name after a file attachment's type prefix.
pub fn file_title(title: &str) -> Option<&str> {
    FILE_TITLE_PREFIXES
        .iter()
        .find_map(|p| title.strip_prefix(p))
}

/// A chip's label: the file name, else the title, else the badge word.
pub fn chip_label(e: &TimelineEvent) -> String {
    if let Some(name) = file_title(&e.title) {
        return name.to_owned();
    }
    if js_trim(&e.title).is_empty() {
        event_badge_label(e)
    } else {
        e.title.clone()
    }
}

pub fn needs_clamp(e: &TimelineEvent) -> bool {
    utf16::len(&e.body) > CLAMP_CHARS
}

/// The collapse id of the node at event position `pos`.
pub fn collapse_id(pos: usize) -> String {
    format!("chat:{pos}")
}

/// The collapse id a node toggles; runs and separators have none.
pub fn collapse_id_of(n: &ChatNode) -> Option<String> {
    match n {
        ChatNode::Message(p) | ChatNode::Band(p) | ChatNode::Notice(p) => Some(collapse_id(*p)),
        ChatNode::Tool { call, result } => call.or(*result).map(collapse_id),
        _ => None,
    }
}

fn is_chip(e: &TimelineEvent) -> bool {
    matches!(
        e.kind,
        EventKind::Attachment | EventKind::Meta | EventKind::System
    )
}

fn chip_identity(e: &TimelineEvent) -> String {
    format!(
        "{}|{}|{}|{}",
        e.kind.as_str(),
        e.attachment_type.as_deref().unwrap_or_default(),
        chip_label(e),
        e.sidechain()
    )
}

/// The first `tool-result` after `from` answering `call`.
fn result_for(
    events: &[TimelineEvent],
    rows: &[usize],
    from: usize,
    call: &TimelineEvent,
) -> Option<usize> {
    let id = call.tool_use_id.as_deref().filter(|id| !id.is_empty())?;
    rows[from..].iter().copied().find(|&p| {
        events[p].kind == EventKind::ToolResult && events[p].tool_use_id.as_deref() == Some(id)
    })
}

fn node_for_row(
    events: &[TimelineEvent],
    rows: &[usize],
    i: usize,
    pos: usize,
) -> (Option<usize>, ChatNode) {
    let e = &events[pos];
    match e.kind {
        EventKind::ToolCall => {
            let result = result_for(events, rows, i + 1, e);
            (
                result,
                ChatNode::Tool {
                    call: Some(pos),
                    result,
                },
            )
        }
        EventKind::ToolResult => (
            None,
            ChatNode::Tool {
                call: None,
                result: Some(pos),
            },
        ),
        EventKind::Compaction | EventKind::Summary | EventKind::SystemPrompt => {
            (None, ChatNode::Band(pos))
        }
        _ if lane_of(e) == Lane::Center => (None, ChatNode::Notice(pos)),
        _ => (None, ChatNode::Message(pos)),
    }
}

/// One forward pass over the visible rows (event positions, in order).
pub fn build_chat_plan(
    events: &[TimelineEvent],
    rows: &[usize],
    turns: &[usize],
    cross: Option<usize>,
) -> Vec<ChatNode> {
    let mut nodes = Vec::new();
    let mut consumed = HashSet::new();
    let mut run: Vec<RunItem> = Vec::new();
    let mut last_body: HashMap<String, &str> = HashMap::new();
    let mut last_turn = 0;
    let flush = |nodes: &mut Vec<ChatNode>, run: &mut Vec<RunItem>| {
        if !run.is_empty() {
            nodes.push(ChatNode::Run(std::mem::take(run)));
        }
    };
    for (i, &pos) in rows.iter().enumerate() {
        if consumed.contains(&pos) {
            continue;
        }
        let e = &events[pos];
        if cross == Some(pos) {
            flush(&mut nodes, &mut run);
            nodes.push(ChatNode::DumbZone);
        }
        let turn = turns.get(pos).copied().unwrap_or(0);
        if turn > 0 && turn != last_turn {
            flush(&mut nodes, &mut run);
            nodes.push(ChatNode::Turn(turn));
            last_turn = turn;
        }
        if is_chip(e) {
            let identity = chip_identity(e);
            let image = e.attachment_type.as_deref() == Some("image") || e.images.is_some();
            let repeat = !image && last_body.get(&identity) == Some(&e.body.as_str());
            run.push(RunItem { pos, repeat });
            last_body.insert(identity, &e.body);
            continue;
        }
        flush(&mut nodes, &mut run);
        let (taken, node) = node_for_row(events, rows, i, pos);
        consumed.extend(taken);
        nodes.push(node);
    }
    flush(&mut nodes, &mut run);
    nodes
}

/// Every collapse id the chat renders: one per row, minus results folded
/// into their call's card.
pub fn chat_ids_of(events: &[TimelineEvent], rows: &[usize]) -> Vec<String> {
    let consumed: HashSet<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, p)| events[**p].kind == EventKind::ToolCall)
        .filter_map(|(i, &p)| result_for(events, rows, i + 1, &events[p]))
        .collect();
    rows.iter()
        .filter(|p| !consumed.contains(p))
        .map(|&p| collapse_id(p))
        .collect()
}

fn base_key(n: &ChatNode) -> String {
    match n {
        ChatNode::Message(p) | ChatNode::Band(p) | ChatNode::Notice(p) => format!("n{p}"),
        ChatNode::Tool { call, result } => call
            .or(*result)
            .map_or_else(|| "n?".to_owned(), |p| format!("n{p}")),
        ChatNode::Run(items) => items
            .first()
            .map_or_else(|| "run?".to_owned(), |i| format!("run{}", i.pos)),
        ChatNode::Turn(t) => format!("turn{t}"),
        ChatNode::DumbZone => "dumbzone".to_owned(),
    }
}

/// One unique key per node; a repeated base key gets `~<k>`.
pub fn chat_node_keys(plan: &[ChatNode]) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    plan.iter()
        .map(|n| {
            let base = base_key(n);
            let count = seen.entry(base.clone()).or_insert(0);
            *count += 1;
            if *count == 1 {
                base
            } else {
                format!("{base}~{}", *count - 1)
            }
        })
        .collect()
}

/// The node that starts turn `n` (its rule, or the dumb-zone band right
/// before it), else the first later turn.
pub fn plan_index_of_turn(plan: &[ChatNode], n: usize) -> Option<usize> {
    let exact = plan.iter().position(|x| *x == ChatNode::Turn(n));
    let at = exact.or_else(|| {
        plan.iter()
            .position(|x| matches!(x, ChatNode::Turn(t) if *t >= n))
    })?;
    Some(if at > 0 && plan[at - 1] == ChatNode::DumbZone {
        at - 1
    } else {
        at
    })
}

/// The band of the first compaction.
pub fn plan_index_of_compaction(events: &[TimelineEvent], plan: &[ChatNode]) -> Option<usize> {
    plan.iter()
        .position(|x| matches!(x, ChatNode::Band(p) if events[*p].kind == EventKind::Compaction))
}

/// The card showing tool call `id`.
pub fn plan_index_of_call(events: &[TimelineEvent], plan: &[ChatNode], id: &str) -> Option<usize> {
    let has = |p: Option<usize>| p.is_some_and(|p| events[p].tool_use_id.as_deref() == Some(id));
    plan.iter().position(|n| match n {
        ChatNode::Tool { call, result } => has(*call) || (call.is_none() && has(*result)),
        _ => false,
    })
}

/// A run's chips split at every opened one: closed neighbours stay chip rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Chips(Vec<RunItem>),
    Card(RunItem),
}

pub fn segments_of(items: &[RunItem], open: impl Fn(&RunItem) -> bool) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    for item in items {
        if open(item) {
            out.push(Segment::Card(*item));
        } else if let Some(Segment::Chips(chips)) = out.last_mut() {
            chips.push(*item);
        } else {
            out.push(Segment::Chips(vec![*item]));
        }
    }
    out
}

/// The chips on screen and how many fold away as repeats or past the limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChipWindow {
    pub shown: Vec<RunItem>,
    pub folded: usize,
    pub hidden: usize,
}

pub fn chip_window(
    items: &[RunItem],
    all_open: bool,
    show_all: bool,
    opened: impl Fn(&RunItem) -> bool,
) -> ChipWindow {
    let reveal = show_all || all_open || items.iter().any(|i| i.repeat && opened(i));
    let visible: Vec<RunItem> = items
        .iter()
        .copied()
        .filter(|i| reveal || !i.repeat)
        .collect();
    let folded = items.len() - visible.len();
    let last_opened = visible.iter().rposition(&opened);
    let expanded = show_all || all_open || last_opened.is_some_and(|i| i >= CHIP_LIMIT);
    let hidden = if expanded {
        0
    } else {
        visible.len().saturating_sub(CHIP_LIMIT)
    };
    let mut shown = visible;
    shown.truncate(shown.len() - hidden);
    ChipWindow {
        shown,
        folded,
        hidden,
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod port_tests;
