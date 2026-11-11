//! Events from the content blocks of Claude `user` and `assistant` lines,
//! server tool results (web search, web fetch, advisor) included.

use crate::js::units;
use crate::js::value::{JsObject, JsValue, defined, js_string, str_of};
use crate::sessions::images::{
    as_image, image_marker, image_ref, omit_image_data, tool_result_image_refs,
};
use crate::sessions::model::{EventKind, EventShape, LoadedCategory, TimelineEvent};
use crate::sessions::prompt::{PromptArgs, prompt_event};

const LINE_CAP: usize = 200;

fn first_line(s: &str) -> String {
    units::first_line(s, LINE_CAP)
}

fn str_field(o: &JsValue, key: &str) -> Option<String> {
    o.get(key).and_then(JsValue::as_str).map(str::to_owned)
}

/// Position fields every event of one user line shares.
pub struct UserBase<'a> {
    pub row: &'a JsValue,
    pub index: usize,
    pub ts: Option<&'a str>,
    pub is_sidechain: bool,
}

/// The event for one block of a user line's content array, if any.
pub fn user_block_event(
    base: &UserBase<'_>,
    block: &JsValue,
    block_index: usize,
) -> Option<TimelineEvent> {
    let ty = block.get("type").and_then(JsValue::as_str);
    if ty == Some("tool_result") {
        let content = block.get("content");
        let raw = omit_image_data(content);
        let text = str_of(raw.as_ref());
        let images = tool_result_image_refs(block_index, content);
        let error = block.get("is_error");
        let title = if error.is_some_and(JsValue::truthy) {
            "tool_result (error)"
        } else {
            "tool_result"
        };
        let mut e = TimelineEvent::new(EventShape::ToolResult, base.index, EventKind::ToolResult)
            .text(
                title.to_owned(),
                first_line(&text),
                text.clone(),
                units::est_tokens(&text),
            );
        e.is_sidechain = Some(base.is_sidechain);
        e.is_error = Some(error.is_some_and(JsValue::is_true));
        e.ts = base.ts.map(str::to_owned);
        e.images = (!images.is_empty()).then_some(images);
        e.tool_use_id = str_field(block, "tool_use_id");
        return Some(e);
    }
    if ty == Some("text") {
        return Some(prompt_event(PromptArgs {
            o: base.row,
            index: base.index,
            ts: base.ts,
            is_sidechain: base.is_sidechain,
            text: str_of(block.get("text")),
            title: "User message",
            tokens_est: None,
        }));
    }
    let img = as_image(block)?;
    let marker = image_marker(img);
    let mut e = TimelineEvent::new(EventShape::Image, base.index, EventKind::Attachment).text(
        "image".to_owned(),
        marker.clone(),
        marker.clone(),
        units::est_tokens(&marker),
    );
    e.is_sidechain = Some(base.is_sidechain);
    e.attachment_type = Some("image".to_owned());
    e.loaded_category = Some(LoadedCategory::File);
    e.images = Some(vec![image_ref(img, block_index, None)]);
    e.ts = base.ts.map(str::to_owned);
    Some(e)
}

/// Position fields every event of one assistant line shares.
pub struct AssistantBase<'a> {
    pub index: usize,
    pub request_id: &'a str,
    pub is_sidechain: bool,
    pub ts: Option<&'a str>,
    pub thinking_duration_ms: Option<f64>,
}

fn assistant_event(
    base: &AssistantBase<'_>,
    kind: EventKind,
    title: String,
    preview: String,
    body: String,
    tokens: f64,
) -> TimelineEvent {
    let mut e = TimelineEvent::new(EventShape::AssistantBlock, base.index, kind)
        .text(title, preview, body, tokens);
    e.request_id = Some(base.request_id.to_owned());
    e.is_sidechain = Some(base.is_sidechain);
    e.ts = base.ts.map(str::to_owned);
    e
}

/// `s.replace(/\s+/g, " ")` then the first line.
fn one_line_preview(s: &str) -> String {
    first_line(&units::collapse_ws(s))
}

/// The event for one block of an assistant message, if any.
pub fn assistant_block_event(base: &AssistantBase<'_>, block: &JsValue) -> Option<TimelineEvent> {
    let ty = block.get("type").and_then(JsValue::as_str)?;
    match ty {
        "thinking" | "redacted_thinking" => {
            let text = str_of(block.get("thinking"));
            let preview = if text.is_empty() {
                "(content not stored in transcript)".to_owned()
            } else {
                first_line(&text)
            };
            let tokens = units::est_tokens(&text);
            let mut e = assistant_event(
                base,
                EventKind::AssistantThinking,
                "Thinking".to_owned(),
                preview,
                text,
                tokens,
            );
            e.thinking_duration_ms = base.thinking_duration_ms;
            Some(e)
        }
        "text" => {
            let text = str_of(block.get("text"));
            let (preview, tokens) = (first_line(&text), units::est_tokens(&text));
            Some(assistant_event(
                base,
                EventKind::AssistantText,
                "Assistant".to_owned(),
                preview,
                text,
                tokens,
            ))
        }
        "tool_use" | "server_tool_use" => {
            let input = defined(block.get("input"))
                .cloned()
                .unwrap_or_else(|| JsValue::Obj(JsObject::default()));
            let input = input.stringify(true);
            let name = js_string(Some(
                defined(block.get("name")).unwrap_or(&JsValue::Str("tool".to_owned())),
            ));
            let (preview, tokens) = (one_line_preview(&input), units::est_tokens(&input));
            let mut e = assistant_event(
                base,
                EventKind::ToolCall,
                name.clone(),
                preview,
                input,
                tokens,
            );
            e.tool_name = Some(name);
            e.tool_use_id = str_field(block, "id");
            Some(e)
        }
        "advisor_tool_result" => Some(advisor_result_event(base, block)),
        t if t.ends_with("_tool_result") => Some(server_result_event(base, block, t)),
        _ => None,
    }
}

fn advisor_result_event(base: &AssistantBase<'_>, block: &JsValue) -> TimelineEvent {
    let result = defined(block.get("content"));
    let redacted = result.and_then(|r| r.get("type")).and_then(JsValue::as_str)
        == Some("advisor_redacted_result");
    let text = if redacted {
        "(advisor response encrypted, not stored in transcript)".to_owned()
    } else {
        let text = result.and_then(|r| defined(r.get("text")));
        match (text, result) {
            (Some(t), _) => str_of(Some(t)),
            (None, Some(r)) => str_of(Some(r)),
            (None, None) => "{}".to_owned(),
        }
    };
    let tokens = if redacted {
        0.0
    } else {
        units::est_tokens(&text)
    };
    let preview = first_line(&text);
    let mut e = assistant_event(
        base,
        EventKind::ToolResult,
        "advisor result".to_owned(),
        preview,
        text,
        tokens,
    );
    e.server_tool = Some(true);
    e.tool_use_id = str_field(block, "tool_use_id");
    e
}

/// One web search hit as a Markdown list item.
fn search_hit(r: &JsValue) -> String {
    let title = str_of(r.get("title"));
    let title = if title.is_empty() {
        str_of(r.get("url"))
    } else {
        title
    };
    let age = r
        .get("page_age")
        .and_then(JsValue::as_str)
        .map(|a| format!(" · {a}"))
        .unwrap_or_default();
    format!("- [{title}]({}){age}", str_of(r.get("url")))
}

/// Text of content blocks, joined; anything else as JSON.
fn block_text(content: Option<&JsValue>) -> String {
    match content.and_then(JsValue::as_arr) {
        Some(items) => items
            .iter()
            .map(|b| match b.get("text").and_then(JsValue::as_str) {
                Some(t) => t.to_owned(),
                None => str_of(Some(b)),
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        None => str_of(content),
    }
}

/// Body of a server tool result and whether it reports an error.
fn server_result_body(block: &JsValue, ty: &str) -> (String, bool) {
    let content = block.get("content");
    let c = defined(content);
    if let Some(code) = c
        .and_then(|c| c.get("error_code"))
        .and_then(JsValue::as_str)
    {
        return (format!("error: {code}"), true);
    }
    if ty != "web_search_tool_result" && ty != "web_fetch_tool_result" {
        return (block_text(content), false);
    }
    if let Some(hits) = content.and_then(JsValue::as_arr) {
        return (
            hits.iter().map(search_hit).collect::<Vec<_>>().join("\n"),
            false,
        );
    }
    let doc = c.and_then(|c| defined(c.get("content")));
    let source = doc.and_then(|d| defined(d.get("source")));
    let data = source
        .filter(|s| s.get("type").and_then(JsValue::as_str) == Some("text"))
        .and_then(|s| s.get("data"))
        .and_then(JsValue::as_str)
        .unwrap_or("");
    let url = str_of(c.and_then(|c| c.get("url")));
    let title = doc
        .and_then(|d| d.get("title"))
        .and_then(JsValue::as_str)
        .unwrap_or("");
    let head = [url.as_str(), title]
        .into_iter()
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" — ");
    let text = if data.is_empty() {
        head
    } else {
        format!("{head}\n\n{data}")
    };
    (text, false)
}

/// A server tool's result, which the API returns inside the assistant
/// message. Search hits carry encrypted page text, so only titles and URLs
/// are readable.
fn server_result_event(base: &AssistantBase<'_>, block: &JsValue, ty: &str) -> TimelineEvent {
    let name = ty.strip_suffix("_tool_result").unwrap_or(ty);
    let (text, is_error) = server_result_body(block, ty);
    let title = format!("{name} result{}", if is_error { " (error)" } else { "" });
    let (preview, tokens) = (first_line(&text), units::est_tokens(&text));
    let mut e = assistant_event(base, EventKind::ToolResult, title, preview, text, tokens);
    e.server_tool = Some(true);
    e.is_error = is_error.then_some(true);
    e.tool_use_id = str_field(block, "tool_use_id");
    e
}
