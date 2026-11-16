//! A Pi (pi.dev) session as a [`ParsedSession`]. Rows are tree entries
//! (`session`, `message`, `model_change`, `compaction`, `context_edit`...);
//! conversation rows carry `message.role`. Usage is `message.usage` as
//! logged; body sizes are chars/4.

use std::collections::HashMap;

use super::branch::active_branch;
use super::context::{
    Base, LINE_CAP, bash_event, branch_summary_event, compaction_event, custom_event, string_or,
    system_event, tool_result_text,
};
use super::edits::{PiEdit, collect_edits, fold_with_edit, is_editable};
use super::header::pi_tokens;
use crate::js::text;
use crate::js::units;
use crate::js::value::{EMPTY_OBJECT, JsValue, defined, str_of};
use crate::sessions::images::{as_image, image_marker};
use crate::sessions::jsonl::parse_rows;
use crate::sessions::measure::measure_tokens;
use crate::sessions::model::{
    EventKind, EventShape, LoadedCategory, ParsedSession, Provider, TimelineEvent, Turn,
};
use crate::sessions::model_windows::window_for_model;

/// What rows fold into; context edits rerun rows against it.
#[derive(Default)]
pub struct State {
    pub events: Vec<TimelineEvent>,
    pub turns: Vec<Turn>,
    by_id: HashMap<String, usize>,
    models: Vec<String>,
    compaction_indexes: Vec<usize>,
}

impl State {
    fn add_model(&mut self, model: &str) {
        if !self.models.iter().any(|m| m == model) {
            self.models.push(model.to_owned());
        }
    }

    /// Register the turn for an assistant row: the first row per id wins,
    /// and only when its prompt occupies context.
    fn register_turn(&mut self, request_id: &str, model: &str, ts: Option<&str>, usage: &JsValue) {
        let tokens = pi_tokens(usage);
        if self.by_id.contains_key(request_id) || tokens.context <= 0.0 {
            return;
        }
        self.by_id.insert(request_id.to_owned(), self.turns.len());
        self.turns.push(Turn::new(
            request_id.to_owned(),
            model.to_owned(),
            tokens,
            ts.map(str::to_owned),
        ));
    }

    fn push(&mut self, e: Option<TimelineEvent>) {
        self.events.extend(e);
    }

    fn compaction(&mut self, base: Base<'_>, summary: Option<&JsValue>) {
        self.compaction_indexes.push(base.index);
        self.events.push(compaction_event(base, summary));
    }
}

/// Session facts gathered while scanning.
#[derive(Default)]
struct Meta {
    cwd: Option<String>,
    ended_at: Option<String>,
    last_model: Option<String>,
    session_id: Option<String>,
    started_at: Option<String>,
    /// A system message was shown already; later ones patch it.
    system_seen: bool,
    title: Option<String>,
    version: Option<String>,
}

impl Meta {
    fn system(&mut self, state: &mut State, msg: &JsValue, base: Base<'_>) {
        if let Some(e) = system_event(msg, base, !self.system_seen) {
            self.system_seen = true;
            state.events.push(e);
        }
    }
}

fn str_field<'a>(v: &'a JsValue, key: &str) -> Option<&'a str> {
    v.get(key).and_then(JsValue::as_str)
}

/// `row.message ?? {}`.
fn message_of(row: &JsValue) -> &JsValue {
    defined(row.get("message")).unwrap_or(&EMPTY_OBJECT)
}

fn prompt_event(text: String, base: Base<'_>) -> TimelineEvent {
    base.event(
        EventShape::PiPrompt,
        EventKind::UserPrompt,
        "User message".to_owned(),
        units::first_line(&text, LINE_CAP),
        text,
    )
}

fn user_block_event(block: &JsValue, base: Base<'_>) -> Option<TimelineEvent> {
    if let Some(img) = as_image(block) {
        let marker = image_marker(img);
        let mut e = base.event(
            EventShape::PiAttachment,
            EventKind::Attachment,
            "image".to_owned(),
            marker.clone(),
            marker,
        );
        e.attachment_type = Some("image".to_owned());
        e.loaded_category = Some(LoadedCategory::File);
        return Some(e);
    }
    (str_field(block, "type") == Some("text"))
        .then(|| prompt_event(str_of(block.get("text")), base))
}

/// A string content is one text block.
fn handle_user(state: &mut State, msg: &JsValue, base: Base<'_>) {
    match msg.get("content") {
        Some(JsValue::Str(s)) => state.events.push(prompt_event(s.clone(), base)),
        Some(JsValue::Arr(blocks)) => {
            for block in blocks {
                state.push(user_block_event(block, base));
            }
        }
        _ => {}
    }
}

fn assistant_block_event(
    block: &JsValue,
    base: Base<'_>,
    request_id: &str,
) -> Option<TimelineEvent> {
    let (kind, title, body, preview) = match str_field(block, "type")? {
        "thinking" => {
            let text = str_of(block.get("thinking"));
            let preview = if text.is_empty() {
                "(content not stored in transcript)".to_owned()
            } else {
                units::first_line(&text, LINE_CAP)
            };
            (
                EventKind::AssistantThinking,
                "Thinking".to_owned(),
                text,
                preview,
            )
        }
        "text" => {
            let text = str_of(block.get("text"));
            let preview = units::first_line(&text, LINE_CAP);
            (
                EventKind::AssistantText,
                "Assistant".to_owned(),
                text,
                preview,
            )
        }
        "toolCall" => {
            let args = defined(block.get("arguments"))
                .map_or_else(|| "{}".to_owned(), |a| a.stringify(true));
            let preview = units::first_line(&units::collapse_ws(&args), LINE_CAP);
            let name = string_or(block.get("name"), "tool");
            let mut e = base.event(
                EventShape::PiAssistantBlock,
                EventKind::ToolCall,
                name.clone(),
                preview,
                args,
            );
            e.request_id = Some(request_id.to_owned());
            e.tool_name = Some(name);
            e.tool_use_id = str_field(block, "id").map(str::to_owned);
            return Some(e);
        }
        _ => return None,
    };
    let mut e = base.event(EventShape::PiAssistantBlock, kind, title, preview, body);
    e.request_id = Some(request_id.to_owned());
    Some(e)
}

/// Register the turn of an assistant row and, on the active branch, add
/// its block events linked to that turn.
fn handle_assistant(
    state: &mut State,
    row: &JsValue,
    msg: &JsValue,
    base: Base<'_>,
    off_branch: bool,
) {
    let model = string_or(msg.get("model"), "unknown");
    state.add_model(&model);
    let request_id =
        str_field(row, "id").map_or_else(|| format!("line-{}", base.index), str::to_owned);
    let usage = defined(msg.get("usage")).unwrap_or(&EMPTY_OBJECT);
    state.register_turn(&request_id, &model, base.ts, usage);
    if off_branch {
        return;
    }
    let turn = state.by_id.get(&request_id).copied();
    for block in msg
        .get("content")
        .and_then(JsValue::as_arr)
        .unwrap_or_default()
    {
        if let Some(e) = assistant_block_event(block, base, &request_id) {
            state.events.push(e);
            if let Some(t) = turn {
                let pos = state.events.len() - 1;
                state.turns[t].link(pos);
            }
        }
    }
}

fn handle_tool_result(state: &mut State, msg: &JsValue, base: Base<'_>) {
    let text = tool_result_text(msg.get("content"));
    let is_error = msg.get("isError").is_some_and(JsValue::is_true);
    let title = if is_error {
        "tool_result (error)"
    } else {
        "tool_result"
    };
    let preview = units::first_line(&text, LINE_CAP);
    let mut e = base.event(
        EventShape::PiToolResult,
        EventKind::ToolResult,
        title.to_owned(),
        preview,
        text,
    );
    e.is_error = Some(is_error);
    e.tool_name = str_field(msg, "toolName").map(str::to_owned);
    e.tool_use_id = str_field(msg, "toolCallId").map(str::to_owned);
    state.events.push(e);
}

/// One `message` row on the active branch.
fn handle_message(state: &mut State, meta: &mut Meta, row: &JsValue, base: Base<'_>) {
    let msg = message_of(row);
    match string_or(msg.get("role"), "").as_str() {
        "user" => handle_user(state, msg, base),
        "assistant" => {
            if let Some(m) = str_field(msg, "model") {
                meta.last_model = Some(m.to_owned());
            }
            handle_assistant(state, row, msg, base, false);
        }
        "toolResult" => handle_tool_result(state, msg, base),
        "system" => meta.system(state, msg, base),
        "custom" => state.push(custom_event(base, msg)),
        "bashExecution" => state.push(bash_event(base, msg)),
        "branchSummary" => state
            .events
            .push(branch_summary_event(base, msg.get("summary"))),
        "compactionSummary" => state.compaction(base, msg.get("summary")),
        _ => {}
    }
}

/// A top-level context entry: `compaction`, `branch_summary` or
/// `custom_message`.
fn handle_context_entry(
    state: &mut State,
    meta: &mut Meta,
    ty: &str,
    row: &JsValue,
    base: Base<'_>,
) {
    match ty {
        "compaction" => {
            // The new system message applies after the compaction.
            state.compaction(base, row.get("summary"));
            if let Some(sys) = row
                .get("systemMessage")
                .filter(|s| matches!(s, JsValue::Obj(_) | JsValue::Arr(_)))
            {
                meta.system(state, sys, base);
            }
        }
        "branch_summary" => state
            .events
            .push(branch_summary_event(base, row.get("summary"))),
        "custom_message" => state.push(custom_event(base, row)),
        _ => {}
    }
}

fn apply_session_row(meta: &mut Meta, row: &JsValue) {
    if let Some(id) = str_field(row, "id") {
        meta.session_id = Some(id.to_owned());
    }
    if let Some(cwd) = str_field(row, "cwd") {
        meta.cwd = Some(cwd.to_owned());
    }
    if let Some(v) = defined(row.get("version")) {
        meta.version = Some(string_or(Some(v), ""));
    }
}

/// Parse a Pi session. `session_id` is used when no `session` row names one.
/// `null` rows are skipped.
pub fn parse_pi_session(text_in: &str, path: &str, session_id: &str) -> ParsedSession {
    let rows = parse_rows(text_in);
    let live = active_branch(&rows);
    let on_branch = |i: usize| live.as_ref().is_none_or(|l| l.contains(&i));
    let edits = collect_edits(&rows, on_branch);
    let edit_for = |row: &JsValue| -> Option<&[PiEdit]> {
        let id = str_field(row, "id")?;
        is_editable(row)
            .then(|| edits.get(id).map(Vec::as_slice))
            .flatten()
    };
    let mut state = State::default();
    let mut meta = Meta::default();
    for (index, row) in rows.iter().enumerate() {
        if row.as_obj().is_none() {
            continue;
        }
        let ty = string_or(row.get("type"), "");
        let ts = str_field(row, "timestamp");
        if let Some(ts) = ts.filter(|t| !t.is_empty()) {
            meta.started_at.get_or_insert_with(|| ts.to_owned());
            meta.ended_at = Some(ts.to_owned());
        }
        let base = Base { index, ts };
        match ty.as_str() {
            "session" => apply_session_row(&mut meta, row),
            "session_info" => {
                if let Some(name) = str_field(row, "name").filter(|n| !text::trim(n).is_empty()) {
                    meta.title = Some(name.to_owned());
                }
            }
            _ if !on_branch(index) => {
                let msg = message_of(row);
                if str_field(row, "type") == Some("message")
                    && str_field(msg, "role") == Some("assistant")
                {
                    handle_assistant(&mut state, row, msg, base, true);
                }
            }
            "model_change" => {
                if let Some(m) = str_field(row, "modelId") {
                    state.add_model(m);
                    meta.last_model = Some(m.to_owned());
                }
            }
            "message" => fold_with_edit(&mut state, row, edit_for(row), &mut |r, s| {
                handle_message(s, &mut meta, r, base);
            }),
            _ => fold_with_edit(&mut state, row, edit_for(row), &mut |r, s| {
                handle_context_entry(s, &mut meta, &ty, r, base);
            }),
        }
    }
    let guess = window_for_model(meta.last_model.as_deref());
    // A call larger than the guessed window proves the guess wrong.
    let window = guess.filter(|&g| state.turns.iter().all(|t| t.context_tokens <= g));
    let mut parsed = ParsedSession {
        provider: Provider::Pi,
        session_id: meta.session_id.unwrap_or_else(|| session_id.to_owned()),
        path: path.to_owned(),
        models: state.models,
        events: state.events,
        turns: state.turns,
        compaction_indexes: state.compaction_indexes,
        subagents: Vec::new(),
        cwd: meta.cwd,
        git_branch: None,
        title: meta.title,
        version: meta.version,
        started_at: meta.started_at,
        ended_at: meta.ended_at,
        native_context_window: window,
        unknown_record_types: None,
        unknown_system_subtypes: None,
        sidechain_cost: None,
        native_cost_usd: None,
    };
    measure_tokens(&mut parsed);
    parsed
}
