/** Codex rollout records that load context into the model window.
 *
 * `session_meta` carries the base system prompt (`base_instructions`) and any
 * app tool definitions (`dynamic_tools`). Developer messages and user-role
 * messages bundle several injected parts (AGENTS.md, environment, plugins,
 * skills); each part is classified on its own so a real prompt is never
 * mislabelled and each injection gets its own chip.
 */
import type { LoadedCategory, TimelineEvent } from "../schema";
import { systemPromptEvent, toolsSection } from "../system-prompt";
import { estTokens, firstLine } from "../tokens";

type Raw = Record<string, unknown>;

/** Position fields shared by every event built here. */
export interface CodexBase {
  readonly index: number;
  readonly ts?: string;
}

const asObj = (v: unknown): Raw | undefined =>
  v && typeof v === "object" && !Array.isArray(v) ? (v as Raw) : undefined;

/** The non-empty `text` of each part of a `message.content` array. */
const partTexts = (content: unknown): string[] =>
  Array.isArray(content)
    ? content
        .map((part) => asObj(part)?.text)
        .filter((t): t is string => typeof t === "string" && t.trim() !== "")
    : [];

/** The base system prompt + dynamic tools from a `session_meta` payload. */
export const sessionMetaEvent = (
  payload: Raw,
  base: CodexBase
): TimelineEvent | null => {
  const instructions = asObj(payload.base_instructions)?.text;
  return systemPromptEvent({
    index: base.index,
    ts: base.ts,
    title: "System prompt",
    attachmentType: "base_instructions",
    sections: [
      typeof instructions === "string"
        ? { name: "base instructions", text: instructions }
        : null,
      toolsSection(payload.dynamic_tools, "dynamic tools"),
    ],
  });
};

interface Injection {
  readonly category: LoadedCategory;
  readonly type: string;
}

/** Leading markers of harness-injected user-role parts. */
const USER_INJECTIONS: readonly (readonly [string, Injection])[] = [
  ["<environment_context>", { type: "environment_context", category: "other" }],
  ["<user_instructions>", { type: "user_instructions", category: "claude-md" }],
  ["# AGENTS.md instructions", { type: "agents_md", category: "claude-md" }],
  ["<recommended_plugins>", { type: "recommended_plugins", category: "other" }],
  ["<skill>", { type: "skill", category: "skills" }],
  ["<turn_aborted>", { type: "turn_aborted", category: "other" }],
];

/** Which injection a user-role part is, or undefined for real user text. */
const userInjection = (text: string) => {
  const t = text.trimStart();
  return USER_INJECTIONS.find(([marker]) => t.startsWith(marker))?.[1];
};

const TAG = /^<([a-z][\w -]*)>/i;

/** Classify a developer part by its leading `<tag>`. */
const developerInjection = (text: string): Injection => {
  const tag = TAG.exec(text.trimStart())?.[1];
  if (!tag) {
    return { type: "developer_instructions", category: "other" };
  }
  const type = tag.replace(/[\s-]+/g, "_");
  return { type, category: type.startsWith("skills") ? "skills" : "other" };
};

/** A context-injection attachment for one part of a message. */
const injectionEvent = (args: {
  readonly base: CodexBase;
  readonly text: string;
  readonly injection: Injection;
}): TimelineEvent => {
  const { base, text, injection } = args;
  return {
    ...base,
    kind: "attachment",
    title: injection.type,
    attachmentType: injection.type,
    loadedCategory: injection.category,
    preview: firstLine(text),
    body: text,
    tokensEst: estTokens(text),
  };
};

/** Events for a `response_item` message: assistant text, injected parts, or a prompt. */
export const messageEvents = (
  payload: Raw,
  base: CodexBase
): TimelineEvent[] => {
  const role = String(payload.role ?? "");
  const parts = partTexts(payload.content);
  if (role === "assistant") {
    const body = parts.join("\n");
    return [
      {
        ...base,
        kind: "assistant-text",
        title: "Assistant",
        preview: firstLine(body),
        body,
        tokensEst: estTokens(body),
      },
    ];
  }
  if (role === "developer") {
    return parts.map((text) =>
      injectionEvent({ base, text, injection: developerInjection(text) })
    );
  }
  const injected = parts.flatMap((text) => {
    const injection = userInjection(text);
    return injection ? [injectionEvent({ base, text, injection })] : [];
  });
  const prompt = parts.filter((text) => !userInjection(text)).join("\n");
  if (prompt === "" && injected.length > 0) {
    return injected;
  }
  return [
    ...injected,
    {
      ...base,
      kind: "user-prompt",
      title: "User prompt",
      preview: firstLine(prompt),
      body: prompt,
      tokensEst: estTokens(prompt),
    },
  ];
};

/** An inter-agent message fed to this agent's model. */
export const agentMessageEvent = (
  payload: Raw,
  base: CodexBase
): TimelineEvent | null => {
  const body = partTexts(payload.content).join("\n");
  if (body === "") {
    return null;
  }
  const author = typeof payload.author === "string" ? payload.author : "agent";
  return {
    ...base,
    kind: "attachment",
    title: `message from ${author}`,
    attachmentType: "agent_message",
    loadedCategory: "other",
    preview: firstLine(body),
    body,
    tokensEst: estTokens(body),
  };
};
