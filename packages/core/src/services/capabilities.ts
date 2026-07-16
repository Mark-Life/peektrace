import { Context, Effect, Layer } from "effect";
import type { AgentId } from "./agents";

/** How well an agent supports a given capability. */
export type SupportLevel = "supported" | "partial" | "planned" | "unsupported";

/** Per-agent support detail for one capability. */
export interface CapabilitySupport {
  readonly level: SupportLevel;
  readonly note?: string;
}

/** A feature row in the capability matrix. `perAgent` is exhaustive over `AgentId`. */
export interface Capability {
  readonly description: string;
  readonly group: string;
  readonly id: string;
  readonly perAgent: Record<AgentId, CapabilitySupport>;
  readonly title: string;
}

/** Build a perAgent record with Claude `supported` and the rest `planned`. */
const claudeOnly = (
  notes?: Partial<Record<AgentId, string>>
): Record<AgentId, CapabilitySupport> => ({
  claude: notes?.claude
    ? { level: "supported", note: notes.claude }
    : { level: "supported" },
  codex: notes?.codex
    ? { level: "planned", note: notes.codex }
    : { level: "planned" },
  pi: notes?.pi ? { level: "planned", note: notes.pi } : { level: "planned" },
  opencode: notes?.opencode
    ? { level: "planned", note: notes.opencode }
    : { level: "planned" },
});

/** Build a uniform perAgent record at a single level. */
const uniform = (level: SupportLevel): Record<AgentId, CapabilitySupport> => ({
  claude: { level },
  codex: { level },
  pi: { level },
  opencode: { level },
});

/** The seeded capability matrix: committed surfaces plus backlog rows. */
const CAPABILITIES: readonly Capability[] = [
  {
    id: "session.view",
    group: "Sessions",
    title: "Session browser",
    description: "Browse sessions and open a transcript with full history.",
    perAgent: {
      claude: { level: "supported" },
      codex: { level: "supported" },
      pi: { level: "supported" },
      opencode: { level: "supported" },
    },
  },
  {
    id: "session.debug-context",
    group: "Sessions",
    title: "Context debug",
    description: "Reproduce the context-budget forensics at peak.",
    perAgent: {
      claude: { level: "supported" },
      codex: {
        level: "partial",
        note: "Ground-truth usage + authoritative window; no on-disk memory attribution.",
      },
      pi: {
        level: "partial",
        note: "Ground-truth usage; context window inferred from the model.",
      },
      opencode: {
        level: "partial",
        note: "Session-level ground-truth token totals; per-turn usage may be estimated and the context window is inferred from the model.",
      },
    },
  },
  {
    id: "memory.view",
    group: "Memory",
    title: "Memory explorer",
    description: "View memories across all projects with budget + link graph.",
    perAgent: claudeOnly(),
  },
  {
    id: "memory.crud",
    group: "Memory",
    title: "Memory edit",
    description: "Create, edit and delete memories with safe atomic writes.",
    perAgent: claudeOnly({ claude: "Claude markdown memories only" }),
  },
  {
    id: "mcp.dashboard",
    group: "MCP",
    title: "MCP dashboard",
    description: "Inspect configured MCP servers and their tools.",
    perAgent: uniform("planned"),
  },
  {
    id: "skills.browser",
    group: "Skills",
    title: "Skills browser",
    description: "Browse installed agent skills.",
    perAgent: uniform("planned"),
  },
  {
    id: "file-history.diff",
    group: "History",
    title: "File-history diff",
    description: "Diff file-history snapshots captured during a session.",
    perAgent: uniform("unsupported"),
  },
];

/** Service contract for the static capability matrix. */
export interface CapabilityRegistryShape {
  /** Every capability row, in matrix order. */
  readonly list: () => Effect.Effect<readonly Capability[]>;
  /** True when `agentId` fully supports `capabilityId` (used for write-gating). */
  readonly supports: (args: {
    readonly capabilityId: string;
    readonly agentId: AgentId;
  }) => Effect.Effect<boolean>;
}

/** Static, typed feature × agent support matrix. */
export class CapabilityRegistry extends Context.Tag(
  "@peektrace/CapabilityRegistry"
)<CapabilityRegistry, CapabilityRegistryShape>() {}

/** Live layer backed by the seeded matrix. */
export const CapabilityRegistryLive = Layer.succeed(CapabilityRegistry, {
  list: () =>
    Effect.succeed(CAPABILITIES).pipe(
      Effect.withSpan("CapabilityRegistry.list")
    ),
  supports: ({ capabilityId, agentId }) =>
    Effect.sync(() => {
      const cap = CAPABILITIES.find((c) => c.id === capabilityId);
      return cap?.perAgent[agentId].level === "supported";
    }).pipe(
      Effect.withSpan("CapabilityRegistry.supports", {
        attributes: { capabilityId, agentId },
      })
    ),
});

/** Exported for tests: the raw seeded matrix and the agent id list. */
export const seededCapabilities = CAPABILITIES;
export { AGENT_IDS } from "./agents";
