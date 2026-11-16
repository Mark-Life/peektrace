/** Illustrative sample session data for demos, docs, and the marketing site.
 *
 * Nothing here was captured from a real transcript: it is a hand-authored
 * session shaped to show every state the session views can render — context
 * climbing into the dumb zone, two compaction cliffs that evict growable
 * history, and a peak that lands late rather than last. Every value is typed
 * against the generated wire types, so a schema change breaks this file rather than the
 * demo, and the per-turn category slices sum exactly to that turn's context, as
 * a real analysis guarantees.
 */
import type {
  AnalyzedSession,
  BudgetKey,
  BudgetSlices,
  TimelineEvent,
  Turn,
  TurnSnapshot,
} from "@workspace/view-core/api-types.gen";
import { CAT_META } from "@workspace/view-core/constants.gen";

import { STACK_ORDER } from "../lib/session-format";

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

/**
 * Category attribution at the peak turn. These ten values are the session's
 * headline: they sum to 612,140 tokens, the peak context.
 */
const PEAK_SLICES: BudgetSlices = {
  assistant_text: 60_100,
  files: 40_000,
  listings: 22_000,
  memory: 71_000,
  other: 30_140,
  prompts: 18_000,
  system_tools: 48_300,
  thinking: 210_400,
  tool_results: 96_200,
  unattributed: 16_000,
};

/**
 * Category attribution at the first turn. The three floors (system + tools,
 * listings, CLAUDE.md) are already fully loaded before the user says anything
 * and never move; everything else grows from here and is evicted by compaction.
 */
const START_SLICES: BudgetSlices = {
  assistant_text: 400,
  files: 0,
  listings: 22_000,
  memory: 71_000,
  other: 600,
  prompts: 1200,
  system_tools: 48_300,
  thinking: 800,
  tool_results: 0,
  unattributed: 2000,
};

/** Sum every category of a slice map — by construction, the turn's context. */
const sumSlices = (s: BudgetSlices) =>
  STACK_ORDER.reduce((total, key) => total + s[key], 0);

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
  { ctx: START_TOTAL, turn: 0 },
  { ctx: 205_000, turn: 6 },
  { ctx: 280_000, turn: 14 },
  { ctx: 322_000, turn: 20 },
  { ctx: 352_000, turn: 24 },
  { cliff: true, ctx: 168_000, turn: 25 },
  { ctx: 330_000, turn: 30 },
  { ctx: 392_000, turn: 32 },
  { ctx: 404_000, turn: 33 },
  { ctx: 505_000, turn: 40 },
  { ctx: 596_000, turn: 46 },
  { cliff: true, ctx: 385_000, turn: 47 },
  { ctx: 421_000, turn: 48 },
  { ctx: 498_000, turn: 55 },
  { ctx: 560_000, turn: 62 },
  { ctx: PEAK_TOTAL, turn: PEAK_TURN_INDEX },
  { ctx: 571_000, turn: 74 },
];

/** Turn indexes whose snapshot shows a compaction drop (turns 26 and 48). */
const COMPACTION_TURNS = CTX_ANCHORS.flatMap((a) =>
  a.cliff === true ? [a.turn] : []
);

/** Context tokens at a turn, linearly interpolated between the control points. */
const ctxTargetAt = (turnIndex: number) => {
  const last = CTX_ANCHORS.at(-1) ?? { ctx: START_TOTAL, turn: 0 };

  for (let i = 0; i < CTX_ANCHORS.length - 1; i += 1) {
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
  assistant_text: sliceAt("assistant_text", growth),
  files: sliceAt("files", growth),
  listings: sliceAt("listings", growth),
  memory: sliceAt("memory", growth),
  other: sliceAt("other", growth),
  prompts: sliceAt("prompts", growth),
  system_tools: sliceAt("system_tools", growth),
  thinking: sliceAt("thinking", growth),
  tool_results: sliceAt("tool_results", growth),
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
  input: { base: 1200, mult: 1, span: 5, step: 260 },
  output: { base: 900, mult: 977, span: 47, step: 91 },
  prompt: { base: 320, mult: 1, span: 7, step: 90 },
  text: { base: 700, mult: 17, span: 23, step: 130 },
  thinking: { base: 1600, mult: 53, span: 31, step: 120 },
  toolCall: { base: 180, mult: 1, span: 4, step: 60 },
  toolResult: { base: 2400, mult: 37, span: 29, step: 700 },
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
  vary({ seed: turnIndex, size: SIZES.output });

/** Fresh (uncached) input tokens billed for a turn. */
const inputAt = (turnIndex: number) =>
  vary({ seed: turnIndex, size: SIZES.input });

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
  /** Every nth tool result comes back as an error, offset off the boundary. */
  errorEvery: 11,
  errorOffset: 7,
  /** Every nth turn runs a second tool call. */
  extraToolEvery: 2,
  /** Match counts quoted in a tool result's preview. */
  matchSpan: 9,
  /** Every nth turn opens with a fresh user prompt. */
  promptEvery: 3,
  /** Tool slots per turn, so a turn's two tool calls get distinct seeds. */
  toolSlots: 3,
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
      body: `${tool}(${path})`,
      kind: "tool-call",
      preview: `${tool}(${path})`,
      requestId,
      title: `${tool} ${path}`,
      tokensEst: vary({ seed, size: SIZES.toolCall }),
      toolName: tool,
      toolUseId,
      ts,
    },
    {
      body: `${path} — ${matches} matches`,
      isError: seed % CADENCE.errorEvery === CADENCE.errorOffset,
      kind: "tool-result",
      preview: `${path} — ${matches} matches`,
      requestId,
      title: `Result: ${tool}`,
      tokensEst: vary({ seed, size: SIZES.toolResult }),
      toolName: tool,
      toolUseId,
      ts,
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
            body: prompt,
            kind: "user-prompt",
            preview: prompt,
            requestId,
            title: "User",
            tokensEst: vary({ seed: turnIndex, size: SIZES.prompt }),
            ts,
          },
        ] as const)
      : []),
    {
      body: rationale,
      kind: "assistant-thinking",
      preview: rationale,
      requestId,
      title: "Thinking",
      tokensEst: vary({ seed: turnIndex, size: SIZES.thinking }),
      ts,
    },
    ...toolPair({ slot: 0, turnIndex }),
    ...(runsExtraTool ? toolPair({ slot: 1, turnIndex }) : []),
    {
      body: summary,
      kind: "assistant-text",
      preview: summary,
      requestId,
      title: "Assistant",
      tokensEst: vary({ seed: turnIndex, size: SIZES.text }),
      ts,
    },
  ];
};

/** The always-loaded preamble: skills, agents, tool defs, and CLAUDE.md. */
const PREAMBLE: readonly Omit<TimelineEvent, "index">[] = [
  {
    attachmentType: "claude_md",
    body: "Project + global instructions",
    kind: "attachment",
    loadedCategory: "claude-md",
    preview: "Project + global instructions",
    title: "CLAUDE.md",
    tokensEst: 71_000,
    ts: tsAt(0),
  },
  {
    attachmentType: "skills",
    body: "14 skills available",
    kind: "attachment",
    loadedCategory: "skills",
    preview: "14 skills available",
    title: "Skill listing",
    tokensEst: 9400,
    ts: tsAt(0),
  },
  {
    attachmentType: "agents",
    body: "6 subagent types available",
    kind: "attachment",
    loadedCategory: "agents",
    preview: "6 subagent types available",
    title: "Agent listing",
    tokensEst: 5100,
    ts: tsAt(0),
  },
  {
    attachmentType: "tools",
    body: "MCP + built-in tool schemas",
    kind: "attachment",
    loadedCategory: "tools",
    preview: "MCP + built-in tool schemas",
    title: "Tool definitions",
    tokensEst: 7500,
    ts: tsAt(0),
  },
];

/** A compaction: history is summarized away and growable context is evicted. */
const compactionEvent = (turnIndex: number): Omit<TimelineEvent, "index"> => ({
  body: "Prior turns summarized; tool results and files evicted",
  kind: "compaction",
  preview: "Prior turns summarized; tool results and files evicted",
  title: "Conversation compacted",
  tokensEst: 3400,
  ts: tsAt(turnIndex),
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
  cacheCreationTokens: cacheCreationAt(i),
  cacheReadTokens: cacheReadAt(i),
  contextTokens: CTX_BY_TURN[i] ?? 0,
  eventIndexes: eventIndexesFor(i),
  inputTokens: inputAt(i),
  model: MODEL,
  outputTokens: outputAt(i),
  requestId: requestIdAt(i),
  ts: tsAt(i),
}));

const SNAPSHOTS: readonly TurnSnapshot[] = TURN_INDEXES.map((i) => ({
  cacheReadTokens: cacheReadAt(i),
  ctx: CTX_BY_TURN[i] ?? 0,
  model: MODEL,
  outputTokens: outputAt(i),
  slices: SLICES_BY_TURN[i] ?? START_SLICES,
  ts: tsAt(i),
  turnIndex: i,
}));

/** The peak-turn budget: every category, with core's own labels and colors. */
const BUDGET = STACK_ORDER.map((key) => ({
  key,
  ...CAT_META[key],
  tokens: PEAK_SLICES[key],
})).filter((slice) => slice.tokens > 0);

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
export const MOCK_SESSION: AnalyzedSession = {
  budget: BUDGET,
  compactionIndexes: EVENTS.filter((e) => e.kind === "compaction").map(
    (e) => e.index
  ),
  compactionTurns: COMPACTION_TURNS,
  contextWindow: CONTEXT_WINDOW,
  contextWindowInferred: true,
  cwd: CWD,
  dumbZoneCrossTurn: DUMB_ZONE_CROSS_TURN,
  dumbZoneFraction: DUMB_ZONE_FRACTION,
  dumbZoneTurns: DUMB_ZONE_TURNS,
  durationMs: (TURN_COUNT - 1) * TURN_INTERVAL_MS,
  endedAt: tsAt(TURN_COUNT - 1),
  events: EVENTS,
  finalContextTokens: CTX_BY_TURN.at(-1) ?? 0,
  gitBranch: "feat/import-pipeline",
  models: [MODEL],
  onDiskContextFiles: [
    {
      bytes: 6820,
      label: "CLAUDE.md",
      path: "/Users/dev/.claude/CLAUDE.md",
      scope: "global",
      tokensEst: 1740,
    },
    {
      bytes: 14_960,
      label: "CLAUDE.md",
      path: `${CWD}/CLAUDE.md`,
      scope: "project",
      tokensEst: 3810,
    },
    {
      bytes: 29_594,
      label: "MEMORY.md",
      path: "/Users/dev/.claude/projects/-Users-dev-code-acme-ingest/memory/MEMORY.md",
      scope: "memory",
      tokensEst: 7420,
    },
    {
      bytes: 3940,
      label: "session-parsing.md",
      path: "/Users/dev/.claude/projects/-Users-dev-code-acme-ingest/memory/session-parsing.md",
      scope: "memory",
      tokensEst: 990,
    },
  ],
  path: `/Users/dev/.claude/projects/-Users-dev-code-acme-ingest/${SESSION_ID}.jsonl`,
  peakCacheReadTokens: cacheReadAt(PEAK_TURN_INDEX),
  peakContextTokens: PEAK_TOTAL,
  peakTurnIndex: PEAK_TURN_INDEX,
  provider: "claude-code",
  sessionId: SESSION_ID,
  snapshots: SNAPSHOTS,
  startedAt: tsAt(0),
  subagents: [
    {
      agentType: "code-reviewer",
      description: "Review the dedupe key change",
      id: "sub_01",
      path: `/Users/dev/.claude/projects/-Users-dev-code-acme-ingest/${SESSION_ID}-sub-01.jsonl`,
      peakContextTokens: 86_400,
      toolUseId: "toolu_042",
      turns: 9,
    },
    {
      agentType: "test-writer",
      description: "Cover the partial-batch failure path",
      id: "sub_02",
      path: `/Users/dev/.claude/projects/-Users-dev-code-acme-ingest/${SESSION_ID}-sub-02.jsonl`,
      peakContextTokens: 132_900,
      toolUseId: "toolu_078",
      turns: 14,
    },
    {
      agentType: "explorer",
      description: "Trace the migration lock",
      id: "sub_03",
      path: `/Users/dev/.claude/projects/-Users-dev-code-acme-ingest/${SESSION_ID}-sub-03.jsonl`,
      peakContextTokens: 61_200,
      toolUseId: "toolu_121",
      turns: 6,
    },
  ],
  systemOverheadTokens: PEAK_SLICES.system_tools,
  title: "Import pipeline: dedupe keys and a 90-day backfill",
  toolCallCount: EVENTS.filter((e) => e.kind === "tool-call").length,
  totalOutputTokens: TURN_INDEXES.reduce((sum, i) => sum + outputAt(i), 0),
  turnCount: TURN_COUNT,
  turns: TURNS,
  userMessageCount: EVENTS.filter((e) => e.kind === "user-prompt").length,
  version: "2.1.4",
  webFetchRequests: 0,
  webSearchRequests: 0,
};
