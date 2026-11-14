/** Runs the `ui` stage's Stats questions (see `parity-ui-stats.ts`). */
import { join } from "node:path";
import { DRILL_PAGE } from "../../apps/inspector/src/lib/stats-atoms";
import type { SessionTitles } from "../../apps/inspector/src/lib/stats-format";
import { findingsOf } from "../../apps/inspector/src/lib/stats-format";
import type { AgentId } from "../../packages/core/src/services/agent-id";
import type {
  DrillResult,
  StatsReport,
} from "../../packages/core/src/services/stats/schema";
import { fixtureEnv, WORK } from "./parity-lib";
import { buildStatsHome, unlockStatsHome } from "./parity-stats-fixtures";
import {
  drillAnswer,
  STATS_AGENTS,
  STATS_DAYS,
  STATS_DRILLS,
  STATS_MS,
  STATS_RATES,
  STATS_SWEEP,
  statsAnswer,
  statsFmtAnswer,
} from "./parity-ui-stats";

/** The stats fixture clock: every fixture transcript is in its window. */
const STATS_NOW = Date.parse("2026-11-14T00:00:00Z");
const STATS_SCOPES: readonly (AgentId | null)[] = [
  null,
  "claude",
  "codex",
  "pi",
  "opencode",
];

type Pipe = (bin: string, lines: readonly string[]) => string[] | null;

interface Deps {
  readonly bin: string;
  readonly compare: Compare;
  readonly digest: (x: unknown) => string;
  readonly listHeaders: (env: Record<string, string>) => {
    agent: string;
    id: string;
    title?: string | null;
  }[];
  readonly pipe: Pipe;
  readonly tsAnswer: TsAnswer;
}

/** Report and drill cases a set asks for. */
interface StatsSet {
  readonly agents: readonly (AgentId | null)[];
  readonly days: readonly number[];
  readonly drills: boolean;
  readonly env: Record<string, string>;
  readonly label: string;
  readonly now: number;
  readonly show: boolean;
}

type Compare = (
  label: string,
  ts: unknown,
  rust: unknown,
  show: boolean
) => void;
type TsAnswer = (key: string, answer: () => unknown) => unknown;

/** Compare each part of an answer on its own, so a diff names the part. */
const compareParts = (
  compare: Compare,
  label: string,
  ts: unknown,
  rust: unknown,
  show: boolean
) => {
  const t = (ts ?? {}) as Record<string, unknown>;
  const r = (rust ?? {}) as Record<string, unknown>;
  for (const k of Object.keys(t)) {
    compare(`${label} ${k}`, t[k], r[k], show);
  }
};

export interface StatsCounts {
  drills: number;
  helpers: number;
  prompts: number;
  reports: number;
}

/** `agent:id` → title from the Rust listing, as the inspector's atom keys them. */
export const titlesOf = (
  headers: readonly { agent: string; id: string; title?: string | null }[]
) =>
  Object.fromEntries(
    headers.flatMap((h) => (h.title ? [[`${h.agent}:${h.id}`, h.title]] : []))
  );

export const runUiStats = (
  bin: string,
  pipe: Pipe,
  set: StatsSet & { readonly titles: Record<string, string> },
  compare: Compare,
  tsAnswer: TsAnswer,
  digest: (x: unknown) => string,
  counts: StatsCounts
) => {
  process.env.TZ = "UTC";
  const titles: SessionTitles = new Map(Object.entries(set.titles));
  const base = {
    home: set.env.HOME,
    dir: set.env.PEEKTRACE_DIR,
    now: set.now,
    titles: set.titles,
  };
  const scopes = set.days.flatMap((days) =>
    set.agents.map((agent) => ({ days, agent }))
  );
  const reqs = scopes.map((s) =>
    JSON.stringify({ fn: "stats", ...base, ...s })
  );
  const out = pipe(bin, reqs);
  if (!out) {
    return;
  }
  const drillReqs: { label: string; req: Record<string, unknown> }[] = [];
  scopes.forEach((scope, i) => {
    const label = `ui stats ${set.label} ${scope.days}d ${scope.agent ?? "all"}`;
    const rust = JSON.parse(out[i] ?? "null") as {
      report?: StatsReport;
      error?: string;
    } | null;
    if (!rust?.report) {
      compare(
        label,
        { report: "ok" },
        { report: rust?.error ?? "missing" },
        true
      );
      return;
    }
    const report = rust.report;
    const ts = tsAnswer(
      `${label} ${digest([JSON.stringify(report), set.titles])}`,
      () => statsAnswer(report, scope.agent, titles)
    );
    if (ts === undefined || ts === null) {
      return;
    }
    compareParts(compare, label, ts, rust, set.show);
    counts.reports++;
    counts.prompts +=
      1 + report.causes.length + report.fails.length + report.time.length;
    counts.helpers += findingsOf(report).length;
    if (!set.drills || scope.agent !== null) {
      return;
    }
    const tables = [
      ["cause", report.causes.map((r) => r.key)],
      ["fail", report.fails.map((r) => r.key)],
      ["time", report.time.map((r) => r.key)],
      ["cluster", report.clusters.map((r) => r.key)],
    ] as const;
    for (const [table, keys] of tables) {
      keys.slice(0, 3).forEach((key, k) => {
        drillReqs.push({
          label: `ui drill ${set.label} ${scope.days}d ${table} ${k}`,
          req: { fn: "drill", ...base, ...scope, table, key, offset: 0 },
        });
      });
    }
  });
  if (drillReqs.length === 0) {
    return;
  }
  const first = pipe(
    bin,
    drillReqs.map((d) => JSON.stringify(d.req))
  );
  if (!first) {
    return;
  }
  /** One later page per table: past 200 when there are more, else the end. */
  const later: typeof drillReqs = [];
  const paged = new Set<string>();
  const check = (cases: typeof drillReqs, lines: readonly string[]) => {
    cases.forEach((c, i) => {
      const rust = JSON.parse(lines[i] ?? "null") as {
        drill?: DrillResult;
        error?: string;
      } | null;
      if (!rust?.drill) {
        compare(
          c.label,
          { drill: "ok" },
          { drill: rust?.error ?? "missing" },
          true
        );
        return;
      }
      const d = rust.drill;
      const ts = tsAnswer(
        `${c.label} ${digest([JSON.stringify(d), set.titles])}`,
        () => drillAnswer(d, titles)
      );
      if (ts !== undefined && ts !== null) {
        compareParts(compare, c.label, ts, rust, set.show);
        counts.drills++;
      }
      const table = String(c.req.table);
      if (c.req.offset === 0 && !paged.has(table)) {
        paged.add(table);
        later.push({
          label: `${c.label} page`,
          req: {
            ...c.req,
            offset: d.total > DRILL_PAGE ? DRILL_PAGE : d.total,
          },
        });
      }
    });
  };
  check(drillReqs, first);
  const second =
    later.length > 0
      ? pipe(
          bin,
          later.map((d) => JSON.stringify(d.req))
        )
      : [];
  if (second) {
    const cases = [...later];
    later.length = 0;
    check(cases, second);
  }
};

/** The Stats helpers: formats on a sweep, then reports and drill pages on
 * the stats fixture HOME and the real corpus. */
export const runStats = (
  deps: Deps,
  { fixtures, real }: { fixtures: boolean; real: boolean }
) => {
  const { bin, compare, digest, listHeaders, pipe, tsAnswer } = deps;
  const counts: StatsCounts & { fmt: number } = {
    drills: 0,
    helpers: 0,
    prompts: 0,
    reports: 0,
    fmt: 0,
  };
  const fmtReq = {
    fn: "statsfmt",
    xs: STATS_SWEEP,
    ms: STATS_MS,
    rates: STATS_RATES,
    days: STATS_DAYS,
    agents: STATS_AGENTS,
    drills: STATS_DRILLS,
  };
  const fmt = pipe(bin, [JSON.stringify(fmtReq)]);
  if (fmt) {
    process.env.TZ = "UTC";
    compare(
      "ui statsfmt",
      tsAnswer(`ui statsfmt ${digest(fmtReq)}`, statsFmtAnswer),
      JSON.parse(fmt[0] ?? "null"),
      true
    );
    counts.fmt =
      STATS_SWEEP.length * 4 +
      STATS_MS.length +
      STATS_RATES.length +
      STATS_DAYS.length +
      STATS_AGENTS.length +
      STATS_DRILLS.length;
  }
  if (fixtures) {
    const home = buildStatsHome(join(WORK, "ui-stats"));
    const env = fixtureEnv(home);
    runUiStats(
      bin,
      pipe,
      {
        agents: STATS_SCOPES,
        days: [36_500, 7],
        drills: true,
        env,
        label: "fixtures",
        now: STATS_NOW,
        show: true,
        titles: titlesOf(listHeaders(env)),
      },
      compare,
      tsAnswer,
      digest,
      counts
    );
    unlockStatsHome(home);
  }
  if (real) {
    const env = {
      PATH: process.env.PATH ?? "",
      HOME: process.env.HOME ?? "",
      TZ: "UTC",
      PEEKTRACE_DIR: join(WORK, "ui-real-stats"),
      PEEKTRACE_NO_TELEMETRY: "1",
    };
    runUiStats(
      bin,
      pipe,
      {
        agents: [null],
        days: [60],
        drills: true,
        env,
        label: "real",
        now: Date.now(),
        show: false,
        titles: titlesOf(listHeaders(env)),
      },
      compare,
      tsAnswer,
      digest,
      counts
    );
  }
  return counts;
};
