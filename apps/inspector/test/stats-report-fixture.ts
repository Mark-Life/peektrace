/** A minimal `StatsReport` with only the rows a test cares about. */
import type {
  CauseRow,
  FailRow,
  StatsReport,
  TimeRow,
} from "@workspace/core/services/stats/schema";
import { DETECTOR_VERSION } from "@workspace/core/services/stats/schema";

export const report = (args: {
  readonly causes?: readonly CauseRow[];
  readonly fails: readonly FailRow[];
  readonly time: readonly TimeRow[];
}): StatsReport => ({
  causes: args.causes ?? [],
  caveats: [],
  clusters: [],
  context: [],
  corpus: {
    agents: {},
    bashCalls: 0,
    bashSec: 0,
    kinds: {},
    projects: 0,
    sessions: 0,
    toolCalls: 0,
    toolSec: 0,
    humanWaitSec: 0,
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
  time: args.time,
  waiting: {
    bgDeclaredSec: 0,
    bgRuns: 0,
    bgSec: 0,
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
