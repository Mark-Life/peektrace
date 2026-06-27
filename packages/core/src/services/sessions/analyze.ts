/**
 * Turn a ParsedSession into an AnalyzedSession: context-budget attribution,
 * per-turn snapshots, residual, dumb-zone, biggest items.
 *
 * Key insight: the real peak context (usage metadata) is far larger than the
 * visible transcript because retained assistant *thinking* is stored as empty
 * strings. We recover it from ground-truth `output_tokens` minus visible
 * text/tool_use, attributing ~90% of context in heavy-reasoning sessions.
 */
import type {
  AnalyzedSession,
  BudgetKey,
  BudgetSlice,
  BudgetSlices,
  OnDiskContextFile,
  ParsedSession,
  TimelineEvent,
  TurnSnapshot,
} from "./schema";

const DEFAULT_WINDOW = 1_000_000;
const DEFAULT_DUMB_ZONE = 0.4;
const BIGGEST_ITEMS_LIMIT = 40;

export interface AnalyzeOptions {
  /** degradation threshold as a fraction of the window (default 0.40). */
  readonly dumbZoneFraction?: number;
  readonly onDiskContextFiles?: readonly OnDiskContextFile[];
  /** explicit context-window override (tokens). */
  readonly window?: number;
}

interface CatMeta {
  readonly color: string;
  readonly estimated: boolean;
  readonly label: string;
  readonly note?: string;
  readonly short: string;
}

/** Display metadata per context-budget category (label, color, estimate flag). */
const CAT_META: Record<BudgetKey, CatMeta> = {
  system_tools: {
    label: "System + tool definitions",
    short: "System",
    color: "#6e7681",
    estimated: true,
    note: "inferred floor — not in transcript",
  },
  listings: {
    label: "Skill / agent / tool listings",
    short: "Listings",
    color: "#58a6ff",
    estimated: true,
  },
  memory: {
    label: "CLAUDE.md / AGENTS.md (in transcript)",
    short: "Memory",
    color: "#bc8cff",
    estimated: true,
  },
  files: {
    label: "Opened files / plans",
    short: "Files",
    color: "#39c5cf",
    estimated: true,
  },
  prompts: {
    label: "User prompts",
    short: "Prompts",
    color: "#3fb950",
    estimated: true,
  },
  tool_results: {
    label: "Tool results",
    short: "Tool results",
    color: "#f0883e",
    estimated: true,
  },
  assistant_text: {
    label: "Assistant text + tool calls",
    short: "Assistant",
    color: "#d29922",
    estimated: true,
  },
  thinking: {
    label: "Assistant thinking (retained)",
    short: "Thinking",
    color: "#db61a2",
    estimated: true,
    note: "derived from output_tokens − visible text",
  },
  other: {
    label: "Other injected (reminders, hooks)",
    short: "Other",
    color: "#8a929e",
    estimated: true,
  },
  unattributed: {
    label: "Tool schemas & overhead (not in transcript)",
    short: "Overhead",
    color: "#484f58",
    estimated: true,
    note: "real context − everything attributable above",
  },
};

/** Mutable per-category accumulator (the schema type is readonly). */
type MutSlices = Record<BudgetKey, number>;

const zeroSlices = (): MutSlices => ({
  system_tools: 0,
  listings: 0,
  memory: 0,
  files: 0,
  prompts: 0,
  tool_results: 0,
  assistant_text: 0,
  thinking: 0,
  other: 0,
  unattributed: 0,
});

interface PeakStats {
  readonly peakCacheReadTokens: number;
  readonly peakContextTokens: number;
  readonly peakTurnIndex: number;
  readonly totalOutputTokens: number;
}

/** Scan turns for the peak-context turn and totals. */
const computePeak = (turns: ParsedSession["turns"]): PeakStats => {
  let peakContextTokens = 0;
  let peakTurnIndex = 0;
  let peakCacheReadTokens = 0;
  let totalOutputTokens = 0;
  turns.forEach((t, i) => {
    totalOutputTokens += t.outputTokens;
    if (t.contextTokens > peakContextTokens) {
      peakContextTokens = t.contextTokens;
      peakTurnIndex = i;
      peakCacheReadTokens = t.cacheReadTokens;
    }
  });
  return {
    peakContextTokens,
    peakTurnIndex,
    peakCacheReadTokens,
    totalOutputTokens,
  };
};

/** Fold one event's tokens into the running category accumulator. */
const foldContent = (args: {
  readonly cat: MutSlices;
  readonly loadedCategory: string | undefined;
  readonly kind: string;
  readonly tokens: number;
}) => {
  const { cat, loadedCategory, kind, tokens } = args;
  switch (loadedCategory) {
    case "claude-md":
      cat.memory += tokens;
      return;
    case "skills":
    case "agents":
    case "tools":
    case "mcp":
      cat.listings += tokens;
      return;
    case "file":
    case "ide":
      cat.files += tokens;
      return;
    case "reminder":
    case "other":
      cat.other += tokens;
      return;
    default:
      break;
  }
  if (kind === "tool-result") {
    cat.tool_results += tokens;
  } else if (kind === "user-prompt" || kind === "compaction") {
    cat.prompts += tokens;
  } else if (kind === "summary") {
    cat.other += tokens;
  }
};

/** Build per-turn slices that always sum to `ctx` with all values >= 0. */
const makeSlices = (args: {
  readonly ctx: number;
  readonly cat: MutSlices;
  readonly retainedThinking: number;
}): BudgetSlices => {
  const { ctx, cat, retainedThinking } = args;
  const floor = Math.min(cat.system_tools, ctx);
  const rest = Math.max(0, ctx - floor);
  const est = {
    thinking: retainedThinking,
    assistant_text: cat.assistant_text,
    tool_results: cat.tool_results,
    prompts: cat.prompts,
    memory: cat.memory,
    listings: cat.listings,
    files: cat.files,
    other: cat.other,
  };
  const estSum = Object.values(est).reduce((a, b) => a + b, 0);
  const scale = estSum > rest && estSum > 0 ? rest / estSum : 1;
  return {
    system_tools: floor,
    thinking: est.thinking * scale,
    assistant_text: est.assistant_text * scale,
    tool_results: est.tool_results * scale,
    prompts: est.prompts * scale,
    memory: est.memory * scale,
    listings: est.listings * scale,
    files: est.files * scale,
    other: est.other * scale,
    unattributed: scale < 1 ? 0 : Math.max(0, rest - estSum),
  };
};

interface WalkResult {
  readonly compactionTurns: number[];
  readonly snapshots: TurnSnapshot[];
}

/** Mutable accumulator threaded through the single ordered event walk. */
interface WalkState {
  cat: MutSlices;
  lastSnapReq: string | undefined;
  pendingCompaction: boolean;
  retainedThinking: number;
}

/** Map each request id of a visible text/tool_use event to its token estimate. */
const visibleByRequest = (
  events: ParsedSession["events"]
): Map<string, number> => {
  const ttByReq = new Map<string, number>();
  for (const e of events) {
    if (
      e.requestId &&
      (e.kind === "assistant-text" || e.kind === "tool-call")
    ) {
      ttByReq.set(e.requestId, (ttByReq.get(e.requestId) ?? 0) + e.tokensEst);
    }
  }
  return ttByReq;
};

/** Snapshot one turn and advance retained-content accumulators. */
const snapshotTurn = (args: {
  readonly ws: WalkState;
  readonly turn: ParsedSession["turns"][number];
  readonly ti: number;
  readonly requestId: string;
  readonly ttByReq: Map<string, number>;
  readonly snapshots: TurnSnapshot[];
  readonly compactionTurns: number[];
}) => {
  const { ws, turn, ti, requestId, ttByReq, snapshots, compactionTurns } = args;
  snapshots.push({
    turnIndex: ti,
    model: turn.model,
    ctx: turn.contextTokens,
    outputTokens: turn.outputTokens,
    cacheReadTokens: turn.cacheReadTokens,
    slices: makeSlices({
      ctx: turn.contextTokens,
      cat: ws.cat,
      retainedThinking: ws.retainedThinking,
    }),
    ...(turn.ts === undefined ? {} : { ts: turn.ts }),
  });
  if (ws.pendingCompaction) {
    compactionTurns.push(ti);
    ws.pendingCompaction = false;
  }
  ws.lastSnapReq = requestId;
  const tt = ttByReq.get(requestId) ?? 0;
  ws.cat.assistant_text += tt;
  ws.retainedThinking += Math.max(0, turn.outputTokens - tt);
};

/** Evict growable content on a compaction, then fold the event's tokens. */
const foldNonTurnEvent = (
  ws: WalkState,
  e: ParsedSession["events"][number]
) => {
  if (e.kind === "compaction") {
    ws.pendingCompaction = true;
    ws.retainedThinking = 0;
    ws.cat.assistant_text = 0;
    ws.cat.tool_results = 0;
    ws.cat.prompts = 0;
    ws.cat.files = 0;
    ws.cat.other = 0;
  }
  foldContent({
    cat: ws.cat,
    loadedCategory: e.loadedCategory,
    kind: e.kind,
    tokens: e.tokensEst,
  });
};

/** Single ordered walk: accumulate retained content, snapshot each turn. */
const walkTurns = (args: {
  readonly p: ParsedSession;
  readonly systemOverheadTokens: number;
}): WalkResult => {
  const { p, systemOverheadTokens } = args;
  const turns = p.turns;
  const turnIndexByReq = new Map(
    turns.map((t, i) => [t.requestId, i] as const)
  );
  const ttByReq = visibleByRequest(p.events);

  const cat = zeroSlices();
  cat.system_tools = systemOverheadTokens;
  const ws: WalkState = {
    cat,
    retainedThinking: 0,
    pendingCompaction: false,
    lastSnapReq: undefined,
  };
  const snapshots: TurnSnapshot[] = [];
  const compactionTurns: number[] = [];

  for (const e of p.events) {
    const ti = e.requestId ? turnIndexByReq.get(e.requestId) : undefined;
    if (ti !== undefined) {
      const turn = turns[ti];
      if (turn && e.requestId !== ws.lastSnapReq) {
        snapshotTurn({
          ws,
          turn,
          ti,
          requestId: e.requestId as string,
          ttByReq,
          snapshots,
          compactionTurns,
        });
      }
    } else if (!e.isSidechain) {
      foldNonTurnEvent(ws, e);
    }
  }
  return { snapshots, compactionTurns };
};

/** Compute the residual system+tools floor from the first turn. */
const computeSystemOverhead = (p: ParsedSession): number => {
  const firstReq = p.turns[0]?.requestId;
  let firstTurnPos = p.events.findIndex((e) => e.requestId === firstReq);
  if (firstTurnPos < 0) {
    firstTurnPos = p.events.length;
  }
  let visibleAtStart = 0;
  for (let i = 0; i < firstTurnPos; i++) {
    visibleAtStart += p.events[i]?.tokensEst ?? 0;
  }
  return Math.max(0, (p.turns[0]?.contextTokens ?? 0) - visibleAtStart);
};

/** Analyze a parsed session into render-ready, serializable metrics. */
export const analyze = (
  p: ParsedSession,
  opts: AnalyzeOptions = {}
): AnalyzedSession => {
  const turns = p.turns;
  const dumbZoneFraction = opts.dumbZoneFraction ?? DEFAULT_DUMB_ZONE;
  const peak = computePeak(turns);
  const finalContextTokens = turns.at(-1)?.contextTokens ?? 0;

  const contextWindow = opts.window ?? DEFAULT_WINDOW;
  const contextWindowInferred = opts.window === undefined;

  const systemOverheadTokens = computeSystemOverhead(p);
  const { snapshots, compactionTurns } = walkTurns({ p, systemOverheadTokens });

  const peakSnap = snapshots[peak.peakTurnIndex] ?? snapshots.at(-1);
  const budget: BudgetSlice[] = (Object.keys(CAT_META) as BudgetKey[])
    .map((key) => ({
      key,
      ...CAT_META[key],
      tokens: peakSnap?.slices[key] ?? 0,
    }))
    .filter((s) => s.tokens > 0);

  const dumbZoneTokens = dumbZoneFraction * contextWindow;
  let dumbZoneCrossTurn = -1;
  let dumbZoneTurns = 0;
  turns.forEach((t, i) => {
    if (t.contextTokens >= dumbZoneTokens) {
      if (dumbZoneCrossTurn < 0) {
        dumbZoneCrossTurn = i;
      }
      dumbZoneTurns++;
    }
  });

  const biggestItems: TimelineEvent[] = [...p.events]
    .filter((e) => e.kind !== "system" && e.tokensEst > 0)
    .sort((a, b) => b.tokensEst - a.tokensEst)
    .slice(0, BIGGEST_ITEMS_LIMIT);

  const userMessageCount = p.events.filter(
    (e) => e.kind === "user-prompt"
  ).length;
  const toolCallCount = p.events.filter((e) => e.kind === "tool-call").length;
  const durationMs =
    p.startedAt && p.endedAt
      ? new Date(p.endedAt).getTime() - new Date(p.startedAt).getTime()
      : undefined;

  return {
    ...p,
    contextWindow,
    contextWindowInferred,
    peakContextTokens: peak.peakContextTokens,
    peakTurnIndex: peak.peakTurnIndex,
    finalContextTokens,
    totalOutputTokens: peak.totalOutputTokens,
    systemOverheadTokens,
    budget,
    snapshots,
    onDiskContextFiles: opts.onDiskContextFiles ?? [],
    dumbZoneCrossTurn,
    dumbZoneFraction,
    dumbZoneTurns,
    compactionTurns,
    biggestItems,
    turnCount: turns.length,
    userMessageCount,
    toolCallCount,
    peakCacheReadTokens: peak.peakCacheReadTokens,
    ...(durationMs === undefined ? {} : { durationMs }),
  };
};

export { CAT_META };
