/**
 * The seam every detector plugs into.
 *
 * A detector is one `FactAccumulator`: it sees each fact once, in path order,
 * keeps state bounded by distinct keys rather than by row count, and produces
 * its rows in `finish`. It never reads a file, never sorts by hand — ordering
 * comes from `rank.ts` — and never mints a string without `ctx.redact`.
 *
 * `add` is called once per fact and must be synchronous: the walk folds inside
 * `Effect.sync`, so an accumulator that wants IO belongs in the service, not
 * here.
 */

import {
  DEFAULT_ROW_LIMIT,
  DEFAULT_SAMPLE_CAP,
  pickSamples,
  round,
  sortedRecord,
} from "./rank";
import type { Redactor } from "./redact";
import type {
  CallFact,
  CorpusSummary,
  DrillHit,
  DrillTable,
  SampleRef,
  StatsFact,
  StatsWindow,
} from "./schema";

/** Sample candidates held per key before the shortest-first cut. */
const SAMPLE_POOL = 64;

/**
 * Tools whose duration is the human deciding or a subagent running, not the
 * harness working. Their seconds are reported apart and never enter a time
 * share: over the live corpus these three carry 6h45m across 42 calls, and
 * `TaskOutput` is 7,887s of it.
 */
export const HUMAN_WAIT_TOOLS: ReadonlySet<string> = new Set([
  "AskUserQuestion",
  "ExitPlanMode",
  "SendUserMessage",
  "TaskOutput",
]);

/** Which aggregate row a drill request is asking about. */
export interface DrillTarget {
  readonly key: string;
  readonly table: DrillTable;
}

/** What a detector is built with. */
export interface AccumulatorOptions {
  /** When set, the detector also retains the facts behind that one key. */
  readonly drill: DrillTarget | null;
  /** Rows kept per table. */
  readonly limit: number;
  readonly redact: Redactor;
  /** Sample lines kept per row. */
  readonly sampleCap: number;
}

/** What a detector is allowed to read when it produces its rows. */
export interface FinishContext {
  /** Add a line that travels with the numbers, e.g. "12 rows not shown". */
  readonly caveat: (note: string) => void;
  readonly corpus: CorpusSummary;
  readonly limit: number;
  readonly redact: Redactor;
  readonly sampleCap: number;
  readonly window: StatsWindow;
}

/** Anything the walk can fold a fact into. */
export interface FactSink {
  /** Fold one fact. Synchronous, called once per fact, in path order. */
  readonly add: (fact: StatsFact) => void;
}

/** One streaming detector. */
export interface FactAccumulator<Out> extends FactSink {
  /** The facts behind `opts.drill.key`; empty when no drill was requested. */
  readonly drill: (ctx: FinishContext) => readonly DrillHit[];
  /** Ranked, redacted rows. Called once, after the pass. */
  readonly finish: (ctx: FinishContext) => Out;
  /** Stable id, used in spans and in the caveat text. */
  readonly id: string;
}

/** How the service builds a detector. */
export type AccumulatorFactory<Out> = (
  opts: AccumulatorOptions
) => FactAccumulator<Out>;

/** Default options for a plain report pass. */
export const defaultOptions = (redact: Redactor): AccumulatorOptions => ({
  redact,
  limit: DEFAULT_ROW_LIMIT,
  sampleCap: DEFAULT_SAMPLE_CAP,
  drill: null,
});

/** Build a sample ref from a fact and one already-redacted line. */
export const sampleOf = (fact: CallFact, line: string): SampleRef => ({
  sid: fact.sid,
  agent: fact.agent,
  project: fact.project,
  kind: fact.kind,
  seq: fact.seq,
  line,
});

/** Build a drill hit from a fact and one already-redacted line. */
export const hitOf = (fact: CallFact, line: string): DrillHit => ({
  ...sampleOf(fact, line),
  ts: fact.ts,
  durSec: fact.durSec,
  tool: fact.tool,
  failed: fact.failed,
});

/** Per-key state every ranked table needs. Bounded by distinct keys. */
export interface Tally {
  /** One entry per row, for median and p95. */
  durations: number[];
  /** Of `rows`, how many set the agent's own error flag. */
  flagged: number;
  projects: Set<string>;
  rows: number;
  /** Shortest-first pool, cut to `sampleCap` in `finish`. */
  samples: SampleRef[];
  sessions: Set<string>;
  totalSec: number;
}

const emptyTally = (): Tally => ({
  rows: 0,
  flagged: 0,
  totalSec: 0,
  durations: [],
  sessions: new Set(),
  projects: new Set(),
  samples: [],
});

/** Get or create the tally for one key. */
export const tallyFor = (map: Map<string, Tally>, key: string): Tally => {
  const found = map.get(key);
  if (found) {
    return found;
  }
  const fresh = emptyTally();
  map.set(key, fresh);
  return fresh;
};

/** Fold one call into a tally, with an optional sample line. */
export const addToTally = (args: {
  readonly tally: Tally;
  readonly fact: CallFact;
  readonly sample?: SampleRef;
}): void => {
  const { tally, fact, sample } = args;
  tally.rows += 1;
  if (fact.err) {
    tally.flagged += 1;
  }
  if (fact.durSec !== null) {
    tally.totalSec += fact.durSec;
    tally.durations.push(fact.durSec);
  }
  tally.sessions.add(fact.sid);
  tally.projects.add(fact.project);
  if (sample) {
    tally.samples.push(sample);
    if (tally.samples.length > SAMPLE_POOL) {
      tally.samples = [...pickSamples(tally.samples, SAMPLE_POOL)];
    }
  }
};

/** The sample lines a row ships: shortest first, then string compare. */
export const samplesOf = (
  tally: Tally,
  cap = DEFAULT_SAMPLE_CAP
): readonly SampleRef[] => pickSamples(tally.samples, cap);

/** The spine's own sink: the totals every share divides by. */
export interface CorpusTotals extends FactSink {
  readonly finish: () => CorpusSummary;
}

/**
 * Corpus totals. Owned by the spine, not by a detector, because every share in
 * the report divides by a field here and the result is handed to the rest in
 * `FinishContext` before any detector produces a row.
 */
export const makeCorpusTotals = (): CorpusTotals => {
  const projects = new Set<string>();
  const sessions = new Set<string>();
  const kinds = new Map<string, number>();
  const agents = new Map<string, number>();
  let transcripts = 0;
  let toolCalls = 0;
  let bashCalls = 0;
  let bashSec = 0;
  let toolSec = 0;
  let humanWaitSec = 0;

  const addSession = (fact: Extract<StatsFact, { row: "session" }>): void => {
    transcripts += 1;
    sessions.add(fact.sid);
    projects.add(fact.project);
    kinds.set(fact.kind, (kinds.get(fact.kind) ?? 0) + 1);
    agents.set(fact.agent, (agents.get(fact.agent) ?? 0) + 1);
  };

  const addCall = (fact: CallFact): void => {
    toolCalls += 1;
    const sec = fact.durSec ?? 0;
    if (HUMAN_WAIT_TOOLS.has(fact.tool)) {
      humanWaitSec += sec;
      return;
    }
    toolSec += sec;
    if (fact.row === "bash") {
      bashCalls += 1;
      bashSec += sec;
    }
  };

  return {
    add: (fact) => {
      if (fact.row === "session") {
        addSession(fact);
        return;
      }
      addCall(fact);
    },
    finish: () => ({
      transcripts,
      sessions: sessions.size,
      projects: projects.size,
      toolCalls,
      bashCalls,
      bashSec: round(bashSec),
      toolSec: round(toolSec),
      humanWaitSec: round(humanWaitSec),
      kinds: sortedRecord(kinds),
      agents: sortedRecord(agents),
    }),
  };
};
