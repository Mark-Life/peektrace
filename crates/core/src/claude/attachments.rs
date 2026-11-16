//! Claude `attachment` lines: context the harness injects. Newer transcripts
//! log the model-facing text on `rendered`; older ones only the structured
//! payload, so the body is rebuilt from it. `prompt_snapshot` attachments
//! become `system-prompt` events.

use crate::js::units;
use crate::js::value::{JsObject, JsValue, defined, join, js_string, str_of};
use crate::sessions::images::{as_image, image_marker};
use crate::sessions::model::{EventKind, EventShape, LoadedCategory, TimelineEvent};
use crate::sessions::prompt::{PromptArgs, prompt_event};
use crate::sessions::system_prompt::{
    PromptSection, SystemPromptArgs, system_prompt_event, tools_section,
};

/// Separator block Claude Code writes between static and dynamic prompt parts.
const PROMPT_BOUNDARY: &str = "__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__";

/// Types Claude Code keeps in the transcript but never sends to the model.
const METADATA_ONLY: [&str; 28] = [
    "structured_output",
    "deferred_tools_record",
    "credential_org",
    "command_permissions",
    "max_turns_reached",
    "thinking_drop",
    "thinking_stripped",
    "advisor_stripped",
    "prompt_render_point",
    "already_read_file",
    "async_hook_response_batch",
    "batching_reminder_sent",
    "secondary_reminder_sent",
    "edited_image_file",
    "goal_status",
    "session_cron_carry",
    "projects_thread_ask",
    "teammate_shutdown_batch",
    "tool_host_result_lines",
    "withheld_memory",
    "hook_cancelled",
    "hook_error_during_execution",
    "hook_non_blocking_error",
    "hook_system_message",
    "hook_permission_decision",
    "hook_deferred_tool",
    "context_efficiency",
    "goals",
];

/// Hook events whose `hook_success` output is added to the context.
const CONTEXT_HOOK_EVENTS: [&str; 3] = ["SessionStart", "UserPromptSubmit", "UserPromptExpansion"];

/// `String(a.type ?? fallback)`.
fn type_of(a: &JsValue, fallback: &str) -> String {
    defined(a.get("type")).map_or_else(|| fallback.to_owned(), |t| js_string(Some(t)))
}

/// The attachment never reaches the model. A `hook_success` without
/// `hookEvent` (older lines) still counts.
fn is_metadata_only(a: &JsValue) -> bool {
    let t = type_of(a, "");
    if t == "hook_success" {
        return a
            .get("hookEvent")
            .and_then(JsValue::as_str)
            .is_some_and(|e| !CONTEXT_HOOK_EVENTS.contains(&e));
    }
    METADATA_ONLY.contains(&t.as_str())
}

/// The budget category an attachment loads.
fn attachment_category(a: &JsValue) -> LoadedCategory {
    match type_of(a, "").as_str() {
        "file" => {
            let name = defined(a.get("filename"))
                .map(|f| js_string(Some(f)))
                .unwrap_or_default()
                .to_lowercase();
            if name.ends_with("claude.md") || name.ends_with("agents.md") {
                LoadedCategory::ClaudeMd
            } else {
                LoadedCategory::File
            }
        }
        "instructions" | "nested_memory" => LoadedCategory::ClaudeMd,
        "skill_listing" | "invoked_skills" | "dynamic_skill" => LoadedCategory::Skills,
        "agent_listing_delta" => LoadedCategory::Agents,
        "deferred_tools_delta" => LoadedCategory::Tools,
        "mcp_instructions_delta" => LoadedCategory::Mcp,
        "opened_file_in_ide" | "selected_lines_in_ide" | "edited_text_file" => LoadedCategory::Ide,
        "image" | "compact_file_reference" => LoadedCategory::File,
        "task_reminder"
        | "hook_success"
        | "total_tokens_reminder"
        | "silent_turn_reminder"
        | "output_style"
        | "output_style_instructions"
        | "environment"
        | "date"
        | "model"
        | "session_context"
        | "auto_mode"
        | "auto_mode_exit"
        | "remote_session_change"
        | "read_truncation_notice"
        | "plan_mode"
        | "plan_mode_exit"
        | "plan_mode_reentry"
        | "todo_reminder"
        | "queued_command"
        | "date_change"
        | "ultra_effort_enter"
        | "workflow_keyword_request" => LoadedCategory::Reminder,
        _ => LoadedCategory::Other,
    }
}

/// Body of a `file` attachment: the file text when present.
fn file_body(c: Option<&JsValue>) -> String {
    if let Some(JsValue::Str(s)) = c {
        return s.clone();
    }
    match c
        .and_then(|c| c.get("file"))
        .and_then(|f| f.get("content"))
        .and_then(JsValue::as_str)
    {
        Some(text) => text.to_owned(),
        None => str_of(c),
    }
}

/// CLAUDE.md files an `instructions` attachment lists, each under its path.
fn instructions_body(a: &JsValue) -> String {
    match a.get("files").and_then(JsValue::as_arr) {
        Some(files) => files
            .iter()
            .map(|f| {
                format!(
                    "# {}\n\n{}",
                    str_of(f.get("path")),
                    str_of(f.get("content"))
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n"),
        None => str_of(Some(a)),
    }
}

/// `list?.join(sep)`; `None` when the value is not an array.
fn joined(v: Option<&JsValue>, sep: &str) -> Option<String> {
    v.and_then(JsValue::as_arr).map(|items| join(items, sep))
}

/// A readable body rebuilt from the structured payload.
fn structured_body(a: &JsValue) -> String {
    match type_of(a, "").as_str() {
        "file" => file_body(a.get("content")),
        "skill_listing" | "selected_lines_in_ide" => str_of(a.get("content")),
        "agent_listing_delta" => joined(a.get("addedLines"), "\n").unwrap_or_default(),
        "deferred_tools_delta" => joined(a.get("addedNames"), ", ").unwrap_or_default(),
        "mcp_instructions_delta" => {
            joined(a.get("addedBlocks"), "\n\n").unwrap_or_else(|| str_of(Some(a)))
        }
        "edited_text_file" => str_of(a.get("snippet")),
        "hook_success" => format!(
            "$ {}\n{}{}",
            str_of(a.get("command")),
            str_of(a.get("stdout")),
            str_of(a.get("stderr"))
        ),
        "instructions" => instructions_body(a),
        "nested_memory" => {
            let c = defined(a.get("content"));
            match c.and_then(|c| c.get("content")).and_then(JsValue::as_str) {
                Some(text) => text.to_owned(),
                None => str_of(Some(c.unwrap_or(a))),
            }
        }
        _ => str_of(Some(defined(a.get("content")).unwrap_or(a))),
    }
}

/// The exact text the model saw, from the line's `rendered` blocks.
fn rendered_body(o: &JsValue) -> Option<String> {
    let text = o
        .get("rendered")?
        .as_arr()?
        .iter()
        .filter_map(|r| r.get("content").and_then(JsValue::as_str))
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

/// The last segment of a `/` path, or the whole path when that is empty.
fn base_name(path: &str) -> &str {
    match path.rsplit('/').next() {
        Some(last) if !last.is_empty() => last,
        _ => path,
    }
}

/// Short title for an attachment row.
fn attachment_title(a: &JsValue) -> String {
    let t = type_of(a, "attachment");
    match t.as_str() {
        "file" | "edited_text_file" | "opened_file_in_ide" => {
            let path = defined(a.get("filename"))
                .or_else(|| defined(a.get("displayPath")))
                .map(|p| js_string(Some(p)))
                .unwrap_or_default();
            format!("{t}: {}", base_name(&path))
        }
        "skill_listing" => {
            let count =
                defined(a.get("skillCount")).map_or_else(|| "?".to_owned(), |c| js_string(Some(c)));
            format!("skill_listing ({count} skills)")
        }
        "nested_memory" => {
            let path = defined(a.get("displayPath"))
                .or_else(|| defined(a.get("path")))
                .map(|p| js_string(Some(p)))
                .unwrap_or_default();
            format!("nested_memory: {path}")
        }
        _ => t,
    }
}

/// Prompt parts already shown in one thread, so a repeat snapshot only adds
/// what changed.
#[derive(Debug, Default)]
pub struct PromptSeen {
    cli_prefix: Option<String>,
    prompt: Option<String>,
    tools: Option<String>,
}

/// Keep a section only when its text differs from the last one shown.
fn changed(slot: &mut Option<String>, section: Option<PromptSection>) -> Option<PromptSection> {
    let section = section?;
    if slot.as_deref() == Some(section.text.as_str()) {
        return None;
    }
    *slot = Some(section.text.clone());
    Some(section)
}

/// Position fields shared by every event of one attachment line.
pub struct AttachmentBase<'a> {
    pub index: usize,
    pub is_sidechain: bool,
    pub ts: Option<&'a str>,
}

fn prompt_snapshot_event(
    a: &JsValue,
    base: &AttachmentBase<'_>,
    seen: &mut PromptSeen,
) -> Option<TimelineEvent> {
    let first = seen.prompt.is_none();
    let blocks: Vec<&str> = a
        .get("systemPrompt")
        .and_then(JsValue::as_arr)
        .unwrap_or_default()
        .iter()
        .filter_map(JsValue::as_str)
        .filter(|b| *b != PROMPT_BOUNDARY)
        .collect();
    let cli = a
        .get("cliPrefix")
        .and_then(JsValue::as_str)
        .map(|p| PromptSection {
            name: "cli prefix".to_owned(),
            sent: None,
            text: p.to_owned(),
        });
    let prompt = (!blocks.is_empty()).then(|| PromptSection {
        name: "system prompt".to_owned(),
        sent: None,
        text: blocks.join("\n\n"),
    });
    let sections = vec![
        changed(&mut seen.cli_prefix, cli),
        changed(&mut seen.prompt, prompt),
        changed(&mut seen.tools, tools_section(a.get("tools"), "tools")),
    ];
    system_prompt_event(SystemPromptArgs {
        index: base.index,
        ts: base.ts,
        title: if first {
            "System prompt"
        } else {
            "System prompt update"
        },
        sections,
        attachment_type: "prompt_snapshot",
        is_sidechain: Some(base.is_sidechain),
    })
}

/// Text of a queued prompt: a string, or a block array's text parts with
/// images as markers.
fn queued_text(prompt: Option<&JsValue>) -> String {
    match prompt {
        Some(JsValue::Str(s)) => s.clone(),
        Some(JsValue::Arr(items)) => items
            .iter()
            .map(|b| match as_image(b) {
                Some(img) => image_marker(img),
                None if b.get("type").and_then(JsValue::as_str) == Some("text") => b
                    .get("text")
                    .and_then(JsValue::as_str)
                    .unwrap_or("")
                    .to_owned(),
                None => String::new(),
            })
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// A message the user sent while the agent was busy, or `None` when it has
/// no text, so the line falls back to a plain attachment.
fn queued_prompt_event(
    o: &JsValue,
    a: &JsValue,
    base: &AttachmentBase<'_>,
) -> Option<TimelineEvent> {
    let text = queued_text(a.get("prompt"));
    if text.is_empty() {
        return None;
    }
    let tokens = match rendered_body(o) {
        Some(shown) => units::est_tokens(&shown),
        None if a.get("renderedByBatchHead").is_some_and(JsValue::is_true) => 0.0,
        None => units::est_tokens(&text),
    };
    let mut e = prompt_event(PromptArgs {
        o: a,
        index: base.index,
        ts: base.ts,
        is_sidechain: base.is_sidechain,
        text,
        title: "Queued message",
        tokens_est: Some(tokens),
    });
    e.shape = EventShape::QueuedPrompt;
    e.attachment_type = Some("queued_command".to_owned());
    Some(e)
}

/// `^\s*<system-reminder>\s*` removed from the start.
fn strip_reminder_open(body: &str) -> &str {
    let ws = crate::js::text::is_js_whitespace;
    body.trim_start_matches(ws)
        .strip_prefix("<system-reminder>")
        .map_or(body, |rest| rest.trim_start_matches(ws))
}

/// The event for one `attachment` line, or `None` when it adds nothing new.
pub fn attachment_event(
    o: &JsValue,
    base: &AttachmentBase<'_>,
    seen: &mut PromptSeen,
) -> Option<TimelineEvent> {
    let empty = JsValue::Obj(JsObject::default());
    let a = defined(o.get("attachment")).unwrap_or(&empty);
    let ty = a.get("type").and_then(JsValue::as_str);
    if ty == Some("prompt_snapshot") {
        return prompt_snapshot_event(a, base, seen);
    }
    if ty == Some("queued_command")
        && a.get("commandMode").and_then(JsValue::as_str) == Some("prompt")
        && let Some(e) = queued_prompt_event(o, a, base)
    {
        return Some(e);
    }
    let shown = rendered_body(o);
    let metadata = shown.is_none() && is_metadata_only(a);
    let body = shown.unwrap_or_else(|| structured_body(a));
    let preview = units::first_line(strip_reminder_open(&body), 200);
    let preview = if preview.is_empty() {
        type_of(a, "")
    } else {
        preview
    };
    let tokens = if metadata {
        0.0
    } else {
        units::est_tokens(&body)
    };
    let mut e = TimelineEvent::new(EventShape::Attachment, base.index, EventKind::Attachment).text(
        attachment_title(a),
        preview,
        body,
        tokens,
    );
    e.is_sidechain = Some(base.is_sidechain);
    e.attachment_type = Some(type_of(a, ""));
    e.loaded_category = (!metadata).then(|| attachment_category(a));
    e.ts = base.ts.map(str::to_owned);
    Some(e)
}
