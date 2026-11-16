/** Memory editing support from the live matrix: the Claude write gate and a
 * one-line summary across agents. */
import type {
  AgentId,
  Capability,
  SupportLevel,
} from "@workspace/view-core/api-types.gen";
import { AGENT_IDS } from "@workspace/view-core/constants.gen";

import { AGENT_LABEL } from "./agent-label";

/** The capability id that gates all memory writes. */
const MEMORY_CRUD = "memory.crud";

const crudCap = (caps: readonly Capability[]) =>
  caps.find((c) => c.id === MEMORY_CRUD);

/** Display order and wording for the non-supported levels. */
const OTHER_LEVELS: readonly {
  level: SupportLevel;
  label: string;
}[] = [
  { label: "Partial", level: "partial" },
  { label: "Planned", level: "planned" },
  { label: "Not supported", level: "unsupported" },
];

/**
 * "Memory editing: Claude. Planned: Codex, Pi, OpenCode." Agents with no cell
 * for `memory.crud` count as not supported.
 */
export const memorySupportSummary = (caps: readonly Capability[]) => {
  const cap = crudCap(caps);

  const levelOf = (agent: AgentId) =>
    cap?.perAgent[agent]?.level ?? "unsupported";

  const namesAt = (level: SupportLevel) =>
    AGENT_IDS.filter((a) => levelOf(a) === level)
      .map((a) => AGENT_LABEL[a])
      .join(", ");

  const supported = namesAt("supported");

  const others = OTHER_LEVELS.flatMap(({ level, label }) => {
    const names = namesAt(level);

    return names.length > 0 ? [` ${label}: ${names}.`] : [];
  }).join("");

  return `Memory editing: ${supported.length > 0 ? supported : "no agent yet"}.${others}`;
};

/**
 * Whether memories can be written. A read-only server blocks first; otherwise
 * only Claude has a memory folder, so the Claude cell decides and its note
 * explains a blocked verdict.
 */
export const memoryCrudVerdict = ({
  caps,
  readOnly,
}: {
  readonly caps: readonly Capability[];
  readonly readOnly: boolean;
}) => {
  const support = crudCap(caps)?.perAgent.claude;
  const capable = support?.level === "supported";
  const blocker = capable ? null : ("capability" as const);

  return {
    /** What stops writes, or null when memories can be written. */
    blockedBy: readOnly ? "read-only" : blocker,
    level: support?.level ?? "unsupported",
    note: support?.note,
  } as const;
};
