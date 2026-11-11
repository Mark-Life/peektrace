//! `system-prompt` events and tool definitions as readable Markdown: a
//! heading per tool, its description, and one list item per parameter.

use super::model::{EventKind, EventShape, TimelineEvent};
use crate::js::text;
use crate::js::units;
use crate::js::value::{JsObject, JsValue, defined, join};

/// One named part of a system prompt. `sent` is what the model received
/// when `text` reshapes it for reading; it sizes the section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptSection {
    pub name: String,
    pub sent: Option<String>,
    pub text: String,
}

/// Schema keys the list item itself shows.
const SHOWN: [&str; 9] = [
    "type",
    "description",
    "properties",
    "items",
    "required",
    "enum",
    "$schema",
    "additionalProperties",
    "title",
];

/// Tool keys the Markdown shows; others stay visible as a JSON block.
const TOOL_KEYS: [&str; 9] = [
    "name",
    "description",
    "schema",
    "input_schema",
    "inputSchema",
    "parameters",
    "tools",
    "type",
    "eager_input_streaming",
];

const INDENT: &str = "  ";
const HEADING_LEVEL: usize = 3;

fn obj(v: Option<&JsValue>) -> Option<&JsObject> {
    v.and_then(JsValue::as_obj)
}

/// The input schema, wherever this agent put it.
fn input_schema(tool: &JsObject) -> Option<&JsObject> {
    let wrapped = obj(tool.get("schema"));
    obj(tool.get("input_schema"))
        .or_else(|| obj(tool.get("inputSchema")))
        .or_else(|| obj(tool.get("parameters")))
        .or_else(|| obj(wrapped.and_then(|w| w.get("input_schema"))))
        .or_else(|| obj(wrapped.and_then(|w| w.get("parameters"))))
}

/// A short type label: `string`, `"a" | "b"`, `string[]`, `object`.
fn type_label(s: &JsObject) -> String {
    if let Some(items) = s.get("enum").and_then(JsValue::as_arr) {
        return items
            .iter()
            .map(|v| v.stringify(false))
            .collect::<Vec<_>>()
            .join(" | ");
    }
    if let Some(types) = s.get("type").and_then(JsValue::as_arr) {
        return join(types, " | ");
    }
    let ty = s.get("type");
    if ty.and_then(JsValue::as_str) == Some("array") {
        return obj(s.get("items"))
            .map_or_else(|| "array".to_owned(), |i| format!("{}[]", type_label(i)));
    }
    if let Some(t) = ty.and_then(JsValue::as_str) {
        return t.to_owned();
    }
    let union = defined(s.get("anyOf")).or_else(|| s.get("oneOf"));
    if union.and_then(JsValue::as_arr).is_some() {
        return "union".to_owned();
    }
    "any".to_owned()
}

/// Indent every line after the first so a multi-line text stays in its item.
fn hang(text: &str, depth: usize) -> String {
    text.replace('\n', &format!("\n{}", INDENT.repeat(depth + 1)))
}

/// The properties nested under a parameter: an object's, or an array item's.
fn nested_of(s: &JsObject) -> Option<&JsObject> {
    if obj(s.get("properties")).is_some() {
        return Some(s);
    }
    obj(s.get("items")).filter(|i| obj(i.get("properties")).is_some())
}

/// One list item per property of `schema`, nested ones indented beneath.
fn param_lines(schema: &JsObject, depth: usize, out: &mut Vec<String>) {
    let empty = JsObject::default();
    let props = obj(schema.get("properties")).unwrap_or(&empty);
    let required: Vec<&str> = schema
        .get("required")
        .and_then(JsValue::as_arr)
        .map(|r| r.iter().filter_map(JsValue::as_str).collect())
        .unwrap_or_default();
    for (name, raw) in props.entries() {
        let s = raw.as_obj().unwrap_or(&empty);
        let facts = std::iter::once(type_label(s))
            .chain(required.contains(&name).then(|| "required".to_owned()))
            .chain(
                s.entries()
                    .filter(|(k, _)| !SHOWN.contains(k))
                    .map(|(k, v)| format!("{k}: {}", v.stringify(false))),
            )
            .collect::<Vec<_>>()
            .join(", ");
        let desc = s
            .get("description")
            .and_then(JsValue::as_str)
            .map(text::trim)
            .filter(|d| !d.is_empty())
            .map(|d| format!(" — {}", hang(d, depth)))
            .unwrap_or_default();
        out.push(format!(
            "{}- `{name}` *{facts}*{desc}",
            INDENT.repeat(depth)
        ));
        if let Some(nested) = nested_of(s) {
            param_lines(nested, depth + 1, out);
        }
    }
}

/// One tool definition as Markdown. Member tools (a namespace) sit one
/// level down; unknown keys stay visible as JSON.
pub fn render_tool(tool: &JsValue, level: usize) -> String {
    let empty = JsObject::default();
    let o = tool.as_obj().unwrap_or(&empty);
    let wrapped = obj(o.get("schema"));
    let name = defined(o.get("name")).or_else(|| wrapped.and_then(|w| w.get("name")));
    let description =
        defined(o.get("description")).or_else(|| wrapped.and_then(|w| w.get("description")));
    let mut params = Vec::new();
    if let Some(schema) = input_schema(o) {
        param_lines(schema, 0, &mut params);
    }
    let members = o.get("tools").and_then(JsValue::as_arr).unwrap_or_default();
    let extra: Vec<_> = o
        .entries()
        .filter(|(k, _)| !TOOL_KEYS.contains(k))
        .map(|(k, v)| (k.to_owned(), v.clone()))
        .collect();
    let heading = format!(
        "{} {}",
        "#".repeat(level),
        name.and_then(JsValue::as_str).unwrap_or("tool")
    );
    let description = description
        .and_then(JsValue::as_str)
        .map_or("", text::trim)
        .to_owned();
    let params = if params.is_empty() {
        String::new()
    } else {
        format!("**Parameters**\n\n{}", params.join("\n"))
    };
    let extra = if extra.is_empty() {
        String::new()
    } else {
        let json = JsValue::Obj(JsObject::from_entries(extra)).stringify(true);
        format!("```json\n{json}\n```")
    };
    std::iter::once(heading)
        .chain([description, params])
        .chain(members.iter().map(|m| render_tool(m, level + 1)))
        .chain(std::iter::once(extra))
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The definition as the model receives it: Claude Code logs the API copy
/// under `schema`.
pub fn tool_wire_text(tool: &JsValue) -> String {
    let wire = tool
        .as_obj()
        .and_then(|o| defined(o.get("schema")))
        .unwrap_or(tool);
    wire.stringify(false)
}

/// A `tools (N)` section, or `None` when there are no tools.
pub fn tools_section(tools: Option<&JsValue>, label: &str) -> Option<PromptSection> {
    let tools = tools.and_then(JsValue::as_arr).filter(|t| !t.is_empty())?;
    Some(PromptSection {
        name: format!("{label} ({})", tools.len()),
        sent: Some(
            tools
                .iter()
                .map(tool_wire_text)
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        text: tools
            .iter()
            .map(|t| render_tool(t, HEADING_LEVEL))
            .collect::<Vec<_>>()
            .join("\n\n"),
    })
}

/// What a `system-prompt` event is built from.
pub struct SystemPromptArgs<'a> {
    pub index: usize,
    pub ts: Option<&'a str>,
    pub title: &'a str,
    pub sections: Vec<Option<PromptSection>>,
    pub attachment_type: &'a str,
    pub is_sidechain: Option<bool>,
}

/// A `system-prompt` event, or `None` when no section carries text. One
/// section shows bare; several get `## name` headings.
pub fn system_prompt_event(args: SystemPromptArgs<'_>) -> Option<TimelineEvent> {
    let sections: Vec<PromptSection> = args
        .sections
        .into_iter()
        .flatten()
        .filter(|s| !text::trim(&s.text).is_empty())
        .collect();
    let body = match sections.as_slice() {
        [] => return None,
        [one] => one.text.clone(),
        many => many
            .iter()
            .map(|s| format!("## {}\n\n{}", s.name, s.text))
            .collect::<Vec<_>>()
            .join("\n\n"),
    };
    let preview = if sections.len() == 1 {
        units::first_line(&body, 200)
    } else {
        sections
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(" · ")
    };
    let tokens = sections
        .iter()
        .map(|s| units::est_tokens(s.sent.as_deref().unwrap_or(&s.text)))
        .fold(0.0, |a, b| a + b);
    let mut e = TimelineEvent::new(
        EventShape::SystemPrompt,
        args.index,
        EventKind::SystemPrompt,
    )
    .text(args.title.to_owned(), preview, body, tokens);
    e.attachment_type = Some(args.attachment_type.to_owned());
    e.ts = args.ts.map(str::to_owned);
    e.is_sidechain = args.is_sidechain;
    Some(e)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(src: &str) -> String {
        render_tool(&JsValue::parse(src).unwrap_or(JsValue::Null), 3)
    }

    #[test]
    fn renders_params() {
        let md = tool(
            r#"{"name":"Read","description":" Reads a file. ","input_schema":{"type":"object","properties":{"path":{"type":"string","description":"Where\nto read"},"mode":{"enum":["a","b"],"default":"a"},"list":{"type":"array","items":{"type":"object","properties":{"x":{"type":["string","null"]}}}}},"required":["path"]},"cache_control":{"type":"ephemeral"}}"#,
        );
        assert_eq!(
            md,
            "### Read\n\nReads a file.\n\n**Parameters**\n\n- `path` *string, required* — Where\n  to read\n- `mode` *\"a\" | \"b\", default: \"a\"*\n- `list` *object[]*\n  - `x` *string | null*\n\n```json\n{\n  \"cache_control\": {\n    \"type\": \"ephemeral\"\n  }\n}\n```"
        );
    }

    #[test]
    fn bare_tool() {
        assert_eq!(tool("5"), "### tool");
        assert_eq!(tool(r#"{"schema":{"name":"X"}}"#), "### X");
    }
}
