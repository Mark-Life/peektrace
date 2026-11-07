/** Pure helpers that link Claude Code workflow runs to their subagents and calls. */

const AGENT_PREFIX = "agent-";
const RUN_ID_KEY = '"runId"';

/** One agent of a workflow run, from `workflowProgress`. */
interface WorkflowAgentPhase {
  readonly phaseIndex?: number;
  readonly phaseTitle?: string;
  /** Epoch ms. */
  readonly startedAt?: number;
}

/** A Workflow tool call that started or resumed a run. */
export interface WorkflowCall {
  /** Epoch ms of its tool_result; the result returns as soon as the run starts. */
  readonly at?: number;
  readonly callId: string;
}

/** A workflow run as read from `<session>/workflows/<runId>.json`. */
export interface WorkflowRun {
  /** Keyed by stub id (`agent-<id>`). */
  readonly agents: ReadonlyMap<string, WorkflowAgentPhase>;
  readonly runId: string;
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

const toEpochMs = (v: unknown) => {
  const ms = typeof v === "string" ? Date.parse(v) : Number.NaN;
  return Number.isNaN(ms) ? undefined : ms;
};

/**
 * Map each workflow run id to the Workflow calls that started or resumed it,
 * in transcript order, from a raw Claude transcript.
 */
export const workflowCallsByRun = (text: string) => {
  const calls = new Map<string, WorkflowCall[]>();
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
    if (typeof runId !== "string" || !callId) {
      continue;
    }
    const list = calls.get(runId) ?? [];
    if (!list.some((c) => c.callId === callId)) {
      const at = toEpochMs(entry.timestamp);
      list.push(at === undefined ? { callId } : { callId, at });
      calls.set(runId, list);
    }
  }
  return calls;
};

/**
 * The call an agent belongs to: the last one that returned before the agent
 * started. Falls back to the first call when the start time is unknown.
 */
export const pickWorkflowCall = (
  calls: readonly WorkflowCall[],
  startedAt?: number
) => {
  if (startedAt === undefined) {
    return calls[0]?.callId;
  }
  const before = calls.filter((c) => c.at !== undefined && c.at <= startedAt);
  return (before.at(-1) ?? calls[0])?.callId;
};

const toAgentPhase = (item: Record<string, unknown>): WorkflowAgentPhase => ({
  ...(typeof item.phaseIndex === "number"
    ? { phaseIndex: item.phaseIndex }
    : {}),
  ...(typeof item.phaseTitle === "string"
    ? { phaseTitle: item.phaseTitle }
    : {}),
  ...(typeof item.startedAt === "number" ? { startedAt: item.startedAt } : {}),
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
      !agents.has(toAgentRef(item.agentId))
    ) {
      agents.set(toAgentRef(item.agentId), toAgentPhase(item));
    }
  }
  return { agents };
};
