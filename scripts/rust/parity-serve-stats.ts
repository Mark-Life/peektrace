/** Serve-stage stats cases: report, drill, prompt, markers, usage and
 * refresh over `/rpc` and `/api`, in a fixed order on one shared
 * `PEEKTRACE_DIR`, so both caches warm alike. */
import type { AgentId } from "@workspace/core/services/agent-id";
import type { StatsReport } from "@workspace/core/services/stats/schema";
import { promptsOf } from "../../apps/inspector/src/lib/stats-format";
import { report } from "./parity-lib";
import type { Counts } from "./parity-serve-cases";
import {
  compareCall,
  compareKnown,
  dropClock,
  query,
  seg,
  send,
} from "./parity-serve-compare";
import type { Pair, RpcExit } from "./parity-serve-lib";

const AGENTS = ["claude", "codex", "pi", "opencode"] as const;
const ALL_DAYS = 36_500;
const TOP = 3;
const SID = "22222222-2222-4222-8222-222222222222";
type Table = "cause" | "fail" | "time" | "cluster" | "wait";
const CAUSE_PREFIX = /^cause:/;

interface ReportArgs {
  readonly agents?: readonly string[];
  readonly days?: number;
  readonly force?: boolean;
  readonly limit?: number;
}

const reportPath = (a: ReportArgs) =>
  `/api/stats/report${query({ agent: a.agents, days: a.days, force: a.force, limit: a.limit })}`;

const okValue = <A>(exit: RpcExit) =>
  exit.kind === "ok" ? (exit.value as A) : undefined;

/** The first keys of each table, as a drill takes them. */
const tableKeys = (r: StatsReport): [Table, string][] => {
  const keys = (rows: readonly { key: string }[]) =>
    rows.slice(0, TOP).map((x) => x.key);
  return [
    ...keys(r.causes).map((k): [Table, string] => ["cause", k]),
    ...keys(r.fails).map((k): [Table, string] => ["fail", k]),
    ...keys(r.time).map((k): [Table, string] => ["time", k]),
    ...keys(r.clusters).map((k): [Table, string] => ["cluster", k]),
    ...keys(r.waiting.byShape).map((k): [Table, string] => ["wait", k]),
  ];
};

const reportCase = (c: Counts, pair: Pair, label: string, a: ReportArgs) =>
  compareCall(
    c,
    pair,
    `stats.report ${label}`,
    { tag: "stats.report", payload: a, path: reportPath(a) },
    { norm: dropClock }
  );

const refreshCase = (c: Counts, pair: Pair, label: string, body: object) =>
  compareCall(c, pair, `stats.refresh ${label}`, {
    tag: "stats.refresh",
    payload: body,
    path: "/api/stats/refresh",
    init: send("POST", body),
  });

interface Drill {
  readonly key: string;
  readonly offset?: number;
  readonly pageSize?: number;
  readonly table: string;
}

const drillCase = (c: Counts, pair: Pair, label: string, d: Drill) =>
  compareCall(c, pair, `stats.drill ${label}`, {
    tag: "stats.drill",
    payload: { days: ALL_DAYS, ...d },
    path: `/api/stats/drill/${seg(d.table)}/${seg(d.key)}${query({
      days: ALL_DAYS,
      offset: d.offset,
      pageSize: d.pageSize,
    })}`,
  });

/** The inspector's "Copy prompt" text from the TS and the Rust report
 * must match; the Rust report takes the TS clock fields first. */
const promptCases = (
  c: Counts,
  ts: StatsReport,
  rust: StatsReport,
  agent: AgentId | null
) => {
  const aligned = {
    ...rust,
    generatedAt: ts.generatedAt,
    window: ts.window,
  } as StatsReport;
  const tsPrompt = promptsOf(ts, agent);
  const rustPrompt = promptsOf(aligned, agent);
  const targets = [
    { table: "report" as const },
    ...ts.causes
      .slice(0, TOP)
      .map((r) => ({ table: "cause" as const, key: r.cause })),
    ...ts.fails
      .slice(0, TOP)
      .map((r) => ({ table: "fail" as const, key: r.key })),
    ...ts.time
      .slice(0, TOP)
      .map((r) => ({ table: "time" as const, key: r.key })),
  ];
  for (const target of targets) {
    c.cases += 1;
    c.features += 1;
    if (tsPrompt(target) !== rustPrompt(target)) {
      report(`serve stats prompt ${JSON.stringify(target)}: text differs`);
    }
  }
};

const markerCases = async (c: Counts, pair: Pair, r: StatsReport) => {
  const sids = [
    ...new Set([...Object.keys(r.paths), SID, "agent-a1", "no-such-session"]),
  ];
  for (const sid of sids) {
    await compareCall(c, pair, `stats.markers ${sid}`, {
      tag: "stats.markers",
      payload: { sid },
      path: `/api/sessions/${seg(sid)}/markers`,
    });
  }
  await compareCall(c, pair, "stats.markers codex", {
    tag: "stats.markers",
    payload: { sid: SID, agents: ["codex"] },
    path: `/api/sessions/${SID}/markers?agent=codex`,
  });
};

const usageCases = async (c: Counts, pair: Pair) => {
  for (const by of ["day", "month", "block"]) {
    for (const tz of ["UTC", "Asia/Tokyo"]) {
      for (const days of [undefined, ALL_DAYS]) {
        const payload = { by, tz, ...(days === undefined ? {} : { days }) };
        await compareCall(
          c,
          pair,
          `stats.usage ${by} ${tz} ${days ?? "default"}`,
          {
            tag: "stats.usage",
            payload,
            path: `/api/stats/usage${query(payload)}`,
          },
          { norm: dropClock, retry: true }
        );
      }
    }
  }
  const bad: [string, object][] = [
    ["tz", { by: "day", tz: "Mars/Olympus" }],
    ["days 0", { by: "day", days: 0 }],
    ["days 1.5", { by: "day", days: 1.5 }],
    ["no by", {}],
  ];
  for (const [label, payload] of bad) {
    await compareCall(c, pair, `stats.usage ${label}`, {
      tag: "stats.usage",
      payload,
      path: `/api/stats/usage${query(payload as Record<string, string>)}`,
    });
  }
};

/** Every stats route on the fixture HOME, in the order both caches need. */
export const statsCases = async (c: Counts, pair: Pair) => {
  await refreshCase(c, pair, "cold", {});
  const wide = await reportCase(c, pair, "36500", { days: ALL_DAYS });
  await reportCase(c, pair, "{}", {});
  for (const a of AGENTS) {
    await reportCase(c, pair, a, { days: ALL_DAYS, agents: [a] });
  }
  await reportCase(c, pair, "limit 1", { days: ALL_DAYS, limit: 1 });
  await reportCase(c, pair, "limit -1", { days: ALL_DAYS, limit: -1 });
  await reportCase(c, pair, "days 0.5", { days: 0.5 });
  await reportCase(c, pair, "force", { days: ALL_DAYS, force: true });
  await compareKnown(
    c,
    pair,
    "stats.report days 1e12",
    {
      tag: "stats.report",
      payload: { days: 1e12 },
      path: reportPath({ days: 1e12 }),
    },
    { status: 500, tag: "StatsScanError" }
  );
  const ts = okValue<StatsReport>(wide.exit);
  if (ts === undefined) {
    report("serve stats.report: TS report failed, drill keys skipped");
    return;
  }
  for (const [table, key] of tableKeys(ts)) {
    await drillCase(c, pair, `${table} ${key}`, { table, key });
  }
  const cause = ts.causes[0]?.key ?? "cause:zsh-glob-path";
  const page = { table: "cause", key: cause };
  await drillCase(c, pair, "bare cause id", {
    table: "cause",
    key: cause.replace(CAUSE_PREFIX, ""),
  });
  await drillCase(c, pair, "pageSize 1", { ...page, pageSize: 1 });
  await drillCase(c, pair, "offset 1", { ...page, offset: 1, pageSize: 1 });
  await drillCase(c, pair, "offset 99", { ...page, offset: 99 });
  await drillCase(c, pair, "clamped", { ...page, offset: -3.7, pageSize: 0.2 });
  await drillCase(c, pair, "unknown key", { table: "fail", key: "nope" });
  await drillCase(c, pair, "wait all", { table: "wait", key: "*" });
  await drillCase(c, pair, "bogus table", { table: "bogus", key: "x" });
  promptCases(c, ts, wide.rust.json as StatsReport, null);
  await markerCases(c, pair, ts);
  await usageCases(c, pair);
  await refreshCase(c, pair, "claude", { agents: ["claude"] });
  await refreshCase(c, pair, "force", { force: true });
};

/** Stats on the real corpus snapshot: refresh, report, usage, a drill per
 * table and the newest sessions' markers. */
export const realStatsCases = async (
  c: Counts,
  pair: Pair,
  newest: readonly string[]
) => {
  const counts = (v: unknown) => {
    const s = v as { transcripts?: number; skipped?: unknown; facts?: number };
    return { transcripts: s.transcripts, skipped: s.skipped, facts: s.facts };
  };
  await compareCall(
    c,
    pair,
    "real stats.refresh",
    {
      tag: "stats.refresh",
      payload: {},
      path: "/api/stats/refresh",
      init: send("POST", {}),
    },
    { norm: counts }
  );
  const def = await compareCall(
    c,
    pair,
    "real stats.report",
    { tag: "stats.report", payload: {}, path: "/api/stats/report" },
    { norm: dropClock, retry: true }
  );
  await compareCall(
    c,
    pair,
    "real stats.report claude",
    {
      tag: "stats.report",
      payload: { agents: ["claude"] },
      path: "/api/stats/report?agent=claude",
    },
    { norm: dropClock, retry: true }
  );
  await compareCall(
    c,
    pair,
    "real stats.usage day",
    {
      tag: "stats.usage",
      payload: { by: "day" },
      path: "/api/stats/usage?by=day",
    },
    { norm: dropClock, retry: true }
  );
  const ts = okValue<StatsReport>(def.exit);
  if (ts !== undefined) {
    const firsts = new Map<Table, string>();
    for (const [table, key] of tableKeys(ts)) {
      if (!firsts.has(table)) {
        firsts.set(table, key);
      }
    }
    for (const [table, key] of firsts) {
      await compareCall(
        c,
        pair,
        `real stats.drill ${table} ${key}`,
        {
          tag: "stats.drill",
          payload: { table, key },
          path: `/api/stats/drill/${table}/${seg(key)}`,
        },
        { retry: true }
      );
    }
  }
  for (const sid of newest) {
    await compareCall(
      c,
      pair,
      `real stats.markers ${sid}`,
      {
        tag: "stats.markers",
        payload: { sid },
        path: `/api/sessions/${seg(sid)}/markers`,
      },
      { retry: true }
    );
  }
};
