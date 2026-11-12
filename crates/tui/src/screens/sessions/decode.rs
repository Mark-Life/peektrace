//! Pure decode helpers for the history view: what an expanded event body
//! shows, which events are listed, and each event's turn number.

use std::collections::HashMap;

use peektrace_core::highlight::Lang;
use peektrace_core::js::text;
use peektrace_core::js::value::JsValue;
use peektrace_core::sessions::labels::thinking_note;
use peektrace_core::sessions::model::{AnalyzedSession, EventKind, TimelineEvent};

/// An expanded body: language (`None` is plain), content, and a note shown
/// instead when the body is empty thinking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub lang: Option<Lang>,
    pub content: String,
    pub note: Option<String>,
}

fn decoded(lang: Option<Lang>, content: &str) -> Decoded {
    Decoded {
        lang,
        content: content.to_owned(),
        note: None,
    }
}

/// The `{type:"text", text}` blocks of a tool-result array, blank-line joined.
fn text_blocks(v: &JsValue) -> Option<String> {
    let texts: Vec<&str> = v
        .as_arr()?
        .iter()
        .filter_map(|b| {
            let o = b.as_obj()?;
            (o.get("type")?.as_str()? == "text").then_some(())?;
            o.get("text")?.as_str()
        })
        .collect();
    (!texts.is_empty()).then(|| texts.join("\n\n"))
}

/// Unwrap JSON tool payloads (`code`, `command`, text blocks) so escaped
/// newlines read as lines; prose stays plain.
pub fn decode_body(e: &TimelineEvent) -> Decoded {
    match e.kind {
        EventKind::ToolCall => {
            let input = JsValue::parse(&e.body);
            let obj = input.as_ref().and_then(JsValue::as_obj);
            if let Some(code) = obj.and_then(|o| o.get("code")).and_then(JsValue::as_str) {
                return decoded(Some(Lang::Typescript), code);
            }
            if let Some(cmd) = obj.and_then(|o| o.get("command")).and_then(JsValue::as_str) {
                return decoded(Some(Lang::Bash), cmd);
            }
            decoded(Some(Lang::Json), &e.body)
        }
        EventKind::ToolResult => match JsValue::parse(&e.body) {
            None => decoded(None, &e.body),
            Some(v) => text_blocks(&v).map_or_else(
                || decoded(Some(Lang::Json), &v.stringify(true)),
                |t| decoded(Some(Lang::Markdown), &t),
            ),
        },
        EventKind::SystemPrompt => decoded(Some(Lang::Markdown), &e.body),
        EventKind::AssistantThinking if text::trim(&e.body).is_empty() => Decoded {
            lang: None,
            content: String::new(),
            note: Some(thinking_note(e)),
        },
        _ => decoded(None, &e.body),
    }
}

/// Positions in `s.events` of every event but `system` ones.
pub fn visible_events(s: &AnalyzedSession) -> Vec<usize> {
    s.parsed
        .events
        .iter()
        .enumerate()
        .filter(|(_, e)| e.kind != EventKind::System)
        .map(|(i, _)| i)
        .collect()
}

/// Each event `index` to its 1-based turn: an event with no `requestId`
/// carries the turn of the model call before it.
pub fn turn_tags(s: &AnalyzedSession) -> HashMap<usize, usize> {
    let mut by_req: HashMap<&str, usize> = HashMap::new();
    for (i, t) in s.parsed.turns.iter().enumerate() {
        by_req.insert(t.request_id.as_str(), i + 1);
    }
    let mut out = HashMap::new();
    let mut cur = 0;
    for e in &s.parsed.events {
        if let Some(turn) = e.request_id.as_deref().and_then(|r| by_req.get(r)) {
            cur = *turn;
        }
        out.insert(e.index, cur);
    }
    out
}
