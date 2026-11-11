//! Codex rollout records that load context into the model window: the base
//! system prompt on `session_meta`, and message parts. Developer and user
//! messages bundle several injected parts (AGENTS.md, environment, skills);
//! each part is classified on its own, so a real prompt is never mislabelled.

use crate::js::text;
use crate::js::units;
use crate::js::value::{JsValue, defined, js_string};
use crate::sessions::model::{EventKind, EventShape, LoadedCategory, TimelineEvent};
use crate::sessions::system_prompt::{
    PromptSection, SystemPromptArgs, system_prompt_event, tools_section,
};

const LINE_CAP: usize = 200;

/// Position fields every event built here carries.
#[derive(Debug, Clone, Copy)]
pub struct Base<'a> {
    pub index: usize,
    pub ts: Option<&'a str>,
}

impl Base<'_> {
    /// An event with title, preview from `body`'s first line, and size.
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

/// The value when it is a plain object (not an array, not `null`).
pub fn plain(v: Option<&JsValue>) -> Option<&JsValue> {
    v.filter(|v| v.as_obj().is_some())
}

/// The non-blank `text` of each part of a `content` array.
fn part_texts(content: Option<&JsValue>) -> Vec<&str> {
    content
        .and_then(JsValue::as_arr)
        .unwrap_or_default()
        .iter()
        .filter_map(|part| plain(Some(part))?.get("text")?.as_str())
        .filter(|t| !text::trim(t).is_empty())
        .collect()
}

/// The base system prompt and dynamic tools of a `session_meta` payload.
pub fn session_meta_event(payload: &JsValue, base: Base<'_>) -> Option<TimelineEvent> {
    let instructions = plain(payload.get("base_instructions"))
        .and_then(|b| b.get("text"))
        .and_then(JsValue::as_str);
    system_prompt_event(SystemPromptArgs {
        index: base.index,
        ts: base.ts,
        title: "System prompt",
        attachment_type: "base_instructions",
        sections: vec![
            instructions.map(|t| PromptSection {
                name: "base instructions".to_owned(),
                sent: None,
                text: t.to_owned(),
            }),
            tools_section(payload.get("dynamic_tools"), "dynamic tools"),
        ],
        is_sidechain: None,
    })
}

/// What an injected part is.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Injection {
    kind: String,
    category: LoadedCategory,
}

/// Leading markers of harness-injected user-role parts.
const USER_INJECTIONS: [(&str, &str, LoadedCategory); 6] = [
    (
        "<environment_context>",
        "environment_context",
        LoadedCategory::Other,
    ),
    (
        "<user_instructions>",
        "user_instructions",
        LoadedCategory::ClaudeMd,
    ),
    (
        "# AGENTS.md instructions",
        "agents_md",
        LoadedCategory::ClaudeMd,
    ),
    (
        "<recommended_plugins>",
        "recommended_plugins",
        LoadedCategory::Other,
    ),
    ("<skill>", "skill", LoadedCategory::Skills),
    ("<turn_aborted>", "turn_aborted", LoadedCategory::Other),
];

/// Which injection a user-role part is, or `None` for real user text.
fn user_injection(part: &str) -> Option<Injection> {
    let t = text::trim_start(part);
    USER_INJECTIONS
        .iter()
        .find(|(marker, ..)| t.starts_with(marker))
        .map(|&(_, kind, category)| Injection {
            kind: kind.to_owned(),
            category,
        })
}

/// A user-role message whose every part the harness injected.
pub fn is_injected_user_message(payload: &JsValue) -> bool {
    let parts = part_texts(payload.get("content"));
    !parts.is_empty() && parts.iter().all(|p| user_injection(p).is_some())
}

/// Stands for one run of separators while a tag is rewritten, so a real `_`
/// next to a run is kept.
const SEP_MARK: char = '\u{1}';

/// The name in a leading `<tag>` (`/^<([a-z][\w -]*)>/i`).
fn leading_tag(s: &str) -> Option<&str> {
    let rest = s.strip_prefix('<')?;
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | ' ' | '-')))
        .unwrap_or(rest.len());
    let tag = &rest[..end];
    let starts_alpha = tag.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
    (starts_alpha && rest[end..].starts_with('>')).then_some(tag)
}

/// Classify a developer part by its leading `<tag>`; runs of spaces and
/// dashes in the tag become `_`.
fn developer_injection(part: &str) -> Injection {
    let Some(tag) = leading_tag(text::trim_start(part)) else {
        return Injection {
            kind: "developer_instructions".to_owned(),
            category: LoadedCategory::Other,
        };
    };
    let mut kind = String::with_capacity(tag.len());
    for c in tag.chars() {
        let sep = matches!(c, ' ' | '-');
        if !sep {
            kind.push(c);
        } else if !kind.ends_with(SEP_MARK) {
            kind.push(SEP_MARK);
        }
    }
    let kind = kind.replace(SEP_MARK, "_");
    Injection {
        category: if kind.starts_with("skills") {
            LoadedCategory::Skills
        } else {
            LoadedCategory::Other
        },
        kind,
    }
}

fn injection_event(base: Base<'_>, part: &str, injection: Injection) -> TimelineEvent {
    let mut e = base.event(
        EventShape::CodexAttachment,
        EventKind::Attachment,
        injection.kind.clone(),
        units::first_line(part, LINE_CAP),
        part.to_owned(),
    );
    e.attachment_type = Some(injection.kind);
    e.loaded_category = Some(injection.category);
    e
}

fn text_event(base: Base<'_>, kind: EventKind, title: &str, body: String) -> TimelineEvent {
    base.event(
        EventShape::CodexText,
        kind,
        title.to_owned(),
        units::first_line(&body, LINE_CAP),
        body,
    )
}

/// Events for a `response_item` message: assistant text, injected parts,
/// or a prompt (possibly empty) after them.
pub fn message_events(payload: &JsValue, base: Base<'_>) -> Vec<TimelineEvent> {
    let role = defined(payload.get("role")).map_or_else(String::new, |r| js_string(Some(r)));
    let parts = part_texts(payload.get("content"));
    match role.as_str() {
        "assistant" => vec![text_event(
            base,
            EventKind::AssistantText,
            "Assistant",
            parts.join("\n"),
        )],
        "developer" => parts
            .iter()
            .map(|p| injection_event(base, p, developer_injection(p)))
            .collect(),
        _ => {
            let (mut events, prompt) =
                parts
                    .iter()
                    .fold((Vec::new(), Vec::new()), |(mut events, mut prompt), p| {
                        match user_injection(p) {
                            Some(inj) => events.push(injection_event(base, p, inj)),
                            None => prompt.push(*p),
                        }
                        (events, prompt)
                    });
            let prompt = prompt.join("\n");
            if prompt.is_empty() && !events.is_empty() {
                return events;
            }
            events.push(text_event(
                base,
                EventKind::UserPrompt,
                "User prompt",
                prompt,
            ));
            events
        }
    }
}

/// An inter-agent message fed to this agent's model, or `None` when empty.
pub fn agent_message_event(payload: &JsValue, base: Base<'_>) -> Option<TimelineEvent> {
    let body = part_texts(payload.get("content")).join("\n");
    if body.is_empty() {
        return None;
    }
    let author = payload
        .get("author")
        .and_then(JsValue::as_str)
        .unwrap_or("agent");
    let mut e = base.event(
        EventShape::CodexAttachment,
        EventKind::Attachment,
        format!("message from {author}"),
        units::first_line(&body, LINE_CAP),
        body,
    );
    e.attachment_type = Some("agent_message".to_owned());
    e.loaded_category = Some(LoadedCategory::Other);
    Some(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn developer_tags() {
        let kind = |s: &str| developer_injection(s).kind;
        assert_eq!(kind("  <skills_instructions>x"), "skills_instructions");
        assert_eq!(kind("<Skills Thing-x>"), "Skills_Thing_x");
        assert_eq!(kind("<a  b--c>"), "a_b_c");
        assert_eq!(kind("<a -b>"), "a_b");
        assert_eq!(kind("<a_ b->"), "a__b_");
        assert_eq!(kind("<h1>"), "h1");
        assert_eq!(kind("<1a>"), "developer_instructions");
        assert_eq!(kind("<a.b>"), "developer_instructions");
        assert_eq!(kind("plain"), "developer_instructions");
        assert_eq!(
            developer_injection("<skills x>").category,
            LoadedCategory::Skills
        );
    }

    #[test]
    fn injected_users() {
        let msg = |src: &str| JsValue::parse(src).unwrap_or(JsValue::Null);
        assert!(is_injected_user_message(&msg(
            r#"{"content":[{"text":" <environment_context>x"},{"text":"  "}]}"#
        )));
        assert!(!is_injected_user_message(&msg(
            r#"{"content":[{"text":"<skill>a"},{"text":"hi"}]}"#
        )));
        assert!(!is_injected_user_message(&msg(r#"{"content":[]}"#)));
    }
}
