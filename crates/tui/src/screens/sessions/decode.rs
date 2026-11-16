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

#[cfg(test)]
mod tests {
    use peektrace_core::sessions::model::{EventShape, ParsedSession, Turn, TurnTokens};

    use super::*;

    fn evt(index: usize, kind: EventKind, body: &str) -> TimelineEvent {
        let mut e = TimelineEvent::new(EventShape::Prompt, index, kind);
        e.body = body.to_owned();
        e
    }

    fn req(mut e: TimelineEvent, id: &str) -> TimelineEvent {
        e.request_id = Some(id.to_owned());
        e
    }

    fn session(events: Vec<TimelineEvent>, turns: &[&str]) -> AnalyzedSession {
        AnalyzedSession {
            parsed: ParsedSession {
                events,
                turns: turns
                    .iter()
                    .map(|r| Turn::new((*r).to_owned(), String::new(), TurnTokens::default(), None))
                    .collect(),
                ..ParsedSession::default()
            },
            ..AnalyzedSession::default()
        }
    }

    fn plain(content: &str) -> Decoded {
        decoded(None, content)
    }

    #[test]
    fn tool_call_code_is_typescript() {
        let d = decode_body(&evt(0, EventKind::ToolCall, r#"{"code":"const x = 1"}"#));
        assert_eq!(d, decoded(Some(Lang::Typescript), "const x = 1"));
    }

    #[test]
    fn tool_call_command_is_bash() {
        let d = decode_body(&evt(0, EventKind::ToolCall, r#"{"command":"ls -la"}"#));
        assert_eq!(d, decoded(Some(Lang::Bash), "ls -la"));
    }

    #[test]
    fn tool_call_without_code_or_command_echoes_json() {
        let body = "{\n  \"pattern\": \"foo\"\n}";
        let d = decode_body(&evt(0, EventKind::ToolCall, body));
        assert_eq!(d, decoded(Some(Lang::Json), body));
    }

    #[test]
    fn tool_result_text_blocks_join_as_markdown() {
        let body = r#"[{"type":"text","text":"line one"},{"type":"image"},{"type":"text","text":"line two"}]"#;
        let d = decode_body(&evt(0, EventKind::ToolResult, body));
        assert_eq!(d, decoded(Some(Lang::Markdown), "line one\n\nline two"));
    }

    #[test]
    fn tool_result_json_object_is_pretty_json() {
        let d = decode_body(&evt(0, EventKind::ToolResult, r#"{"ok":true}"#));
        assert_eq!(d, decoded(Some(Lang::Json), "{\n  \"ok\": true\n}"));
    }

    #[test]
    fn tool_result_non_json_is_plain() {
        let d = decode_body(&evt(0, EventKind::ToolResult, "not json {"));
        assert_eq!(d, plain("not json {"));
    }

    #[test]
    fn empty_thinking_shows_the_note() {
        let e = evt(0, EventKind::AssistantThinking, "  ");
        assert_eq!(
            decode_body(&e),
            Decoded {
                lang: None,
                content: String::new(),
                note: Some(thinking_note(&e)),
            }
        );
    }

    #[test]
    fn empty_thinking_with_measured_tokens_names_the_count() {
        let mut e = evt(0, EventKind::AssistantThinking, "");
        e.tokens_measured = Some(41.0);
        let note = decode_body(&e).note.expect("note");
        assert!(note.contains("41 thinking tokens"), "{note}");
    }

    #[test]
    fn non_empty_thinking_is_plain() {
        let d = decode_body(&evt(0, EventKind::AssistantThinking, "reasoning"));
        assert_eq!(d, plain("reasoning"));
    }

    #[test]
    fn assistant_text_is_plain() {
        let d = decode_body(&evt(0, EventKind::AssistantText, "# hi"));
        assert_eq!(d, plain("# hi"));
    }

    #[test]
    fn system_prompt_is_markdown() {
        let d = decode_body(&evt(0, EventKind::SystemPrompt, "## rules"));
        assert_eq!(d, decoded(Some(Lang::Markdown), "## rules"));
    }

    #[test]
    fn visible_events_drop_system_events() {
        let s = session(
            vec![
                evt(0, EventKind::UserPrompt, ""),
                evt(1, EventKind::System, ""),
                evt(2, EventKind::AssistantText, ""),
            ],
            &[],
        );
        let idx: Vec<usize> = visible_events(&s)
            .into_iter()
            .map(|p| s.parsed.events[p].index)
            .collect();
        assert_eq!(idx, [0, 2]);
    }

    #[test]
    fn visible_events_keep_the_system_prompt() {
        let s = session(vec![evt(0, EventKind::SystemPrompt, "")], &[]);
        let kinds: Vec<EventKind> = visible_events(&s)
            .into_iter()
            .map(|p| s.parsed.events[p].kind)
            .collect();
        assert_eq!(kinds, [EventKind::SystemPrompt]);
    }

    #[test]
    fn tool_results_inherit_the_preceding_turn() {
        let s = session(
            vec![
                evt(0, EventKind::UserPrompt, ""),
                req(evt(1, EventKind::AssistantText, ""), "r1"),
                evt(2, EventKind::ToolResult, ""),
                req(evt(3, EventKind::AssistantText, ""), "r2"),
                evt(4, EventKind::ToolResult, ""),
            ],
            &["r1", "r2"],
        );
        let tags = turn_tags(&s);
        let got: Vec<usize> = (0..5).map(|i| tags[&i]).collect();
        assert_eq!(got, [0, 1, 1, 2, 2]);
    }
}
