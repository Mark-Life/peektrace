/** Pi entries that reach the model beyond plain user/assistant/tool turns:
 * the system message (named sections + tool definitions), compaction and branch
 * summaries, extension-injected custom messages, and `!` bash executions.
 * Shapes follow pi-coding-agent's `session-manager` and `messages` types.
 */

import { imageMarker, isImageBlock } from "../images";
import type { TimelineEvent } from "../schema";
import {
  type PromptSection,
  systemPromptEvent,
  toolsSection,
} from "../system-prompt";
import { estTokens, firstLine } from "../tokens";

type Raw = Record<string, unknown>;

const COMPACTION_PREFIX =
  "The conversation history before this point was compacted into the following summary:\n\n<summary>\n";
const COMPACTION_SUFFIX = "\n</summary>";
const BRANCH_PREFIX =
  "The following is a summary of a branch that this conversation came back from:\n\n<summary>\n";
const BRANCH_SUFFIX = "</summary>";

/** Position fields every event built here carries. */
export interface PiBase {
  readonly index: number;
  readonly ts: string | undefined;
}

const withTs = (ts: string | undefined) => (ts === undefined ? {} : { ts });

/** Join text blocks of a Pi content value; images become a short marker. */
export const piText = (content: unknown): string => {
  if (typeof content === "string") {
    return content;
  }
  if (!Array.isArray(content)) {
    return "";
  }
  return (content as Raw[])
    .map((b) => {
      if (typeof b?.text === "string") {
        return b.text;
      }
      return b?.type === "image"
        ? `[image: ${String(b.mimeType ?? "image")}]`
        : "";
    })
    .filter(Boolean)
    .join("\n");
};

/** Coerce an unknown value into a display string (JSON for non-strings). */
export const str = (v: unknown): string => {
  if (typeof v === "string") {
    return v;
  }
  return v == null ? "" : JSON.stringify(v);
};

/** Join a tool result's `{type:"text", text}` blocks into one body. */
export const piToolResultText = (content: unknown): string => {
  if (typeof content === "string") {
    return content;
  }
  if (!Array.isArray(content)) {
    return str(content);
  }
  return (content as Raw[])
    .map((b) => {
      if (isImageBlock(b)) {
        return imageMarker(b);
      }
      return typeof b?.text === "string" ? b.text : "";
    })
    .join("");
};

/** Sections of a Pi system message: `content`, named sections, tool changes. */
const systemSections = (msg: Raw): (PromptSection | null)[] => {
  const content = piText(msg.content);
  const named = Object.entries(
    (msg.sections ?? {}) as Record<string, unknown>
  ).map(
    ([name, value]): PromptSection =>
      typeof value === "string"
        ? { name, text: value }
        : { name, text: `(section "${name}" removed)` }
  );
  const removed = Array.isArray(msg.toolsRemoved)
    ? (msg.toolsRemoved as Raw[])
        .map((t) => String(t?.name ?? ""))
        .filter(Boolean)
    : [];
  return [
    content ? { name: "content", text: content } : null,
    ...named,
    toolsSection(msg.toolsAdded, removed.length > 0 ? "tools added" : "tools"),
    removed.length > 0
      ? { name: "tools removed", text: removed.join(", ") }
      : null,
  ];
};

/** A Pi system message; the first is the prompt, later ones patch it. */
export const piSystemEvent = (args: {
  readonly msg: Raw;
  readonly base: PiBase;
  readonly first: boolean;
}): TimelineEvent | null =>
  systemPromptEvent({
    ...args.base,
    title: args.first ? "System prompt" : "System prompt update",
    attachmentType: "system_message",
    sections: systemSections(args.msg),
  });

/** A summary entry as the model sees it (prefix + summary + suffix). */
const summaryEvent = (args: {
  readonly base: PiBase;
  readonly kind: "compaction" | "summary";
  readonly title: string;
  readonly text: string;
}): TimelineEvent => ({
  index: args.base.index,
  kind: args.kind,
  title: args.title,
  preview: firstLine(args.text.replace(/<\/?summary>/g, "")),
  body: args.text,
  tokensEst: estTokens(args.text),
  ...withTs(args.base.ts),
});

/** A compaction summary (from a `compaction` entry or a `compactionSummary` message). */
export const piCompactionEvent = (base: PiBase, summary: unknown) =>
  summaryEvent({
    base,
    kind: "compaction",
    title: "Context compaction (summary)",
    text: `${COMPACTION_PREFIX}${String(summary ?? "")}${COMPACTION_SUFFIX}`,
  });

/** A branch summary (from a `branch_summary` entry or a `branchSummary` message). */
export const piBranchSummaryEvent = (base: PiBase, summary: unknown) =>
  summaryEvent({
    base,
    kind: "summary",
    title: "Branch summary",
    text: `${BRANCH_PREFIX}${String(summary ?? "")}${BRANCH_SUFFIX}`,
  });

/** An extension-injected message; sent to the model as a user turn even when hidden. */
export const piCustomEvent = (
  base: PiBase,
  entry: Raw
): TimelineEvent | null => {
  const body = piText(entry.content);
  if (!body) {
    return null;
  }
  const customType =
    typeof entry.customType === "string" ? entry.customType : "custom";
  return {
    index: base.index,
    kind: "attachment",
    title: customType,
    preview: firstLine(body),
    body,
    tokensEst: estTokens(body),
    attachmentType: "custom_message",
    loadedCategory: "other",
    ...withTs(base.ts),
  };
};

/** Text Pi sends for a user `!` command (mirrors `bashExecutionToText`). */
const bashText = (msg: Raw): string => {
  const output = typeof msg.output === "string" ? msg.output : "";
  const parts = [
    `Ran \`${String(msg.command ?? "")}\`\n${output ? `\`\`\`\n${output}\n\`\`\`` : "(no output)"}`,
  ];
  if (msg.cancelled === true) {
    parts.push("(command cancelled)");
  } else if (typeof msg.exitCode === "number" && msg.exitCode !== 0) {
    parts.push(`Command exited with code ${msg.exitCode}`);
  }
  if (msg.truncated === true && typeof msg.fullOutputPath === "string") {
    parts.push(`[Output truncated. Full output: ${msg.fullOutputPath}]`);
  }
  return parts.join("\n\n");
};

/** A user-run shell command, or null when it was kept out of context. */
export const piBashEvent = (base: PiBase, msg: Raw): TimelineEvent | null => {
  if (msg.excludeFromContext === true) {
    return null;
  }
  const body = bashText(msg);
  return {
    index: base.index,
    kind: "attachment",
    title: `$ ${firstLine(String(msg.command ?? ""))}`,
    preview: firstLine(String(msg.command ?? "")),
    body,
    tokensEst: estTokens(body),
    attachmentType: "bash_execution",
    loadedCategory: "other",
    ...withTs(base.ts),
  };
};
