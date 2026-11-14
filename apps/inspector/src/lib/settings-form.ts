/** The settings editor's pure rules: rows from the loaded settings and back,
 * and the scanned default folder per agent. */
import { AGENT_IDS, type AgentId } from "@workspace/core/services/agent-id";
import type { PeektraceSettings } from "@workspace/core/services/settings";
import type { ServerInfo } from "./api-types.gen";

/** One editable extra-root row. */
export interface Row {
  readonly label: string;
  readonly path: string;
}

export type RowsByAgent = Record<AgentId, Row[]>;

/** Seed the editable per-agent rows from the loaded settings. */
export const seedRows = (settings: PeektraceSettings): RowsByAgent =>
  Object.fromEntries(
    AGENT_IDS.map((agent) => [
      agent,
      (settings.roots?.[agent] ?? []).map((entry) => ({
        path: entry.path,
        label: entry.label ?? "",
      })),
    ])
  ) as RowsByAgent;

/** Collapse the editable rows back into a `PeektraceSettings` (drop empties). */
export const assemble = (rows: RowsByAgent): PeektraceSettings => {
  const roots: Partial<Record<AgentId, { path: string; label?: string }[]>> =
    {};
  for (const agent of AGENT_IDS) {
    const cleaned = rows[agent]
      .map((r) => {
        const path = r.path.trim();
        const label = r.label.trim();
        return label ? { path, label } : { path };
      })
      .filter((r) => r.path !== "");
    if (cleaned.length > 0) {
      roots[agent] = cleaned;
    }
  }
  return Object.keys(roots).length > 0 ? { roots } : {};
};

/** The scanned default folder per agent, from `server.info`. */
export const defaultRoots = (info: ServerInfo) =>
  new Map(
    info.agents.flatMap(({ agent, roots }) => {
      const root = roots.find((r) => r.isDefault);
      return root ? [[agent, root] as const] : [];
    })
  );
