//! Pi entries that reach the model beyond plain user, assistant and tool
//! turns: the system message (named sections and tool definitions),
//! compaction and branch summaries, extension messages and `!` bash runs.

use crate::js::num;
use crate::js::units;
use crate::js::value::{JsValue, defined, js_string, str_of};
use crate::sessions::images::{as_image, image_marker};
use crate::sessions::model::{EventKind, EventShape, LoadedCategory, TimelineEvent};
use crate::sessions::system_prompt::{
    PromptSection, SystemPromptArgs, system_prompt_event, tools_section,
};

pub const LINE_CAP: usize = 200;

const COMPACTION_PREFIX: &str = "The conversation history before this point was compacted into the following summary:\n\n<summary>\n";
const COMPACTION_SUFFIX: &str = "\n</summary>";
const BRANCH_PREFIX: &str =
    "The following is a summary of a branch that this conversation came back from:\n\n<summary>\n";
const BRANCH_SUFFIX: &str = "</summary>";

/// Position fields every event built here carries.
#[derive(Debug, Clone, Copy)]
pub struct Base<'a> {
    pub index: usize,
    pub ts: Option<&'a str>,
}

impl Base<'_> {
    /// An event with its preview and size set.
    pub fn event(
        self,
        shape: EventShape,
        kind: EventKind,
        title: String,
        preview: String,
        body: String,
    ) -> TimelineEvent {
        let tokens = units::est_tokens(&body);
        let mut e = TimelineEvent::new(shape, self.index, kind).text(title, preview, body, tokens);
        e.ts = self.ts.map(str::to_owned);
        e
    }
}

/// `String(v ?? fallback)`.
pub fn string_or(v: Option<&JsValue>, fallback: &str) -> String {
    defined(v).map_or_else(|| fallback.to_owned(), |v| js_string(Some(v)))
}

/// Text blocks of a Pi content value joined by `\n`; an image becomes a
/// marker of its `mimeType`.
pub fn pi_text(content: Option<&JsValue>) -> String {
    match content {
        Some(JsValue::Str(s)) => s.clone(),
        Some(JsValue::Arr(items)) => items
            .iter()
            .map(|b| match b.get("text") {
                Some(JsValue::Str(t)) => t.clone(),
                _ if b.get("type").and_then(JsValue::as_str) == Some("image") => {
                    format!("[image: {}]", string_or(b.get("mimeType"), "image"))
                }
                _ => String::new(),
            })
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// A tool result's text blocks joined with no separator; images become
/// markers.
pub fn tool_result_text(content: Option<&JsValue>) -> String {
    match content {
        Some(JsValue::Str(s)) => s.clone(),
        Some(JsValue::Arr(items)) => items
            .iter()
            .map(|b| match as_image(b) {
                Some(img) => image_marker(img),
                None => b
                    .get("text")
                    .and_then(JsValue::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            })
            .collect(),
        other => str_of(other),
    }
}

/// `Object.entries(v ?? {})` as `(name, value)`; a string gives one entry per
/// UTF-16 unit.
fn entries_of(v: Option<&JsValue>) -> Vec<(String, Option<String>)> {
    match defined(v) {
        Some(JsValue::Obj(o)) => o
            .entries()
            .map(|(k, v)| (k.to_owned(), v.as_str().map(str::to_owned)))
            .collect(),
        Some(JsValue::Arr(items)) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v.as_str().map(str::to_owned)))
            .collect(),
        Some(JsValue::Str(s)) => (0..units::len(s))
            .map(|i| (i.to_string(), Some(units::slice(s, i, i + 1))))
            .collect(),
        _ => Vec::new(),
    }
}

fn section(name: &str, text: String) -> PromptSection {
    PromptSection {
        name: name.to_owned(),
        sent: None,
        text,
    }
}

/// Sections of a Pi system message: `content`, named sections, tool changes.
fn system_sections(msg: &JsValue) -> Vec<Option<PromptSection>> {
    let content = pi_text(msg.get("content"));
    let named = entries_of(msg.get("sections"))
        .into_iter()
        .map(|(name, value)| {
            let text = value.unwrap_or_else(|| format!("(section \"{name}\" removed)"));
            Some(section(&name, text))
        });
    let removed: Vec<String> = msg
        .get("toolsRemoved")
        .and_then(JsValue::as_arr)
        .unwrap_or_default()
        .iter()
        .map(|t| string_or(t.get("name"), ""))
        .filter(|n| !n.is_empty())
        .collect();
    let tools_label = if removed.is_empty() {
        "tools"
    } else {
        "tools added"
    };
    std::iter::once((!content.is_empty()).then(|| section("content", content)))
        .chain(named)
        .chain([
            tools_section(msg.get("toolsAdded"), tools_label),
            (!removed.is_empty()).then(|| section("tools removed", removed.join(", "))),
        ])
        .collect()
}

/// A Pi system message; the first is the prompt, later ones patch it.
pub fn system_event(msg: &JsValue, base: Base<'_>, first: bool) -> Option<TimelineEvent> {
    system_prompt_event(SystemPromptArgs {
        index: base.index,
        ts: base.ts,
        title: if first {
            "System prompt"
        } else {
            "System prompt update"
        },
        attachment_type: "system_message",
        sections: system_sections(msg),
        is_sidechain: None,
    })
}

/// `s.replace(/<\/?summary>/g, "")`.
fn without_summary_tags(s: &str) -> String {
    s.replace("<summary>", "").replace("</summary>", "")
}

fn summary_event(base: Base<'_>, kind: EventKind, title: &str, body: String) -> TimelineEvent {
    let preview = units::first_line(&without_summary_tags(&body), LINE_CAP);
    base.event(EventShape::Summary, kind, title.to_owned(), preview, body)
}

/// A compaction summary, as the model sees it.
pub fn compaction_event(base: Base<'_>, summary: Option<&JsValue>) -> TimelineEvent {
    let body = format!(
        "{COMPACTION_PREFIX}{}{COMPACTION_SUFFIX}",
        string_or(summary, "")
    );
    summary_event(
        base,
        EventKind::Compaction,
        "Context compaction (summary)",
        body,
    )
}

/// A branch summary, as the model sees it.
pub fn branch_summary_event(base: Base<'_>, summary: Option<&JsValue>) -> TimelineEvent {
    let body = format!("{BRANCH_PREFIX}{}{BRANCH_SUFFIX}", string_or(summary, ""));
    summary_event(base, EventKind::Summary, "Branch summary", body)
}

fn attachment(
    base: Base<'_>,
    title: String,
    preview: String,
    body: String,
    kind: &str,
) -> TimelineEvent {
    let mut e = base.event(
        EventShape::PiAttachment,
        EventKind::Attachment,
        title,
        preview,
        body,
    );
    e.attachment_type = Some(kind.to_owned());
    e.loaded_category = Some(LoadedCategory::Other);
    e
}

/// An extension message, sent to the model as a user turn; `None` when it
/// has no text.
pub fn custom_event(base: Base<'_>, entry: &JsValue) -> Option<TimelineEvent> {
    let body = pi_text(entry.get("content"));
    if body.is_empty() {
        return None;
    }
    let title = entry
        .get("customType")
        .and_then(JsValue::as_str)
        .unwrap_or("custom")
        .to_owned();
    let preview = units::first_line(&body, LINE_CAP);
    Some(attachment(base, title, preview, body, "custom_message"))
}

/// The text Pi sends for a user `!` command.
fn bash_text(msg: &JsValue) -> String {
    let output = msg
        .get("output")
        .and_then(JsValue::as_str)
        .unwrap_or_default();
    let shown = if output.is_empty() {
        "(no output)".to_owned()
    } else {
        format!("```\n{output}\n```")
    };
    let mut parts = vec![format!(
        "Ran `{}`\n{shown}",
        string_or(msg.get("command"), "")
    )];
    if msg.get("cancelled").is_some_and(JsValue::is_true) {
        parts.push("(command cancelled)".to_owned());
    } else if let Some(code) = msg.get("exitCode").and_then(JsValue::as_num)
        && code != 0.0
    {
        parts.push(format!(
            "Command exited with code {}",
            num::to_js_string(code)
        ));
    }
    if msg.get("truncated").is_some_and(JsValue::is_true)
        && let Some(path) = msg.get("fullOutputPath").and_then(JsValue::as_str)
    {
        parts.push(format!("[Output truncated. Full output: {path}]"));
    }
    parts.join("\n\n")
}

/// A user-run shell command, or `None` when it was kept out of context.
pub fn bash_event(base: Base<'_>, msg: &JsValue) -> Option<TimelineEvent> {
    if msg.get("excludeFromContext").is_some_and(JsValue::is_true) {
        return None;
    }
    let command = units::first_line(&string_or(msg.get("command"), ""), LINE_CAP);
    Some(attachment(
        base,
        format!("$ {command}"),
        command,
        bash_text(msg),
        "bash_execution",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(src: &str) -> JsValue {
        JsValue::parse(src).unwrap_or(JsValue::Null)
    }

    #[test]
    fn texts() {
        let c = v(
            r#"["a",{"text":""},{"type":"image","mimeType":"image/png"},{"type":"image"},{"text":"b"}]"#,
        );
        assert_eq!(pi_text(Some(&c)), "[image: image/png]\n[image: image]\nb");
        let r = v(r#"[{"text":"a"},{"type":"image","mimeType":"image/png"},{"text":"b"},5]"#);
        assert_eq!(tool_result_text(Some(&r)), "a[image: image/png]b");
        assert_eq!(tool_result_text(Some(&v("5"))), "5");
        assert_eq!(tool_result_text(None), "");
    }

    #[test]
    fn string_sections_split_by_unit() {
        let got = entries_of(Some(&v(r#""a😀""#)));
        assert_eq!(got.len(), 3);
        assert_eq!(got[0], ("0".to_owned(), Some("a".to_owned())));
    }

    #[test]
    fn bash_bodies() {
        let msg = v(
            r#"{"command":"ls\nx","output":"o","exitCode":2.5,"truncated":true,"fullOutputPath":"/f"}"#,
        );
        assert_eq!(
            bash_text(&msg),
            "Ran `ls\nx`\n```\no\n```\n\nCommand exited with code 2.5\n\n[Output truncated. Full output: /f]"
        );
        let e = bash_event(Base { index: 0, ts: None }, &msg).map(|e| e.title);
        assert_eq!(e.as_deref(), Some("$ ls"));
    }
}
