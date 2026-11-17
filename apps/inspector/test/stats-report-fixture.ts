/** A minimal `StatsReport` with only the rows a test cares about. */
import type {
  CauseRow,
  FailRow,
  StatsReport,
  StatsText,
  TimeRow,
} from "@workspace/view-core/api-types.gen";
import { DETECTOR_VERSION } from "@workspace/view-core/constants.gen";

/** Core's text for a report, empty unless a test fills it in. */
export const statsText = (over: Partial<StatsText> = {}): StatsText => ({
  causeNarrow: {},
  causeSplit: {},
  findings: [],
  prompts: { cause: {}, fail: {}, report: "", time: {} },
  ruleLines: {},
  suggested: [],
  suggestedRule: {},
  timeNotes: {},
  verdict: null,
  verdictNote: null,
  working: [],
  ...over,
});

export const report = (args: {
  readonly causes?: readonly CauseRow[];
  readonly fails: readonly FailRow[];
  readonly text?: StatsText;
  readonly time: readonly TimeRow[];
}): StatsReport => ({
  causeTotals: {
    hits: 0,
    nonFailingPairs: 0,
    pairs: 0,
    retryCtxTokens: 0,
    retrySec: 0,
    retryShare: 0,
    retryUsd: null,
    shell: true,
    unclassifiedPairs: 0,
  },
  causes: args.causes ?? [],
  caveats: [],
  clusters: [],
  context: [],
  corpus: {
    agents: {},
    bashCalls: 0,
    bashSec: 0,
    humanWaitSec: 0,
    kinds: {},
    projects: 0,
    sessions: 0,
    toolCalls: 0,
    toolSec: 0,
    transcripts: 0,
  },
  cost: [],
  detectorVersion: DETECTOR_VERSION,
  fails: args.fails,
  fanout: [],
  generatedAt: 0,
  paths: {},
  schemaVersion: "peektrace-stats/v2",
  skipped: [],
  text: args.text ?? statsText(),
  time: args.time,
  waiting: {
    bgDeclaredSec: 0,
    bgRuns: 0,
    bgSec: 0,
    // oxlint-disable-next-line anti-slop/no-shape-in-symbol-names -- field name of the generated API type
    byShape: [],
    calls: 0,
    medianSec: 0,
    p95Sec: 0,
    projects: 0,
    samples: [],
    sessions: 0,
    share: 0,
    totalSec: 0,
  },
  web: [],
  window: {
    days: 30,
    fromIso: "2026-09-21T00:00:00.000Z",
    toIso: "2026-10-21T00:00:00.000Z",
  },
});
