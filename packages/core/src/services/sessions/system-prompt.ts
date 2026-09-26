/** Shared builder for `system-prompt` timeline events.
 *
 * Each agent logs its system prompt differently (Pi named sections, Codex
 * `base_instructions`, Claude `prompt_snapshot` blocks), but the history shows
 * them the same way: one collapsible entry whose body lists every section under
 * its own heading, tool definitions included.
 */
import type { TimelineEvent } from "./schema";
import { estTokens, firstLine } from "./tokens";

/** One named part of a system prompt. */
export interface PromptSection {
  readonly name: string;
  readonly text: string;
}

const asObj = (v: unknown): Record<string, unknown> | undefined =>
  v && typeof v === "object" && !Array.isArray(v)
    ? (v as Record<string, unknown>)
    : undefined;

/** Render one tool definition the way the model receives it: name, description, schema. */
const renderTool = (tool: unknown): string => {
  const o = asObj(tool) ?? {};
  const { name, description, ...rest } = o;
  const head = `### ${typeof name === "string" ? name : "tool"}`;
  const desc = typeof description === "string" ? description : "";
  const schema = Object.keys(rest).length ? JSON.stringify(rest, null, 2) : "";
  return [head, desc, schema].filter(Boolean).join("\n\n");
};

/** A `tools (N)` section from raw tool definitions, or null when there are none. */
export const toolsSection = (
  tools: unknown,
  label = "tools"
): PromptSection | null =>
  Array.isArray(tools) && tools.length > 0
    ? {
        name: `${label} (${tools.length})`,
        text: tools.map(renderTool).join("\n\n"),
      }
    : null;

/** Drop nulls and blank sections. */
export const presentSections = (
  sections: readonly (PromptSection | null)[]
): PromptSection[] =>
  sections.filter(
    (s): s is PromptSection => s !== null && s.text.trim() !== ""
  );

/** Body text: a lone section is shown bare, several get `## name` headings. */
export const renderSections = (sections: readonly PromptSection[]) =>
  sections.length === 1
    ? (sections[0]?.text ?? "")
    : sections.map((s) => `## ${s.name}\n\n${s.text}`).join("\n\n");

/** Build a `system-prompt` event, or null when no section carries text. */
export const systemPromptEvent = (args: {
  readonly index: number;
  readonly ts: string | undefined;
  readonly title: string;
  readonly sections: readonly (PromptSection | null)[];
  readonly attachmentType: string;
  readonly isSidechain?: boolean;
}): TimelineEvent | null => {
  const sections = presentSections(args.sections);
  if (sections.length === 0) {
    return null;
  }
  const body = renderSections(sections);
  return {
    index: args.index,
    kind: "system-prompt",
    title: args.title,
    preview:
      sections.length === 1
        ? firstLine(body)
        : sections.map((s) => s.name).join(" · "),
    body,
    tokensEst: estTokens(body),
    attachmentType: args.attachmentType,
    ...(args.ts === undefined ? {} : { ts: args.ts }),
    ...(args.isSidechain === undefined
      ? {}
      : { isSidechain: args.isSidechain }),
  };
};
