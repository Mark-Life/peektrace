//! Tool calls and results shaped for display: the arg that carries the
//! payload unwrapped, edits as diffs, results as prose or code.

use std::collections::HashMap;

use peektrace_protocol::view::{EventKind, TimelineEvent};

use super::chat::file_title;
use super::diff::{LineDiff, diff_lines};
use super::utf16;
use peektrace_js::text::trim as js_trim;
use peektrace_js::value::{JsObject, JsValue as Json};
use peektrace_protocol::view::mend_surrogates;

/// Text shown for an event with no body.
pub const EMPTY_BODY: &str = "(no content)";
const SUMMARY_CAP: usize = 200;

/// Args whose value is the payload, and its language.
const PRIMARY_ARGS: [(&str, &str); 3] = [
    ("code", "typescript"),
    ("command", "bash"),
    ("script", "typescript"),
];
/// Args that hold text a person or agent wrote, shown as Markdown.
const PROSE_ARGS: [&str; 15] = [
    "args",
    "body",
    "content",
    "context",
    "description",
    "instructions",
    "message",
    "notes",
    "plan",
    "prompt",
    "question",
    "reason",
    "summary",
    "task",
    "text",
];
const PATH_ARGS: [&str; 4] = ["file_path", "path", "notebook_path", "filename"];
const EDIT_KEYS: [&str; 3] = ["old_string", "new_string", "edits"];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ToolPane {
    Code {
        label: String,
        code: String,
        language: &'static str,
    },
    Diff {
        label: String,
        diff: LineDiff,
        language: &'static str,
        old: String,
        new: String,
    },
    Prose {
        label: String,
        text: String,
    },
}

impl ToolPane {
    pub fn label(&self) -> &str {
        match self {
            Self::Code { label, .. } | Self::Diff { label, .. } | Self::Prose { label, .. } => {
                label
            }
        }
    }

    /// The text a pane shows; none for a diff.
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Code { code, .. } => Some(code),
            Self::Prose { text, .. } => Some(text),
            Self::Diff { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CallView {
    pub panes: Vec<ToolPane>,
    pub summary: String,
}

/// How a result or a plain body renders.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Body {
    Code {
        code: String,
        language: &'static str,
    },
    Prose {
        text: String,
    },
}

/// `\s+` -> one space, trimmed, capped at 200 UTF-16 units plus `…`.
pub fn one_line(s: &str) -> String {
    let mut flat = String::with_capacity(s.len().min(1024));
    let mut space = false;
    for c in js_trim(s).chars() {
        if is_js_space(c) {
            space = true;
        } else {
            if space {
                flat.push(' ');
                space = false;
            }
            flat.push(c);
        }
    }
    utf16::clip(&flat, SUMMARY_CAP)
}

fn is_js_space(c: char) -> bool {
    c == '\u{FEFF}' || (c.is_whitespace() && c != '\u{0085}')
}

/// A highlight language guessed from a file path.
pub fn language_of_path(path: &str) -> &'static str {
    let ext = |exts: &[&str]| exts.iter().any(|e| path.ends_with(&format!(".{e}")));
    if ext(&["ts", "tsx", "js", "jsx", "mts", "cts"]) {
        "typescript"
    } else if ext(&["json", "jsonc", "json5"]) {
        "json"
    } else if ext(&["md", "mdx"]) {
        "markdown"
    } else if ext(&["sh", "bash", "zsh"]) {
        "bash"
    } else {
        "text"
    }
}

fn file_path_of(input: &Json) -> &str {
    input
        .get("file_path")
        .and_then(Json::as_str)
        .unwrap_or_default()
}

fn language_of(key: &str, input: &Json) -> &'static str {
    PRIMARY_ARGS
        .iter()
        .find(|(k, _)| *k == key)
        .map_or_else(|| language_of_path(file_path_of(input)), |(_, lang)| lang)
}

fn is_payload(key: &str, value: &Json) -> bool {
    value
        .as_str()
        .is_some_and(|s| PRIMARY_ARGS.iter().any(|(k, _)| *k == key) || s.contains('\n'))
}

fn is_prose(key: &str, input: &Json) -> bool {
    PROSE_ARGS.contains(&key)
        && !PATH_ARGS
            .iter()
            .any(|k| input.get(k).and_then(Json::as_str).is_some())
}

fn replacement_in(v: &Json) -> Option<(String, String)> {
    let old = v.get("old_string")?.as_str()?;
    let new = v.get("new_string")?.as_str()?;
    Some((old.to_owned(), new.to_owned()))
}

fn edit_panes(input: &Json) -> Vec<ToolPane> {
    let batch = match input.get("edits") {
        Some(Json::Arr(items)) => items.iter().filter_map(replacement_in).collect(),
        _ => Vec::new(),
    };
    let edits: Vec<(String, String)> = replacement_in(input).into_iter().chain(batch).collect();
    let language = language_of("", input);
    let many = edits.len() > 1;
    edits
        .into_iter()
        .enumerate()
        .map(|(i, (old, new))| ToolPane::Diff {
            label: if many {
                format!("Edit {}", i + 1)
            } else {
                "Edit".to_owned()
            },
            diff: diff_lines(&old, &new),
            language,
            old,
            new,
        })
        .collect()
}

fn edit_summary(panes: &[ToolPane], input: &Json) -> String {
    let (added, removed) = panes.iter().fold((0, 0), |(a, r), p| match p {
        ToolPane::Diff { diff, .. } => (a + diff.added, r + diff.removed),
        _ => (a, r),
    });
    let name = file_path_of(input).rsplit('/').next().unwrap_or_default();
    js_trim(&format!("{name} +{added} −{removed}")).to_owned()
}

fn parameters(e: &TimelineEvent) -> CallView {
    CallView {
        panes: vec![ToolPane::Code {
            label: "Parameters".to_owned(),
            code: if e.body.is_empty() {
                EMPTY_BODY.to_owned()
            } else {
                e.body.clone()
            },
            language: "json",
        }],
        summary: e.preview.clone(),
    }
}

/// A tool call's panes: a diff per recorded replacement, then each
/// multi-line or payload arg, then the rest as JSON.
pub fn call_view(e: &TimelineEvent) -> CallView {
    let Some(input) = parse_json(&e.body) else {
        return parameters(e);
    };
    let Json::Obj(obj) = &input else {
        return parameters(e);
    };
    let diffs = edit_panes(&input);
    let args: Vec<(&str, &Json)> = obj
        .entries()
        .filter(|(k, _)| diffs.is_empty() || !EDIT_KEYS.contains(k))
        .collect();
    let (payloads, rest): (Vec<_>, Vec<_>) = args.into_iter().partition(|(k, v)| is_payload(k, v));
    if diffs.is_empty() && payloads.is_empty() {
        return parameters(e);
    }
    let summary_from_diffs = !diffs.is_empty();
    let mut panes = diffs;
    panes.extend(payloads.into_iter().map(|(key, value)| {
        let text = value.as_str().unwrap_or_default().to_owned();
        if is_prose(key, &input) {
            ToolPane::Prose {
                label: key.to_owned(),
                text,
            }
        } else {
            ToolPane::Code {
                label: key.to_owned(),
                code: text,
                language: language_of(key, &input),
            }
        }
    }));
    if !rest.is_empty() {
        let rest = JsObject::from_entries(rest.into_iter().map(|(k, v)| (k.to_owned(), v.clone())));
        panes.push(ToolPane::Code {
            label: "Other parameters".to_owned(),
            code: Json::Obj(rest).stringify(true),
            language: "json",
        });
    }
    let summary = if summary_from_diffs {
        edit_summary(&panes, &input)
    } else {
        one_line(panes.first().and_then(ToolPane::text).unwrap_or(&e.preview))
    };
    CallView { panes, summary }
}

/// `JSON.parse(s)`, or `None` where it would throw. Lone surrogate escapes
/// read as U+FFFD.
fn parse_json(s: &str) -> Option<Json> {
    Json::parse(&mend_surrogates(s))
}

fn text_from_blocks(v: &Json) -> Option<String> {
    let Json::Arr(items) = v else { return None };
    let texts: Vec<&str> = items
        .iter()
        .filter(|b| {
            matches!(b, Json::Obj(_)) && b.get("type").and_then(Json::as_str) == Some("text")
        })
        .filter_map(|b| b.get("text").and_then(Json::as_str))
        .collect();
    (!texts.is_empty()).then(|| texts.join("\n\n"))
}

fn json_body(v: &Json) -> Body {
    Body::Code {
        code: v.stringify(true),
        language: "json",
    }
}

/// A tool result: text blocks join into prose (or JSON when the text is
/// JSON), other JSON pretty-printed, anything else verbatim.
pub fn result_pane(e: &TimelineEvent) -> Body {
    if e.server_tool == Some(true) && !js_trim(&e.body).is_empty() {
        return Body::Prose {
            text: e.body.clone(),
        };
    }
    let parsed = parse_json(&e.body);
    if let Some(text) = parsed.as_ref().and_then(text_from_blocks) {
        return match parse_json(&text) {
            Some(inner @ (Json::Arr(_) | Json::Obj(_))) => json_body(&inner),
            _ => Body::Prose { text },
        };
    }
    if let Some(v) = parsed {
        return json_body(&v);
    }
    Body::Code {
        code: if e.body.is_empty() {
            EMPTY_BODY.to_owned()
        } else {
            e.body.clone()
        },
        language: "text",
    }
}

/// Tool calls and results by `toolUseId` (positions in `events`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolIndex {
    pub calls: HashMap<String, usize>,
    pub results: HashMap<String, usize>,
}

pub fn index_tools(events: &[TimelineEvent]) -> ToolIndex {
    let mut ix = ToolIndex::default();
    for (pos, e) in events.iter().enumerate() {
        let Some(id) = e.tool_use_id.as_deref().filter(|id| !id.is_empty()) else {
            continue;
        };
        match e.kind {
            EventKind::ToolCall => {
                ix.calls.insert(id.to_owned(), pos);
            }
            EventKind::ToolResult => {
                ix.results.insert(id.to_owned(), pos);
            }
            _ => {}
        }
    }
    ix
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolState {
    Completed,
    Error,
    Unanswered,
}

impl ToolState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Error => "error",
            Self::Unanswered => "unanswered",
        }
    }
}

pub fn tool_state(events: &[TimelineEvent], e: &TimelineEvent, ix: &ToolIndex) -> ToolState {
    let errored = |r: &TimelineEvent| {
        if r.is_error == Some(true) {
            ToolState::Error
        } else {
            ToolState::Completed
        }
    };
    if e.kind == EventKind::ToolResult {
        return errored(e);
    }
    e.tool_use_id
        .as_deref()
        .filter(|id| !id.is_empty())
        .and_then(|id| ix.results.get(id))
        .map_or(ToolState::Unanswered, |&p| errored(&events[p]))
}

pub fn tool_name_of<'a>(
    events: &'a [TimelineEvent],
    e: &'a TimelineEvent,
    ix: &ToolIndex,
) -> &'a str {
    e.tool_name
        .as_deref()
        .or_else(|| {
            let id = e.tool_use_id.as_deref().filter(|id| !id.is_empty())?;
            events[*ix.calls.get(id)?].tool_name.as_deref()
        })
        .unwrap_or("tool")
}

/// How a non-tool body renders: a file's content as source (Markdown files
/// excepted), everything else as Markdown.
pub fn body_view(e: &TimelineEvent) -> Option<&'static str> {
    let language = file_title(&e.title).map_or("markdown", language_of_path);
    (language != "markdown").then_some(language)
}

#[cfg(test)]
mod tests;
