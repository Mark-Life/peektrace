/** Claude Code `attachment` lines: context the harness injects into the window.
 *
 * Newer transcripts carry the exact model-facing text on `rendered`; older ones
 * only have the structured `attachment` payload, so the body is rebuilt from it.
 * `prompt_snapshot` attachments hold the system prompt and tool definitions and
 * become `system-prompt` events instead of plain attachments.
 */
import { promptEvent } from "./prompt-origin";
import type { LoadedCategory, TimelineEvent } from "./schema";
import {
  type PromptSection,
  systemPromptEvent,
  toolsSection,
} from "./system-prompt";
import { estTokens, firstLine } from "./tokens";

type Raw = Record<string, unknown>;

/** Separator block Claude Code writes between static and dynamic prompt parts. */
const PROMPT_BOUNDARY = "__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__";

const REMINDER_OPEN = /^\s*<system-reminder>\s*/;

const str = (v: unknown): string => {
  if (typeof v === "string") {
    return v;
  }
  return v == null ? "" : JSON.stringify(v);
};

/** Types Claude Code keeps in the transcript but never sends to the model. */
const METADATA_ONLY: ReadonlySet<string> = new Set([
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
]);

/** Hook events whose `hook_success` output is added to the context. */
const CONTEXT_HOOK_EVENTS: ReadonlySet<string> = new Set([
  "SessionStart",
  "UserPromptSubmit",
  "UserPromptExpansion",
]);

/** Whether the attachment never reaches the model. A `hook_success` without
 *  `hookEvent` (older lines) still counts. */
const isMetadataOnly = (a: Raw) => {
  const t = String(a.type ?? "");
  if (t === "hook_success") {
    return (
      typeof a.hookEvent === "string" && !CONTEXT_HOOK_EVENTS.has(a.hookEvent)
    );
  }
  return METADATA_ONLY.has(t);
};

/** Classify an attachment into the budget category it loads, if any. */
const attachmentCategory = (a: Raw): LoadedCategory => {
  const t = String(a.type ?? "");
  switch (t) {
    case "file": {
      const fn = String(a.filename ?? "").toLowerCase();
      return fn.endsWith("claude.md") || fn.endsWith("agents.md")
        ? "claude-md"
        : "file";
    }
    case "instructions":
    case "nested_memory":
      return "claude-md";
    case "skill_listing":
    case "invoked_skills":
    case "dynamic_skill":
      return "skills";
    case "agent_listing_delta":
      return "agents";
    case "deferred_tools_delta":
      return "tools";
    case "mcp_instructions_delta":
      return "mcp";
    case "opened_file_in_ide":
    case "selected_lines_in_ide":
    case "edited_text_file":
      return "ide";
    case "image":
      return "file";
    case "compact_file_reference":
      return "file";
    case "task_reminder":
    case "hook_success":
    case "total_tokens_reminder":
    case "silent_turn_reminder":
    case "output_style":
    case "output_style_instructions":
    case "environment":
    case "date":
    case "model":
    case "session_context":
    case "auto_mode":
    case "auto_mode_exit":
    case "remote_session_change":
    case "read_truncation_notice":
    case "plan_mode":
    case "plan_mode_exit":
    case "plan_mode_reentry":
    case "todo_reminder":
    case "queued_command":
    case "date_change":
    case "ultra_effort_enter":
    case "workflow_keyword_request":
      return "reminder";
    default:
      return "other";
  }
};

/** Body of a `file` attachment: the file text when present. */
const fileBody = (c: unknown): string => {
  if (typeof c === "string") {
    return c;
  }
  const file = (c as Raw | undefined)?.file as Raw | undefined;
  return file && typeof file.content === "string" ? file.content : str(c);
};

/** CLAUDE.md files listed by an `instructions` attachment, each under its path. */
const instructionsBody = (a: Raw): string =>
  Array.isArray(a.files)
    ? (a.files as Raw[])
        .map((f) => `# ${str(f.path)}\n\n${str(f.content)}`)
        .join("\n\n")
    : str(a);

/** Rebuild a readable body from the structured payload (older transcripts). */
const structuredBody = (a: Raw): string => {
  switch (String(a.type ?? "")) {
    case "file":
      return fileBody(a.content);
    case "skill_listing":
    case "selected_lines_in_ide":
      return str(a.content);
    case "agent_listing_delta":
      return (a.addedLines as string[] | undefined)?.join("\n") ?? "";
    case "deferred_tools_delta":
      return (a.addedNames as string[] | undefined)?.join(", ") ?? "";
    case "mcp_instructions_delta":
      return (a.addedBlocks as string[] | undefined)?.join("\n\n") ?? str(a);
    case "edited_text_file":
      return str(a.snippet);
    case "hook_success":
      return `$ ${str(a.command)}\n${str(a.stdout)}${str(a.stderr)}`;
    case "instructions":
      return instructionsBody(a);
    case "nested_memory": {
      const c = a.content as Raw | undefined;
      return typeof c?.content === "string" ? c.content : str(c ?? a);
    }
    default:
      return str(a.content ?? a);
  }
};

/** The exact text the model saw, from the line's `rendered` blocks, if logged. */
const renderedBody = (o: Raw): string | undefined => {
  if (!Array.isArray(o.rendered)) {
    return;
  }
  const text = (o.rendered as Raw[])
    .map((r) => (typeof r?.content === "string" ? r.content : ""))
    .filter(Boolean)
    .join("\n");
  return text === "" ? undefined : text;
};

/** Short title for an attachment row. */
const attachmentTitle = (a: Raw): string => {
  const t = String(a.type ?? "attachment");
  if (t === "file" || t === "edited_text_file" || t === "opened_file_in_ide") {
    const fn = String(a.filename ?? a.displayPath ?? "");
    return `${t}: ${fn.split("/").pop() || fn}`;
  }
  if (t === "skill_listing") {
    return `skill_listing (${a.skillCount ?? "?"} skills)`;
  }
  if (t === "nested_memory") {
    const p = String(a.displayPath ?? a.path ?? "");
    return `nested_memory: ${p}`;
  }
  return t;
};

/** Last system prompt parts already shown, so repeat snapshots only add changes. */
export interface PromptSeen {
  cliPrefix?: string;
  prompt?: string;
  tools?: string;
}

/** Keep a section only when its text differs from the last one shown. */
const changed = (args: {
  readonly seen: PromptSeen;
  readonly key: keyof PromptSeen;
  readonly section: PromptSection | null;
}): PromptSection | null => {
  const { seen, key, section } = args;
  if (!section || seen[key] === section.text) {
    return null;
  }
  seen[key] = section.text;
  return section;
};

/** A `prompt_snapshot` attachment as a `system-prompt` event (changed parts only). */
const promptSnapshotEvent = (args: {
  readonly a: Raw;
  readonly base: AttachmentBase;
  readonly seen: PromptSeen;
}): TimelineEvent | null => {
  const { a, base, seen } = args;
  const first = seen.prompt === undefined;
  const blocks = Array.isArray(a.systemPrompt)
    ? (a.systemPrompt as unknown[])
        .filter((b): b is string => typeof b === "string")
        .filter((b) => b !== PROMPT_BOUNDARY)
    : [];
  const cli =
    typeof a.cliPrefix === "string"
      ? { name: "cli prefix", text: a.cliPrefix }
      : null;
  const prompt =
    blocks.length > 0
      ? { name: "system prompt", text: blocks.join("\n\n") }
      : null;
  return systemPromptEvent({
    index: base.index,
    ts: base.ts,
    isSidechain: base.isSidechain,
    title: first ? "System prompt" : "System prompt update",
    attachmentType: "prompt_snapshot",
    sections: [
      changed({ seen, key: "cliPrefix", section: cli }),
      changed({ seen, key: "prompt", section: prompt }),
      changed({ seen, key: "tools", section: toolsSection(a.tools) }),
    ],
  });
};

/** Position fields shared by every event an attachment line produces. */
export interface AttachmentBase {
  readonly index: number;
  readonly isSidechain: boolean;
  readonly ts: string | undefined;
}

/** Text of a queued prompt: a string, or the text parts of a block array. */
const queuedText = (prompt: unknown): string => {
  if (typeof prompt === "string") {
    return prompt;
  }
  if (!Array.isArray(prompt)) {
    return "";
  }
  return (prompt as Raw[])
    .filter((b) => b?.type === "text" && typeof b.text === "string")
    .map((b) => b.text as string)
    .join("\n");
};

/** Tokens a queued prompt adds: its rendered text, else the raw prompt. Batched
 *  prompts render nothing, since the batch head's text holds them. */
const queuedTokens = (args: {
  readonly o: Raw;
  readonly a: Raw;
  readonly text: string;
}) => {
  const shown = renderedBody(args.o);
  if (shown !== undefined) {
    return estTokens(shown);
  }
  return args.a.renderedByBatchHead === true ? 0 : estTokens(args.text);
};

/** A message the user sent while the agent was busy, as a `user-prompt` event.
 *  Null when the prompt has no text, so the line falls back to an attachment. */
const queuedPromptEvent = (args: {
  readonly o: Raw;
  readonly a: Raw;
  readonly base: AttachmentBase;
}): TimelineEvent | null => {
  const { o, a, base } = args;
  const text = queuedText(a.prompt);
  if (text === "") {
    return null;
  }
  return {
    ...promptEvent({
      o: a,
      index: base.index,
      ts: base.ts,
      isSidechain: base.isSidechain,
      text,
      title: "Queued message",
      tokensEst: queuedTokens({ o, a, text }),
    }),
    attachmentType: "queued_command",
  };
};

/** Build the event for one `attachment` line, or null when it adds nothing new. */
export const attachmentEvent = (args: {
  readonly o: Raw;
  readonly base: AttachmentBase;
  readonly seen: PromptSeen;
}): TimelineEvent | null => {
  const { o, base, seen } = args;
  const a = (o.attachment ?? {}) as Raw;
  if (a.type === "prompt_snapshot") {
    return promptSnapshotEvent({ a, base, seen });
  }
  if (a.type === "queued_command" && a.commandMode === "prompt") {
    const queued = queuedPromptEvent({ o, a, base });
    if (queued) {
      return queued;
    }
  }
  const shown = renderedBody(o);
  const body = shown ?? structuredBody(a);
  // Rendered text reached the model, so it costs tokens whatever the type.
  const metadata = shown === undefined && isMetadataOnly(a);
  return {
    index: base.index,
    kind: "attachment",
    isSidechain: base.isSidechain,
    title: attachmentTitle(a),
    preview: firstLine(body.replace(REMINDER_OPEN, "")) || String(a.type ?? ""),
    body,
    tokensEst: metadata ? 0 : estTokens(body),
    attachmentType: String(a.type ?? ""),
    ...(metadata ? {} : { loadedCategory: attachmentCategory(a) }),
    ...(base.ts === undefined ? {} : { ts: base.ts }),
  };
};
