/** Server tool results (web search, web fetch, advisor) inside a Claude assistant message. */
import type { TimelineEvent } from "./schema";
import { estTokens, firstLine } from "./tokens";
import { opt } from "./usage";

const TOOL_RESULT_SUFFIX = /_tool_result$/;

/** Any value as display text: strings as-is, nullish as empty, the rest as JSON. */
export const str = (v: unknown): string => {
  if (typeof v === "string") {
    return v;
  }
  return v == null ? "" : JSON.stringify(v);
};

/** Position fields every event from one assistant line carries. */
export interface AssistantBase {
  readonly index: number;
  readonly isSidechain: boolean;
  readonly requestId: string;
  readonly ts?: string;
}

export type Block = Record<string, unknown>;

/** The `tool_use_id` a result block answers, spread-ready. */
const answers = (block: Block) =>
  opt(
    "toolUseId",
    typeof block.tool_use_id === "string" ? block.tool_use_id : undefined
  );

export const advisorResultEvent = (
  base: AssistantBase,
  block: Block
): TimelineEvent => {
  const result = (block.content ?? {}) as Block;
  const redacted = result.type === "advisor_redacted_result";
  const text = redacted
    ? "(advisor response encrypted, not stored in transcript)"
    : str(result.text ?? result);
  return {
    ...base,
    kind: "tool-result",
    title: "advisor result",
    preview: firstLine(text),
    body: text,
    tokensEst: redacted ? 0 : estTokens(text),
    serverTool: true,
    ...answers(block),
  };
};

/** One web search hit as a Markdown list item. */
const searchHit = (r: Block) => {
  const title = str(r.title) || str(r.url);
  const age = typeof r.page_age === "string" ? ` · ${r.page_age}` : "";
  return `- [${title}](${str(r.url)})${age}`;
};

/** Text of content blocks, joined; anything else as JSON. */
const blockText = (content: unknown) =>
  Array.isArray(content)
    ? (content as Block[])
        .map((b) => (typeof b?.text === "string" ? b.text : str(b)))
        .join("\n\n")
    : str(content);

/** Body of a server tool result: hits, fetched text, or the error code. */
const serverResultBody = (block: Block) => {
  const content = block.content;
  const c = (content ?? {}) as Block;
  if (typeof c.error_code === "string") {
    return { text: `error: ${c.error_code}`, isError: true };
  }
  if (
    block.type !== "web_search_tool_result" &&
    block.type !== "web_fetch_tool_result"
  ) {
    return { text: blockText(content), isError: false };
  }
  if (Array.isArray(content)) {
    return {
      text: (content as Block[]).map(searchHit).join("\n"),
      isError: false,
    };
  }
  const doc = (c.content ?? {}) as Block;
  const source = (doc.source ?? {}) as Block;
  const text =
    typeof source.data === "string" && source.type === "text"
      ? source.data
      : "";
  const head = [str(c.url), typeof doc.title === "string" ? doc.title : ""]
    .filter(Boolean)
    .join(" — ");
  return { text: text ? `${head}\n\n${text}` : head, isError: false };
};

/**
 * A server tool's result (`web_search_tool_result`, `web_fetch_tool_result`,
 * …), which the API returns inside the assistant message. Search hits carry
 * their page text encrypted, so only titles and URLs are readable.
 */
export const serverResultEvent = (
  base: AssistantBase,
  block: Block
): TimelineEvent => {
  const name = String(block.type).replace(TOOL_RESULT_SUFFIX, "");
  const { text, isError } = serverResultBody(block);
  return {
    ...base,
    kind: "tool-result",
    title: `${name} result${isError ? " (error)" : ""}`,
    preview: firstLine(text),
    body: text,
    tokensEst: estTokens(text),
    serverTool: true,
    ...opt("isError", isError ? true : undefined),
    ...answers(block),
  };
};
