/** The capability matrix's pure rules: columns, level styles and glosses,
 * legend order, grouping, and a cell's note. */
import type { Capability } from "./api-types.gen";

/** A per-agent support cell, narrowed from the contract `Capability`. */
export type Support = Capability["perAgent"][keyof Capability["perAgent"]];

/** Column order for the matrix (also the agent-id set). */
export const AGENT_COLUMNS = [
  { id: "claude", label: "Claude" },
  { id: "codex", label: "Codex" },
  { id: "pi", label: "Pi" },
  { id: "opencode", label: "OpenCode" },
] as const;

/** Tailwind classes per support level. */
export const LEVEL_STYLES: Record<Support["level"], string> = {
  supported: "bg-emerald-500/15 text-emerald-400 border-emerald-500/30",
  partial: "bg-amber-500/15 text-amber-400 border-amber-500/30",
  planned: "bg-sky-500/15 text-sky-400 border-sky-500/30",
  unsupported: "bg-muted text-muted-foreground border-border",
};

/** Plain-language gloss per support level (shown in the legend + drawer). */
export const LEVEL_GLOSS: Record<Support["level"], string> = {
  supported: "Built and working today.",
  partial: "Partially built — some paths work.",
  planned: "On the roadmap, not yet built.",
  unsupported: "Not applicable / no plan.",
};

/** Legend order: the `LEVEL_STYLES` keys. */
export const LEVELS = Object.keys(LEVEL_STYLES) as Support["level"][];

/** Group capabilities by their `group` field, preserving first-seen order. */
export const groupBy = (caps: readonly Capability[]) => {
  const groups = new Map<string, Capability[]>();
  for (const cap of caps) {
    const bucket = groups.get(cap.group) ?? [];
    bucket.push(cap);
    groups.set(cap.group, bucket);
  }
  return [...groups.entries()];
};

/** A cell's note, else the gloss of its level. */
export const cellNote = (support: Support) =>
  support.note ?? LEVEL_GLOSS[support.level];
