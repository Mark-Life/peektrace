/** Illustrative sample session data for demos, docs, and the marketing site.
 *
 * Nothing here was captured from a real transcript: it is a hand-authored
 * session shaped to show every state the session views can render — context
 * climbing into the dumb zone, two compaction cliffs that evict growable
 * history, and a peak that lands late rather than last. Every value is typed
 * against the core schemas, so a schema change breaks this file rather than the
 * demo, and the per-turn category slices sum exactly to that turn's context, as
 * a real analysis guarantees.
 */
import { CAT_META } from "@workspace/core/services/sessions/analyze";
import type {
  AnalyzedSession,
  BudgetKey,
  BudgetSlices,
  TimelineEvent,
  Turn,
  TurnSnapshot,
} from "@workspace/core/services/sessions/schema";

const TURN_COUNT = 75;
const PEAK_TURN_INDEX = 69;
const CONTEXT_WINDOW = 1_000_000;
const DUMB_ZONE_FRACTION = 0.4;
const MODEL = "claude-sonnet-4-5-20250929";
const SESSION_ID = "a3f2c1d8-7b4e-4f19-9c2a-6d5e8b1f0a37";
const CWD = "/Users/dev/code/acme/ingest";
/** Wall-clock spacing between turns, and the epoch the session starts from. */
const TURN_INTERVAL_MS = 90_000;
const SESSION_START_MS = Date.parse("2026-03-11T09:02:14.000Z");
/** Top-N largest events surfaced as `biggestItems`, matching core's analysis. */
const BIGGEST_ITEMS_LIMIT = 40;

/**
 * Category attribution at the peak turn. These ten values are the session's
 * headline: they sum to 612,140 tokens, the peak context.
 */
const PEAK_SLICES: BudgetSlices = {
  system_tools: 48_300,
  listings: 22_000,
  memory: 71_000,
  files: 40_000,
  prompts: 18_000,
  tool_results: 96_200,
  assistant_text: 60_100,
  thinking: 210_400,
  other: 30_140,
  unattributed: 16_000,
};

/**
 * Category attribution at the first turn. The three floors (system + tools,
 * listings, CLAUDE.md) are already fully loaded before the user says anything
 * and never move; everything else grows from here and is evicted by compaction.
 */
const START_SLICES: BudgetSlices = {
  system_tools: 48_300,
  listings: 22_000,
  memory: 71_000,
  files: 0,
  prompts: 1200,
  tool_results: 0,
  assistant_text: 400,
  thinking: 800,
  other: 600,
  unattributed: 2000,
};

const BUDGET_KEYS = Object.keys(PEAK_SLICES) as readonly BudgetKey[];

/** Sum every category of a slice map — by construction, the turn's context. */
const sumSlices = (s: BudgetSlices) =>
  BUDGET_KEYS.reduce((total, key) => total + s[key], 0);

const START_TOTAL = sumSlices(START_SLICES);
const PEAK_TOTAL = sumSlices(PEAK_SLICES);

/** One control point of the context curve; `cliff` marks a compaction drop. */
interface CtxAnchor {
  readonly cliff?: boolean;
  readonly ctx: number;
  readonly turn: number;
}

/**
 * The shape of the session: a climb into the first compaction cliff, a steep
 * re-climb across the 40% dumb-zone line, a second cliff, then the long run up
 * to the peak and a mild decay. Context between anchors is interpolated.
 */
const CTX_ANCHORS: readonly CtxAnchor[] = [
  { turn: 0, ctx: START_TOTAL },
  { turn: 6, ctx: 205_000 },
  { turn: 14, ctx: 280_000 },
  { turn: 20, ctx: 322_000 },
  { turn: 24, ctx: 352_000 },
  { turn: 25, ctx: 168_000, cliff: true },
  { turn: 30, ctx: 330_000 },
  { turn: 32, ctx: 392_000 },
  { turn: 33, ctx: 404_000 },
  { turn: 40, ctx: 505_000 },
  { turn: 46, ctx: 596_000 },
  { turn: 47, ctx: 385_000, cliff: true },
  { turn: 48, ctx: 421_000 },
  { turn: 55, ctx: 498_000 },
  { turn: 62, ctx: 560_000 },
  { turn: PEAK_TURN_INDEX, ctx: PEAK_TOTAL },
  { turn: 74, ctx: 571_000 },
];

/** Turn indexes whose snapshot shows a compaction drop (turns 26 and 48). */
const COMPACTION_TURNS = CTX_ANCHORS.filter((a) => a.cliff).map((a) => a.turn);

/** Context tokens at a turn, linearly interpolated between the control points. */
const ctxTargetAt = (turnIndex: number) => {
  const last = CTX_ANCHORS.at(-1) ?? { turn: 0, ctx: START_TOTAL };
  for (let i = 0; i < CTX_ANCHORS.length - 1; i++) {
    const from = CTX_ANCHORS[i] ?? last;
    const to = CTX_ANCHORS[i + 1] ?? last;
    if (turnIndex >= from.turn && turnIndex <= to.turn) {
      const span = to.turn - from.turn;
      if (span === 0) {
        return from.ctx;
      }
      return from.ctx + ((to.ctx - from.ctx) * (turnIndex - from.turn)) / span;
    }
  }
  return last.ctx;
};

/**
 * How far a turn has grown from its start composition toward its peak one, as a
 * 0→1 fraction. Exactly 1 at the peak turn, so the peak slices land on their
 * headline values to the token.
 */
const growthAt = (turnIndex: number) =>
  (ctxTargetAt(turnIndex) - START_TOTAL) / (PEAK_TOTAL - START_TOTAL);

/** Interpolate one category between its start and peak size. */
const sliceAt = (key: BudgetKey, growth: number) =>
  Math.round(
    START_SLICES[key] + (PEAK_SLICES[key] - START_SLICES[key]) * growth
  );

/** Category attribution for a turn at a given growth fraction. */
const slicesAt = (growth: number): BudgetSlices => ({
  system_tools: sliceAt("system_tools", growth),
  listings: sliceAt("listings", growth),
  memory: sliceAt("memory", growth),
  files: sliceAt("files", growth),
  prompts: sliceAt("prompts", growth),
  tool_results: sliceAt("tool_results", growth),
  assistant_text: sliceAt("assistant_text", growth),
  thinking: sliceAt("thinking", growth),
  other: sliceAt("other", growth),
  unattributed: sliceAt("unattributed", growth),
});

const TURN_INDEXES = Array.from({ length: TURN_COUNT }, (_, i) => i);
const SLICES_BY_TURN = TURN_INDEXES.map((i) => slicesAt(growthAt(i)));
/** Context per turn: always the exact sum of that turn's category slices. */
const CTX_BY_TURN = SLICES_BY_TURN.map(sumSlices);

/** ISO timestamp of a turn, 90s apart from a fixed epoch. */
const tsAt = (turnIndex: number) =>
  new Date(SESSION_START_MS + turnIndex * TURN_INTERVAL_MS).toISOString();

/** Zero-padding width for the synthetic request and tool-use ids. */
const ID_WIDTH = 3;

const requestIdAt = (turnIndex: number) =>
  `req_${String(turnIndex).padStart(ID_WIDTH, "0")}`;

/**
 * Token sizes that vary per turn without ever being random: `base` plus one of
 * `span` steps, chosen by multiplying the turn index by `mult`. Same turn, same
 * number, on every render and on every machine.
 */
const SIZES = {
  output: { base: 900, step: 91, span: 47, mult: 977 },
  input: { base: 1200, step: 260, span: 5, mult: 1 },
  prompt: { base: 320, step: 90, span: 7, mult: 1 },
  thinking: { base: 1600, step: 120, span: 31, mult: 53 },
  toolCall: { base: 180, step: 60, span: 4, mult: 1 },
  toolResult: { base: 2400, step: 700, span: 29, mult: 37 },
  text: { base: 700, step: 130, span: 23, mult: 17 },
} as const;

/** Resolve one entry of {@link SIZES} for a given seed. */
const vary = ({
  size,
  seed,
}: {
  readonly size: (typeof SIZES)[keyof typeof SIZES];
  readonly seed: number;
}) => size.base + ((seed * size.mult) % size.span) * size.step;

/** Model output for a turn — varied but deterministic, driving the timeline dots. */
const outputAt = (turnIndex: number) =>
  vary({ size: SIZES.output, seed: turnIndex });

/** Fresh (uncached) input tokens billed for a turn. */
const inputAt = (turnIndex: number) =>
  vary({ size: SIZES.input, seed: turnIndex });

/** Newly cached tokens: the context a turn added over its predecessor. */
const cacheCreationAt = (turnIndex: number) => {
  const ctx = CTX_BY_TURN[turnIndex] ?? 0;
  if (turnIndex === 0) {
    return Math.max(0, ctx - inputAt(0));
  }
  return Math.max(0, ctx - (CTX_BY_TURN[turnIndex - 1] ?? 0));
};

/** Cache hits: everything in context that was neither fresh input nor new cache. */
const cacheReadAt = (turnIndex: number) =>
  Math.max(
    0,
    (CTX_BY_TURN[turnIndex] ?? 0) -
      inputAt(turnIndex) -
      cacheCreationAt(turnIndex)
  );

const TOOLS = ["Read", "Grep", "Edit", "Bash", "Task"] as const;
const PATHS = [
  "src/ingest/normalize.ts",
  "src/ingest/dedupe.ts",
  "src/ingest/backfill.ts",
  "src/db/schema.ts",
  "src/db/migrate.ts",
  "test/ingest.test.ts",
] as const;
const PROMPTS = [
  "Backfill the last 90 days without double-writing rows",
  "The dedupe key collides on re-imported batches — fix it",
  "Why is the migration locking the table for 40s?",
  "Add a retry with backoff around the upstream fetch",
  "Cover the partial-batch failure path with a test",
] as const;

/** Pick from a fixed pool by turn, so every render produces the same session. */
const pick = <T>(pool: readonly [T, ...T[]], turnIndex: number) =>
  pool[turnIndex % pool.length] ?? pool[0];

/** Cadences that shape the event stream, all keyed off the turn index. */
const CADENCE = {
  /** Tool slots per turn, so a turn's two tool calls get distinct seeds. */
  toolSlots: 3,
  /** Every nth turn opens with a fresh user prompt. */
  promptEvery: 3,
  /** Every nth turn runs a second tool call. */
  extraToolEvery: 2,
  /** Every nth tool result comes back as an error, offset off the boundary. */
  errorEvery: 11,
  errorOffset: 7,
  /** Match counts quoted in a tool result's preview. */
  matchSpan: 9,
} as const;

/** One tool call + its result, sized deterministically from the turn index. */
const toolPair = (args: {
  readonly turnIndex: number;
  readonly slot: number;
}): readonly Omit<TimelineEvent, "index">[] => {
  const { turnIndex, slot } = args;
  const seed = turnIndex * CADENCE.toolSlots + slot;
  const tool = pick(TOOLS, seed);
  const path = pick(PATHS, seed);
  const toolUseId = `toolu_${String(seed).padStart(ID_WIDTH, "0")}`;
  const requestId = requestIdAt(turnIndex);
  const ts = tsAt(turnIndex);
  const matches = (seed % CADENCE.matchSpan) + 1;
  return [
    {
      kind: "tool-call",
      requestId,
      ts,
      title: `${tool} ${path}`,
      preview: `${tool}(${path})`,
      body: `${tool}(${path})`,
      tokensEst: vary({ size: SIZES.toolCall, seed }),
      toolName: tool,
      toolUseId,
    },
    {
      kind: "tool-result",
      requestId,
      ts,
      title: `Result: ${tool}`,
      preview: `${path} — ${matches} matches`,
      body: `${path} — ${matches} matches`,
      tokensEst: vary({ size: SIZES.toolResult, seed }),
      toolName: tool,
      toolUseId,
      isError: seed % CADENCE.errorEvery === CADENCE.errorOffset,
    },
  ];
};

/** Every event of one turn: optional prompt, thinking, tool traffic, then text. */
const turnEvents = (
  turnIndex: number
): readonly Omit<TimelineEvent, "index">[] => {
  const requestId = requestIdAt(turnIndex);
  const ts = tsAt(turnIndex);
  const prompt = pick(PROMPTS, turnIndex);
  const isPromptTurn = turnIndex % CADENCE.promptEvery === 0;
  const runsExtraTool = turnIndex % CADENCE.extraToolEvery === 0;
  const summary = `Patched ${pick(PATHS, turnIndex)} and re-ran the suite.`;
  const rationale = "Weighing the upsert against a staging table…";
  return [
    ...(isPromptTurn
      ? ([
          {
            kind: "user-prompt",
            requestId,
            ts,
            title: "User",
            preview: prompt,
            body: prompt,
            tokensEst: vary({ size: SIZES.prompt, seed: turnIndex }),
          },
        ] as const)
      : []),
    {
      kind: "assistant-thinking",
      requestId,
      ts,
      title: "Thinking",
      preview: rationale,
      body: rationale,
      tokensEst: vary({ size: SIZES.thinking, seed: turnIndex }),
    },
    ...toolPair({ turnIndex, slot: 0 }),
    ...(runsExtraTool ? toolPair({ turnIndex, slot: 1 }) : []),
    {
      kind: "assistant-text",
      requestId,
      ts,
      title: "Assistant",
      preview: summary,
      body: summary,
      tokensEst: vary({ size: SIZES.text, seed: turnIndex }),
    },
  ];
};

/** The always-loaded preamble: skills, agents, tool defs, and CLAUDE.md. */
const PREAMBLE: readonly Omit<TimelineEvent, "index">[] = [
  {
    kind: "attachment",
    ts: tsAt(0),
    title: "CLAUDE.md",
    preview: "Project + global instructions",
    body: "Project + global instructions",
    tokensEst: 71_000,
    attachmentType: "claude_md",
    loadedCategory: "claude-md",
  },
  {
    kind: "attachment",
    ts: tsAt(0),
    title: "Skill listing",
    preview: "14 skills available",
    body: "14 skills available",
    tokensEst: 9400,
    attachmentType: "skills",
    loadedCategory: "skills",
  },
  {
    kind: "attachment",
    ts: tsAt(0),
    title: "Agent listing",
    preview: "6 subagent types available",
    body: "6 subagent types available",
    tokensEst: 5100,
    attachmentType: "agents",
    loadedCategory: "agents",
  },
  {
    kind: "attachment",
    ts: tsAt(0),
    title: "Tool definitions",
    preview: "MCP + built-in tool schemas",
    body: "MCP + built-in tool schemas",
    tokensEst: 7500,
    attachmentType: "tools",
    loadedCategory: "tools",
  },
];

/** A compaction: history is summarized away and growable context is evicted. */
const compactionEvent = (turnIndex: number): Omit<TimelineEvent, "index"> => ({
  kind: "compaction",
  ts: tsAt(turnIndex),
  title: "Conversation compacted",
  preview: "Prior turns summarized; tool results and files evicted",
  body: "Prior turns summarized; tool results and files evicted",
  tokensEst: 3400,
});

/** Lay every event out in order and stamp it with its position. */
const buildEvents = (): readonly TimelineEvent[] => {
  const ordered = TURN_INDEXES.flatMap((i) => [
    ...(COMPACTION_TURNS.includes(i) ? [compactionEvent(i)] : []),
    ...turnEvents(i),
  ]);
  return [...PREAMBLE, ...ordered].map((e, index) => ({ ...e, index }));
};

const EVENTS = buildEvents();

const eventIndexesFor = (turnIndex: number) =>
  EVENTS.filter((e) => e.requestId === requestIdAt(turnIndex)).map(
    (e) => e.index
  );

const TURNS: readonly Turn[] = TURN_INDEXES.map((i) => ({
  requestId: requestIdAt(i),
  ts: tsAt(i),
  model: MODEL,
  contextTokens: CTX_BY_TURN[i] ?? 0,
  inputTokens: inputAt(i),
  cacheReadTokens: cacheReadAt(i),
  cacheCreationTokens: cacheCreationAt(i),
  outputTokens: outputAt(i),
  eventIndexes: eventIndexesFor(i),
}));

const SNAPSHOTS: readonly TurnSnapshot[] = TURN_INDEXES.map((i) => ({
  turnIndex: i,
  ts: tsAt(i),
  model: MODEL,
  ctx: CTX_BY_TURN[i] ?? 0,
  outputTokens: outputAt(i),
  cacheReadTokens: cacheReadAt(i),
  slices: SLICES_BY_TURN[i] ?? START_SLICES,
}));

/** The peak-turn budget: every category, with core's own labels and colors. */
const BUDGET = BUDGET_KEYS.map((key) => ({
  key,
  ...CAT_META[key],
  tokens: PEAK_SLICES[key],
})).filter((slice) => slice.tokens > 0);

const BIGGEST_ITEMS = [...EVENTS]
  .filter((e) => e.kind !== "system" && e.tokensEst > 0)
  .sort((a, b) => b.tokensEst - a.tokensEst)
  .slice(0, BIGGEST_ITEMS_LIMIT);

/** First turn at or above the dumb-zone line, and how long the session stays there. */
const DUMB_ZONE_TOKENS = DUMB_ZONE_FRACTION * CONTEXT_WINDOW;
const DUMB_ZONE_CROSS_TURN = CTX_BY_TURN.findIndex(
  (ctx) => ctx >= DUMB_ZONE_TOKENS
);
const DUMB_ZONE_TURNS = CTX_BY_TURN.filter(
  (ctx) => ctx >= DUMB_ZONE_TOKENS
).length;

/**
 * Illustrative sample analyzed session: 75 turns against an assumed 1M-token
 * window, crossing the 40% dumb-zone line at turn 34, compacting at turns 26 and
 * 48, and peaking at 612,140 tokens on turn 70 — which the smaller final turn
 * would have hidden.
 */
export const MOCK_SESSION: typeof AnalyzedSession.Type = {
  provider: "claude-code",
  sessionId: SESSION_ID,
  path: `/Users/dev/.claude/projects/-Users-dev-code-acme-ingest/${SESSION_ID}.jsonl`,
  cwd: CWD,
  gitBranch: "feat/import-pipeline",
  title: "Import pipeline: dedupe keys and a 90-day backfill",
  version: "2.1.4",
  models: [MODEL],
  startedAt: tsAt(0),
  endedAt: tsAt(TURN_COUNT - 1),
  events: EVENTS,
  turns: TURNS,
  compactionIndexes: EVENTS.filter((e) => e.kind === "compaction").map(
    (e) => e.index
  ),
  subagents: [
    {
      id: "sub_01",
      agentType: "code-reviewer",
      description: "Review the dedupe key change",
      toolUseId: "toolu_042",
      path: `/Users/dev/.claude/projects/-Users-dev-code-acme-ingest/${SESSION_ID}-sub-01.jsonl`,
      turns: 9,
      peakContextTokens: 86_400,
    },
    {
      id: "sub_02",
      agentType: "test-writer",
      description: "Cover the partial-batch failure path",
      toolUseId: "toolu_078",
      path: `/Users/dev/.claude/projects/-Users-dev-code-acme-ingest/${SESSION_ID}-sub-02.jsonl`,
      turns: 14,
      peakContextTokens: 132_900,
    },
    {
      id: "sub_03",
      agentType: "explorer",
      description: "Trace the migration lock",
      toolUseId: "toolu_121",
      path: `/Users/dev/.claude/projects/-Users-dev-code-acme-ingest/${SESSION_ID}-sub-03.jsonl`,
      turns: 6,
      peakContextTokens: 61_200,
    },
  ],
  contextWindow: CONTEXT_WINDOW,
  contextWindowInferred: true,
  peakContextTokens: PEAK_TOTAL,
  peakTurnIndex: PEAK_TURN_INDEX,
  finalContextTokens: CTX_BY_TURN.at(-1) ?? 0,
  totalOutputTokens: TURN_INDEXES.reduce((sum, i) => sum + outputAt(i), 0),
  systemOverheadTokens: PEAK_SLICES.system_tools,
  durationMs: (TURN_COUNT - 1) * TURN_INTERVAL_MS,
  budget: BUDGET,
  snapshots: SNAPSHOTS,
  onDiskContextFiles: [
    {
      label: "CLAUDE.md",
      path: "/Users/dev/.claude/CLAUDE.md",
      bytes: 6820,
      tokensEst: 1740,
      scope: "global",
    },
    {
      label: "CLAUDE.md",
      path: `${CWD}/CLAUDE.md`,
      bytes: 14_960,
      tokensEst: 3810,
      scope: "project",
    },
    {
      label: "MEMORY.md",
      path: "/Users/dev/.claude/projects/-Users-dev-code-acme-ingest/memory/MEMORY.md",
      bytes: 29_594,
      tokensEst: 7420,
      scope: "memory",
    },
    {
      label: "session-parsing.md",
      path: "/Users/dev/.claude/projects/-Users-dev-code-acme-ingest/memory/session-parsing.md",
      bytes: 3940,
      tokensEst: 990,
      scope: "memory",
    },
  ],
  dumbZoneCrossTurn: DUMB_ZONE_CROSS_TURN,
  dumbZoneFraction: DUMB_ZONE_FRACTION,
  dumbZoneTurns: DUMB_ZONE_TURNS,
  compactionTurns: COMPACTION_TURNS,
  biggestItems: BIGGEST_ITEMS,
  turnCount: TURN_COUNT,
  userMessageCount: EVENTS.filter((e) => e.kind === "user-prompt").length,
  toolCallCount: EVENTS.filter((e) => e.kind === "tool-call").length,
  peakCacheReadTokens: cacheReadAt(PEAK_TURN_INDEX),
};
