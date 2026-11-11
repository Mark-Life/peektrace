//! A Claude Code JSONL transcript as a [`ParsedSession`]. Token usage is
//! taken from assistant `usage` as logged; body sizes are chars/4.

use std::collections::HashMap;

use super::attachments::{AttachmentBase, PromptSeen, attachment_event};
use super::blocks::{AssistantBase, UserBase, assistant_block_event, user_block_event};
use super::branches::dead_branch_rows;
use super::records::{
    KNOWN_IGNORED_TYPES, KNOWN_SYSTEM_SUBTYPES, NewTurn, SYNTHETIC_MODEL, bump, compact_meta_of,
    has_one_million_key, merge_streamed_usage, new_turn,
};
use crate::js::units;
use crate::js::value::{JsValue, defined, js_key_order, js_string, str_of};
use crate::sessions::cost::turns_cost;
use crate::sessions::jsonl::parse_rows;
use crate::sessions::measure::measure_tokens;
use crate::sessions::model::{EventKind, EventShape, ParsedSession, Provider, TimelineEvent, Turn};
use crate::sessions::prompt::{PromptArgs, prompt_event};

const ONE_MILLION: f64 = 1_000_000.0;

/// What a parse reads besides the text.
#[derive(Debug, Clone, Default)]
pub struct ParseArgs<'a> {
    pub text: &'a str,
    pub path: &'a str,
    pub session_id: &'a str,
    /// Count `isSidechain` turns toward the curve: on only for a subagent's
    /// own file.
    pub include_sidechain_turns: bool,
    /// Leave out `prompt_snapshot` system-prompt events.
    pub skip_system_prompt: bool,
}

/// Turns keyed by request id, in first-seen order.
#[derive(Default)]
struct TurnMap {
    turns: Vec<Turn>,
    by_id: HashMap<String, usize>,
}

impl TurnMap {
    fn get_mut(&mut self, id: &str) -> Option<&mut Turn> {
        self.by_id.get(id).map(|&i| &mut self.turns[i])
    }

    fn insert(&mut self, t: Turn) {
        self.by_id.insert(t.request_id.clone(), self.turns.len());
        self.turns.push(t);
    }
}

#[derive(Default)]
struct State {
    events: Vec<TimelineEvent>,
    turns: TurnMap,
    /// Inline `isSidechain` turns left out of `turns`; priced apart.
    sidechain: TurnMap,
    compaction_indexes: Vec<usize>,
    models: Vec<String>,
    one_million: bool,
    /// Token counts of the last `compact_boundary`, for the next summary.
    pending_compact: Option<(Option<f64>, Option<f64>)>,
    /// Prompt parts already shown, per thread (main or one subagent).
    prompt_seen: HashMap<String, PromptSeen>,
    unknown_types: Vec<(String, f64)>,
    unknown_subtypes: Vec<(String, f64)>,
}

#[derive(Default)]
struct Meta {
    cwd: Option<String>,
    ended_at: Option<String>,
    git_branch: Option<String>,
    started_at: Option<String>,
    title: Option<String>,
    version: Option<String>,
}

/// One row being handled.
struct Row<'a> {
    o: &'a JsValue,
    index: usize,
    ts: Option<&'a str>,
    is_sidechain: bool,
}

fn str_field<'a>(o: &'a JsValue, key: &str) -> Option<&'a str> {
    o.get(key).and_then(JsValue::as_str)
}

fn apply_meta(meta: &mut Meta, o: &JsValue, ts: Option<&str>) {
    if let Some(ts) = ts.filter(|t| !t.is_empty()) {
        meta.started_at.get_or_insert_with(|| ts.to_owned());
        meta.ended_at = Some(ts.to_owned());
    }
    if let Some(cwd) = str_field(o, "cwd")
        && meta.cwd.as_deref().is_none_or(str::is_empty)
    {
        meta.cwd = Some(cwd.to_owned());
    }
    if let Some(b) = str_field(o, "gitBranch") {
        meta.git_branch = Some(b.to_owned());
    }
    if let Some(v) = str_field(o, "version") {
        meta.version = Some(v.to_owned());
    }
}

fn handle_user(row: &Row<'_>, state: &mut State) {
    let content = row.o.get("message").and_then(|m| m.get("content"));
    let is_compact = row.o.get("isCompactSummary").is_some_and(JsValue::is_true);
    let compact_tokens = if is_compact {
        state.compaction_indexes.push(row.index);
        state.pending_compact.take()
    } else {
        None
    };
    match content {
        Some(JsValue::Str(text)) if is_compact => {
            let mut e =
                TimelineEvent::new(EventShape::Compaction, row.index, EventKind::Compaction).text(
                    "Context compaction (summary)".to_owned(),
                    units::first_line(text, 200),
                    text.clone(),
                    units::est_tokens(text),
                );
            e.is_sidechain = Some(row.is_sidechain);
            e.ts = row.ts.map(str::to_owned);
            e.is_meta = row
                .o
                .get("isMeta")
                .is_some_and(JsValue::is_true)
                .then_some(true);
            if let Some((pre, post)) = compact_tokens {
                e.compact_pre_tokens = pre;
                e.compact_post_tokens = post;
            }
            state.events.push(e);
        }
        Some(JsValue::Str(text)) => state.events.push(prompt_event(PromptArgs {
            o: row.o,
            index: row.index,
            ts: row.ts,
            is_sidechain: row.is_sidechain,
            text: text.clone(),
            title: "User prompt",
            tokens_est: None,
        })),
        Some(JsValue::Arr(blocks)) => {
            let base = UserBase {
                row: row.o,
                index: row.index,
                ts: row.ts,
                is_sidechain: row.is_sidechain,
            };
            state.events.extend(
                blocks
                    .iter()
                    .enumerate()
                    .filter_map(|(i, b)| user_block_event(&base, b, i)),
            );
        }
        _ => {}
    }
}

/// Add one assistant block's event, if any, then point the turn at the last
/// event, as TS does even when the block made none.
fn push_block(state: &mut State, base: &AssistantBase<'_>, block: Option<&JsValue>) {
    if let Some(e) = block.and_then(|b| assistant_block_event(base, b)) {
        state.events.push(e);
    }
    let last = i64::try_from(state.events.len()).unwrap_or(i64::MAX) - 1;
    if let Some(turn) = state.turns.get_mut(base.request_id) {
        turn.event_indexes.push(last);
    }
}

/// Register an assistant line's turn and, on a live branch, add its block
/// events. A row on a rewound branch keeps its billed turn only.
fn handle_assistant(
    row: &Row<'_>,
    state: &mut State,
    include_sidechain_turns: bool,
    off_branch: bool,
) {
    let msg = defined(row.o.get("message"));
    let model = js_string(Some(
        msg.and_then(|m| defined(m.get("model")))
            .unwrap_or(&JsValue::Str("unknown".to_owned())),
    ));
    if model != SYNTHETIC_MODEL && !state.models.contains(&model) {
        state.models.push(model.clone());
    }
    let request_id =
        str_field(row.o, "requestId").map_or_else(|| format!("line-{}", row.index), str::to_owned);
    let usage = msg.and_then(|m| defined(m.get("usage")));
    let countable = include_sidechain_turns || !row.is_sidechain;
    let map = if countable {
        &mut state.turns
    } else {
        &mut state.sidechain
    };
    if let Some(turn) = map.get_mut(&request_id) {
        merge_streamed_usage(turn, usage);
    } else if let Some(t) = new_turn(&NewTurn {
        request_id: &request_id,
        model: &model,
        ts: row.ts,
        usage,
        effort: str_field(row.o, "effort"),
    }) {
        map.insert(t);
    }
    if off_branch {
        return;
    }
    let base = AssistantBase {
        index: row.index,
        request_id: &request_id,
        is_sidechain: row.is_sidechain,
        ts: row.ts,
        thinking_duration_ms: row.o.get("thinkingDurationMs").and_then(JsValue::as_num),
    };
    // `for (const block of content ?? [])`: array items, or one block per
    // code point of a string, which yields no event.
    match msg.and_then(|m| defined(m.get("content"))) {
        Some(JsValue::Arr(blocks)) => blocks
            .iter()
            .for_each(|b| push_block(state, &base, Some(b))),
        Some(JsValue::Str(s)) => s.chars().for_each(|_| push_block(state, &base, None)),
        _ => {}
    }
}

fn handle_summary(row: &Row<'_>, state: &mut State) {
    let text = str_of(row.o.get("summary"));
    let mut e = TimelineEvent::new(EventShape::Summary, row.index, EventKind::Summary).text(
        "Rolling summary".to_owned(),
        units::first_line(&text, 200),
        text.clone(),
        units::est_tokens(&text),
    );
    e.ts = row.ts.map(str::to_owned);
    state.events.push(e);
}

fn handle_attachment(row: &Row<'_>, state: &mut State) {
    let thread = if row.is_sidechain {
        str_field(row.o, "agentId").unwrap_or("sidechain")
    } else {
        "main"
    };
    let seen = state.prompt_seen.entry(thread.to_owned()).or_default();
    let base = AttachmentBase {
        index: row.index,
        is_sidechain: row.is_sidechain,
        ts: row.ts,
    };
    if let Some(e) = attachment_event(row.o, &base, seen) {
        state.events.push(e);
    }
}

fn handle_system(row: &Row<'_>, state: &mut State) {
    let o = row.o;
    let subtype = o.get("subtype");
    let text = str_of(Some(
        defined(o.get("content"))
            .or_else(|| defined(subtype))
            .unwrap_or(&JsValue::Str(String::new())),
    ));
    match subtype.and_then(JsValue::as_str) {
        Some("compact_boundary") => state.pending_compact = Some(compact_meta_of(o)),
        Some(s) if !KNOWN_SYSTEM_SUBTYPES.contains(&s) => bump(&mut state.unknown_subtypes, s),
        _ => {}
    }
    let title = format!(
        "system: {}",
        defined(subtype).map_or_else(|| "event".to_owned(), |s| js_string(Some(s)))
    );
    let mut e = TimelineEvent::new(EventShape::System, row.index, EventKind::System).text(
        title,
        units::first_line(&text, 200),
        text,
        0.0,
    );
    e.is_sidechain = Some(row.is_sidechain);
    e.ts = row.ts.map(str::to_owned);
    state.events.push(e);
}

/// Give thinking blocks the size usage reports for them, split by text
/// length (evenly when no text was stored).
fn measure_thinking(events: &mut [TimelineEvent], turns: &[Turn]) {
    let mut by_req: HashMap<&str, Vec<usize>> = HashMap::new();
    for (pos, e) in events.iter().enumerate() {
        if e.kind == EventKind::AssistantThinking
            && let Some(r) = e.request_id.as_deref().filter(|r| !r.is_empty())
        {
            by_req.entry(r).or_default().push(pos);
        }
    }
    let mut measured: Vec<(usize, f64)> = Vec::new();
    for turn in turns {
        let (Some(reported), Some(positions)) =
            (turn.thinking_tokens, by_req.get(turn.request_id.as_str()))
        else {
            continue;
        };
        let weights: Vec<f64> = positions.iter().map(|&p| events[p].tokens_est).collect();
        let total = weights.iter().fold(0.0, |a, b| a + b);
        #[expect(clippy::cast_precision_loss, reason = "JS numbers are f64")]
        let n = positions.len() as f64;
        let mut left = reported;
        for (i, &pos) in positions.iter().enumerate() {
            let share = if total > 0.0 {
                weights[i] / total
            } else {
                1.0 / n
            };
            let v = if i == positions.len() - 1 {
                left
            } else {
                crate::js::num::js_round(reported * share)
            };
            measured.push((pos, v));
            left -= v;
        }
    }
    for (pos, v) in measured {
        events[pos].tokens_measured = Some(v);
    }
}

/// Parse a Claude Code transcript.
pub fn parse_claude_session(args: &ParseArgs<'_>) -> ParsedSession {
    let rows = parse_rows(args.text);
    let dead = dead_branch_rows(&rows);
    let mut state = State::default();
    let mut meta = Meta::default();
    for (index, o) in rows.iter().enumerate() {
        let ty = str_field(o, "type").unwrap_or("");
        let row = Row {
            o,
            index,
            ts: str_field(o, "timestamp"),
            is_sidechain: o.get("isSidechain").is_some_and(JsValue::is_true),
        };
        apply_meta(&mut meta, o, row.ts);
        if dead.contains(&index) {
            if ty == "assistant" {
                handle_assistant(&row, &mut state, args.include_sidechain_turns, true);
            }
            continue;
        }
        match ty {
            "ai-title" => {
                if let Some(t) = str_field(o, "aiTitle") {
                    meta.title = Some(t.to_owned());
                }
            }
            "attachment" => {
                let snapshot = o
                    .get("attachment")
                    .and_then(|a| a.get("type"))
                    .and_then(JsValue::as_str)
                    == Some("prompt_snapshot");
                if !(args.skip_system_prompt && snapshot) {
                    handle_attachment(&row, &mut state);
                }
            }
            "user" => handle_user(&row, &mut state),
            "assistant" => handle_assistant(&row, &mut state, args.include_sidechain_turns, false),
            "summary" => handle_summary(&row, &mut state),
            "system" => handle_system(&row, &mut state),
            "cost-state" => state.one_million = state.one_million || has_one_million_key(o),
            "" => {}
            t if !KNOWN_IGNORED_TYPES.contains(&t) => bump(&mut state.unknown_types, t),
            _ => {}
        }
    }
    let turns = state.turns.turns;
    let sidechain = state.sidechain.turns;
    let mut events = state.events;
    measure_thinking(&mut events, &turns);
    let counts = |c: Vec<(String, f64)>| (!c.is_empty()).then(|| js_key_order(c));
    let mut parsed = ParsedSession {
        provider: Provider::ClaudeCode,
        session_id: args.session_id.to_owned(),
        path: args.path.to_owned(),
        models: state.models,
        events,
        turns,
        compaction_indexes: state.compaction_indexes,
        subagents: Vec::new(),
        cwd: meta.cwd,
        git_branch: meta.git_branch,
        title: meta.title,
        version: meta.version,
        started_at: meta.started_at,
        ended_at: meta.ended_at,
        native_context_window: state.one_million.then_some(ONE_MILLION),
        unknown_record_types: counts(state.unknown_types),
        unknown_system_subtypes: counts(state.unknown_subtypes),
        sidechain_cost: (!sidechain.is_empty()).then(|| turns_cost(&sidechain)),
        native_cost_usd: None,
    };
    measure_tokens(&mut parsed);
    parsed
}
