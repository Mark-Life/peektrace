/** Claude Code `attachment` lines: context the harness injects into the window.
 *
 * Newer transcripts carry the exact model-facing text on `rendered`; older ones
 * only have the structured `attachment` payload, so the body is rebuilt from it.
 * `prompt_snapshot` attachments hold the system prompt and tool definitions and
 * become `system-prompt` events instead of plain attachments.
 */
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
    case "task_reminder":
    case "hook_success":
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
  const body = renderedBody(o) ?? structuredBody(a);
  return {
    index: base.index,
    kind: "attachment",
    isSidechain: base.isSidechain,
    title: attachmentTitle(a),
    preview: firstLine(body.replace(REMINDER_OPEN, "")) || String(a.type ?? ""),
    body,
    tokensEst: estTokens(body),
    attachmentType: String(a.type ?? ""),
    loadedCategory: attachmentCategory(a),
    ...(base.ts === undefined ? {} : { ts: base.ts }),
  };
};
