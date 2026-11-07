/** Pure helpers that link Claude Code workflow runs to their subagents and calls. */

const AGENT_PREFIX = "agent-";
const RUN_ID_KEY = '"runId"';

/** Phase info for one agent of a workflow run, from `workflowProgress`. */
export interface WorkflowAgentPhase {
  readonly phaseIndex?: number;
  readonly phaseTitle?: string;
}

/** A workflow run as read from `<session>/workflows/<runId>.json`. */
export interface WorkflowRun {
  /** Keyed by bare agent id (no `agent-` prefix). */
  readonly agents: ReadonlyMap<string, WorkflowAgentPhase>;
  readonly runId: string;
  readonly workflowName?: string;
}

const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

const safeParse = (raw: string): unknown => {
  try {
    return JSON.parse(raw);
  } catch {
    return;
  }
};

/** Normalize an agent id to the stub form `agent-<id>`. */
export const toAgentRef = (v: string) =>
  v.startsWith(AGENT_PREFIX) ? v : `${AGENT_PREFIX}${v}`;

/** Strip the `agent-` prefix from a stub id. */
export const toBareAgentId = (v: string) =>
  v.startsWith(AGENT_PREFIX) ? v.slice(AGENT_PREFIX.length) : v;

const toolResultId = (entry: Record<string, unknown>) => {
  const message = entry.message;
  if (!(isRecord(message) && Array.isArray(message.content))) {
    return;
  }
  const block = message.content.find(
    (b): b is Record<string, unknown> =>
      isRecord(b) &&
      b.type === "tool_result" &&
      typeof b.tool_use_id === "string"
  );
  return block?.tool_use_id as string | undefined;
};

/**
 * Map each workflow run id to the Workflow tool call that started it, from a
 * raw Claude transcript. First match wins: one run can show up in many results.
 */
export const workflowCallsByRun = (text: string) => {
  const calls = new Map<string, string>();
  for (const raw of text.split("\n")) {
    if (!raw.includes(RUN_ID_KEY)) {
      continue;
    }
    const entry = safeParse(raw);
    if (!(isRecord(entry) && isRecord(entry.toolUseResult))) {
      continue;
    }
    const runId = entry.toolUseResult.runId;
    const callId = toolResultId(entry);
    if (typeof runId === "string" && callId && !calls.has(runId)) {
      calls.set(runId, callId);
    }
  }
  return calls;
};

const toAgentPhase = (item: Record<string, unknown>): WorkflowAgentPhase => ({
  ...(typeof item.phaseIndex === "number"
    ? { phaseIndex: item.phaseIndex }
    : {}),
  ...(typeof item.phaseTitle === "string"
    ? { phaseTitle: item.phaseTitle }
    : {}),
});

/** Parse a workflow run json. Bad JSON or missing keys give an empty result. */
export const parseWorkflowRun = (raw: string): Omit<WorkflowRun, "runId"> => {
  const doc = safeParse(raw);
  const agents = new Map<string, WorkflowAgentPhase>();
  if (!isRecord(doc)) {
    return { agents };
  }
  const progress = Array.isArray(doc.workflowProgress)
    ? doc.workflowProgress
    : [];
  for (const item of progress) {
    if (
      isRecord(item) &&
      item.type === "workflow_agent" &&
      typeof item.agentId === "string" &&
      !agents.has(item.agentId)
    ) {
      agents.set(item.agentId, toAgentPhase(item));
    }
  }
  return {
    agents,
    ...(typeof doc.workflowName === "string"
      ? { workflowName: doc.workflowName }
      : {}),
  };
};
