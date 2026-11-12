//! Who wrote a Claude `user` line: a person, the SDK, a slash command, a
//! task notification, a scheduled wake-up or the harness. Explicit fields
//! (`origin.kind`, `turnOrigin`, `promptSource`) win over the tags the
//! harness wraps its own text in.

use super::model::{EventKind, EventShape, PromptOrigin, TimelineEvent};
use crate::js::value::JsValue;
use crate::js::{text, units};

/// `^\s*<` followed by one of `tags` and `>`.
fn starts_with_tag(s: &str, tags: &[&str]) -> bool {
    s.trim_start_matches(text::is_js_whitespace)
        .strip_prefix('<')
        .is_some_and(|rest| {
            tags.iter().any(|t| {
                rest.strip_prefix(t)
                    .is_some_and(|after| after.starts_with('>'))
            })
        })
}

const COMMAND: &[&str] = &["command-name", "command-message", "bash-input"];
const COMMAND_OUTPUT: &[&str] = &[
    "local-command-stdout",
    "local-command-stderr",
    "bash-stdout",
    "bash-stderr",
];

/// The trimmed value of the first `<tag>value</tag>` whose value holds no
/// `<`, as `/<tag>\s*([^<]*?)\s*<\/tag>/` captures it.
pub fn tag_value<'a>(s: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut from = 0;
    while let Some(at) = s[from..].find(&open) {
        let start = from + at + open.len();
        let rest = &s[start..];
        let end = rest.find('<').unwrap_or(rest.len());
        if rest[end..].starts_with(&close) {
            return Some(text::trim(&rest[..end]));
        }
        from += at + 1;
    }
    None
}

/// The origin of a prompt from its line's fields, with the text as fallback.
pub fn claude_prompt_origin(o: &JsValue, text: &str) -> PromptOrigin {
    let kind = o.get("origin").and_then(|v| v.get("kind"));
    let turn_origin = o.get("turnOrigin").and_then(JsValue::as_str);
    if kind.and_then(JsValue::as_str) == Some("task-notification")
        || turn_origin == Some("task_notification")
        || starts_with_tag(text, &["task-notification"])
    {
        return PromptOrigin::TaskNotification;
    }
    if turn_origin == Some("scheduled") {
        return PromptOrigin::Scheduled;
    }
    if o.get("isMeta").is_some_and(JsValue::is_true) || starts_with_tag(text, COMMAND_OUTPUT) {
        return PromptOrigin::Harness;
    }
    if starts_with_tag(text, COMMAND) {
        return PromptOrigin::Command;
    }
    if o.get("promptSource").and_then(JsValue::as_str) == Some("sdk") || turn_origin == Some("sdk")
    {
        return PromptOrigin::Sdk;
    }
    PromptOrigin::Human
}

/// Title and preview for a prompt the harness or a command wrote.
pub fn prompt_label(origin: PromptOrigin, text: &str) -> (String, Option<String>) {
    match origin {
        PromptOrigin::TaskNotification => {
            let title = match tag_value(text, "status").filter(|s| !s.is_empty()) {
                Some(status) => format!("Task notification ({status})"),
                None => "Task notification".to_owned(),
            };
            let preview = tag_value(text, "summary")
                .filter(|s| !s.is_empty())
                .map(str::to_owned);
            (title, preview)
        }
        PromptOrigin::Scheduled => ("Scheduled prompt".to_owned(), None),
        PromptOrigin::Harness => {
            let title = if starts_with_tag(text, COMMAND_OUTPUT) {
                "Command output"
            } else {
                "Harness message"
            };
            (title.to_owned(), None)
        }
        PromptOrigin::Command => match tag_value(text, "command-name").filter(|n| !n.is_empty()) {
            None => ("Command".to_owned(), None),
            Some(name) => {
                let line = match tag_value(text, "command-args").filter(|a| !a.is_empty()) {
                    Some(args) => format!("{name} {args}"),
                    None => name.to_owned(),
                };
                (format!("Command {name}"), Some(line))
            }
        },
        PromptOrigin::Human | PromptOrigin::Sdk => ("User prompt".to_owned(), None),
    }
}

/// Where a prompt event sits and what it says.
pub struct PromptArgs<'a> {
    /// The line (or attachment payload) the origin fields are read from.
    pub o: &'a JsValue,
    pub index: usize,
    pub ts: Option<&'a str>,
    pub is_sidechain: bool,
    pub text: String,
    /// Used when the origin has no label of its own.
    pub title: &'a str,
    /// Defaults to the text's estimate.
    pub tokens_est: Option<f64>,
}

/// A `user-prompt` event, titled after whoever wrote it.
pub fn prompt_event(args: PromptArgs<'_>) -> TimelineEvent {
    let PromptArgs {
        o,
        index,
        ts,
        is_sidechain,
        text,
        title,
        tokens_est,
    } = args;
    let origin = claude_prompt_origin(o, &text);
    let (title, preview) = match origin {
        PromptOrigin::Human | PromptOrigin::Sdk => (title.to_owned(), None),
        _ => prompt_label(origin, &text),
    };
    let preview = preview.unwrap_or_else(|| units::first_line(&text, 200));
    let tokens = tokens_est.unwrap_or_else(|| units::est_tokens(&text));
    let mut e = TimelineEvent::new(EventShape::Prompt, index, EventKind::UserPrompt)
        .text(title, preview, text, tokens);
    e.is_sidechain = Some(is_sidechain);
    e.prompt_origin = Some(origin);
    e.ts = ts.map(str::to_owned);
    e.is_meta = o
        .get("isMeta")
        .is_some_and(JsValue::is_true)
        .then_some(true);
    e.prompt_source = o
        .get("promptSource")
        .and_then(JsValue::as_str)
        .map(str::to_owned);
    e
}

#[cfg(test)]
mod tests {
    use super::*;

    fn origin(line: &str, text: &str) -> PromptOrigin {
        claude_prompt_origin(&JsValue::parse(line).unwrap_or(JsValue::Null), text)
    }

    #[test]
    fn origins() {
        assert_eq!(
            origin("{}", " <task-notification>x"),
            PromptOrigin::TaskNotification
        );
        assert_eq!(
            origin(r#"{"origin":{"kind":"task-notification"}}"#, "hi"),
            PromptOrigin::TaskNotification
        );
        assert_eq!(
            origin(r#"{"turnOrigin":"scheduled"}"#, "hi"),
            PromptOrigin::Scheduled
        );
        assert_eq!(
            origin(r#"{"isMeta":true}"#, "<command-name>x"),
            PromptOrigin::Harness
        );
        assert_eq!(origin("{}", "<bash-stdout>x"), PromptOrigin::Harness);
        assert_eq!(
            origin("{}", "\n<command-name>/x</command-name>"),
            PromptOrigin::Command
        );
        assert_eq!(origin(r#"{"promptSource":"sdk"}"#, "hi"), PromptOrigin::Sdk);
        assert_eq!(origin("{}", "hi"), PromptOrigin::Human);
    }

    #[test]
    fn labels() {
        let text = "<command-name>/model</command-name><command-args> opus </command-args>";
        assert_eq!(
            prompt_label(PromptOrigin::Command, text),
            ("Command /model".to_owned(), Some("/model opus".to_owned()))
        );
        assert_eq!(
            prompt_label(
                PromptOrigin::TaskNotification,
                "<status>done</status><summary>a\nb</summary>"
            ),
            (
                "Task notification (done)".to_owned(),
                Some("a\nb".to_owned())
            )
        );
        assert_eq!(tag_value("<x>a<b></x><x> c </x>", "x"), Some("c"));
    }
}
