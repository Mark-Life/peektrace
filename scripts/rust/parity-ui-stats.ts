/** The `ui` stage's Stats questions: the app's stats helpers
 * (`ui_helpers` `statsfmt`, `stats`, `drill`) against the inspector's own
 * functions, on the stats fixture HOME and the real corpus.
 *
 * Rust answers with the core report or drill page it ran, plus every text
 * the app derives from it. The TS side parses that same report and runs the
 * inspector's helpers over it: findings, prompts, rules, chips, rank rows.
 * Cells the inspector writes inline in JSX (cause, fail and time columns,
 * the drill page note) are spelled here with the same `n`/`hms`/`pct`. */

import { DRILL_PAGE } from "../../apps/inspector/src/lib/stats-atoms";
import {
  at,
  chipLabelOf,
  coPresentNote,
  findingsOf,
  hms,
  n,
  pct,
  promptsOf,
  ruleLine,
  type SessionTitles,
  sessionChips,
  suggestedRules,
  timeNote,
} from "../../apps/inspector/src/lib/stats-format";
import { drillLabel } from "../../apps/inspector/src/routes/stats/drill-panel";
import {
  CLUSTER_ROWS,
  clusterRows,
  shapeRows,
} from "../../apps/inspector/src/routes/stats/evidence";
import { VISIBLE } from "../../apps/inspector/src/routes/stats/findings";
import {
  agentOf,
  daysOf,
} from "../../apps/inspector/src/routes/stats/report-header";
import { titleOf } from "../../apps/inspector/src/routes/stats/session-links";
import {
  detectorOf,
  parseDrill,
} from "../../apps/inspector/src/routes/stats-route";
import type { AgentId } from "../../packages/core/src/services/agent-id";
import { RETRY_WINDOW } from "../../packages/core/src/services/stats/detectors/retry-gates";
import {
  rateText,
  suggestedRuleOf,
} from "../../packages/core/src/services/stats/findings";
import type {
  DrillHit,
  DrillResult,
  SampleRef,
  StatsReport,
} from "../../packages/core/src/services/stats/schema";
import { widthOf } from "../../packages/viz/src/components/rank-bar";
import { fmtK } from "../../packages/viz/src/lib/session-format";

export const STATS_SWEEP = [
  0, 0.4, 9.995, 10, 42, 59.4, 59.6, 60, 119.6, 187, 3599.6, 3600, 18_277, -1,
  1e6, 0.000_05, 1234.5678, 0.16, 0.1571, 1, 1e21,
];
export const STATS_MS = [
  Date.parse("2026-11-14T09:11:55.696Z"),
  Date.parse("2026-11-14T00:00:00Z"),
  Date.parse("2026-11-14T12:00:00Z"),
  Date.parse("1999-01-02T23:59:59.999Z"),
];
export const STATS_RATES: [number, number][] = [
  [0, 0],
  [0.001, 1],
  [0.125, 1],
  [3.33, 2],
  [12.5, 1],
  [26.3, 125],
  [1234.5, 9],
];
export const STATS_DAYS = ["7", "30", "60", "90", "31", "", "abc", "60.0"];
export const STATS_AGENTS = ["claude", "codex", "pi", "opencode", "all", "x"];
export const STATS_DRILLS = [
  "cause:cause:zsh",
  "cause:",
  "nope:x",
  "fail:scan:tsc",
  "wait:loop-poll",
  "cluster:a:b",
  "time",
];

export const statsFmtAnswer = () => ({
  hms: STATS_SWEEP.map(hms),
  pct: STATS_SWEEP.map(pct),
  n: STATS_SWEEP.map(n),
  fmtK: STATS_SWEEP.map(fmtK),
  rate: STATS_RATES.map(([r, h]) => rateText(r, h)),
  at: STATS_MS.map(at),
  days: STATS_DAYS.map((d) => daysOf(d)),
  agent: STATS_AGENTS.map((a) => agentOf(a)),
  drill: STATS_DRILLS.map((d) => parseDrill(d)),
});

const record = (m: Iterable<readonly [string, string | null]>) =>
  Object.fromEntries(
    [...m].filter((e): e is [string, string] => e[1] !== null)
  );

const chips = (samples: readonly SampleRef[], titles: SessionTitles) => ({
  groups: sessionChips(samples).map((g) => {
    const c = chipLabelOf(g.first, titles);
    return {
      label: c.label,
      id: c.id,
      project: c.project,
      session: c.session,
      chipSubagent: c.subagent,
      subagent: g.samples.some((s) => s.parent !== null),
      title: titleOf(g.samples),
      targets: g.samples.map(
        (s) => `${s.parent === null ? "main" : `subagent ${s.sid}`} · ${s.line}`
      ),
    };
  }),
  line: samples[0]?.line ?? null,
});

interface Ranked {
  readonly key: string;
  readonly label: string;
  readonly secondary?: string;
  readonly value: number;
  readonly valueText?: string;
}

const rank = (rows: readonly Ranked[], max: number) =>
  rows.map((r) => ({
    key: r.key,
    label: r.label,
    valueText: r.valueText ?? r.value.toLocaleString(),
    secondary: r.secondary ?? "",
    width: widthOf(r.value, max),
  }));

const retries = (failing: boolean) => (failing ? "retries" : "read-backs");

const causeView = (
  r: StatsReport,
  row: StatsReport["causes"][number],
  titles: SessionTitles
) => ({
  key: row.key,
  badge: row.key.replace("cause:", ""),
  cells: [
    n(row.hits),
    n(row.pairs),
    `${n(row.retrySessions)} sessions`,
    `${n(row.hitsBy.main)} / ${n(row.hitsBy.nested)}`,
    row.secMeasured === 0 ? "—" : hms(row.retrySec),
    row.tokensMeasured === 0 ? "—" : fmtK(row.retryCtxTokens),
  ],
  narrow: `${n(row.hits)} hits · ${n(row.pairs)} ${retries(row.failing)} in ${n(row.retrySessions)} sessions${row.secMeasured === 0 ? "" : ` · ${hms(row.retrySec)}`}${row.tokensMeasured === 0 ? "" : ` · ${fmtK(row.retryCtxTokens)} ctx re-read`}`,
  split: `hits main ${n(row.hitsBy.main)} / sub ${n(row.hitsBy.nested)} · per 1k calls in this window main ${rateText(row.per1k.main, row.hitsBy.main)} / sub ${rateText(row.per1k.nested, row.hitsBy.nested)} · ${retries(row.failing)} main ${n(row.pairsBy.main)} / sub ${n(row.pairsBy.nested)} · exit 0 ${n(row.hits - row.flagged)} · context re-read ${row.tokensMeasured === 0 ? "not measured" : fmtK(row.retryCtxTokens)}`,
  learned:
    row.diffs.length === 0
      ? null
      : `learned: ${row.diffs.map((d) => `${d.from} → ${d.to} (${n(d.pairs)})`).join(" · ")}`,
  named:
    row.details.length === 0
      ? null
      : `named: ${row.details.map((d) => `${d.value} ${n(d.hits)}`).join(" · ")}`,
  pairs: row.pairSamples.map((p) => `${p.failed.line} → ${p.retry.line}`),
  matches: (row.rule?.matches ?? []).map(
    (m) => `${m.path}:${m.line} ${m.excerpt}`
  ),
  detector: detectorOf(r, { table: "cause", key: row.key }),
  chips: chips(row.samples, titles),
});

const viewOf = (r: StatsReport, titles: SessionTitles) => {
  const w = r.waiting;
  return {
    corpus: `${n(r.corpus.transcripts)} transcripts · ${n(r.corpus.projects)} projects · ${n(r.corpus.toolCalls)} tool calls · ${n(r.corpus.bashCalls)} shell calls · ${hms(r.corpus.bashSec)} of shell time`,
    window: `${r.window.fromIso.slice(0, 10)} → ${r.window.toIso.slice(0, 10)} · generated ${at(r.generatedAt)} · ${r.schemaVersion} · detector v${r.detectorVersion}`,
    caveats: `${n(r.caveats.length)} caveats travel with these numbers${r.skipped.length > 0 ? ` · ${n(r.skipped.length)} files skipped` : ""}`,
    causes: r.causes.map((row) => causeView(r, row, titles)),
    fails: r.fails.map((row) => ({
      key: row.key,
      cells: [n(row.sessions), n(row.rows), n(row.flagged), n(row.projects)],
      detector: detectorOf(r, { table: "fail", key: row.key }),
      chips: chips(row.samples, titles),
    })),
    time: r.time.map((row) => ({
      key: row.key,
      badges: [
        ...(row.cappedRuns > 0 ? [`${n(row.cappedRuns)} capped`] : []),
        ...(row.bgRuns > 0 ? [`${n(row.bgRuns)} bg`] : []),
      ],
      cells: [
        hms(row.totalSec),
        pct(row.share),
        n(row.runs),
        n(row.coPresentRuns),
        hms(row.medianSec),
        hms(row.p95Sec),
      ],
      coPresent: coPresentNote(row),
      bg:
        row.bgRuns > 0
          ? `backgrounded: ${n(row.bgRuns)} runs, ${hms(row.bgSec)} charged`
          : null,
      chips: chips(row.samples, titles),
    })),
    wait: {
      figures: [
        [hms(w.totalSec), "waiting"],
        [pct(w.share), "of bash time"],
        [n(w.calls), "calls"],
        [n(w.sessions), "sessions"],
        [hms(w.medianSec), "median"],
        [hms(w.p95Sec), "p95"],
      ],
      rows: rank(shapeRows(w), w.totalSec),
      note:
        w.bgRuns > 0
          ? `${n(w.bgRuns)} backgrounded waits are charged ${hms(w.bgSec)} while declaring ${hms(w.bgDeclaredSec)} of literal sleep — the declared seconds are printed here and never summed into a total.`
          : null,
    },
    clusters: {
      rows: rank(clusterRows(r.clusters), r.clusters[0]?.rows ?? 1),
      more:
        r.clusters.length > CLUSTER_ROWS
          ? `${n(r.clusters.length - CLUSTER_ROWS)} more clusters not shown.`
          : null,
    },
  };
};

/** The inspector's answer for the report Rust ran. */
export const statsAnswer = (
  report: StatsReport,
  agent: AgentId | null,
  titles: SessionTitles
) => {
  const prompt = promptsOf(report, agent);
  const findings = findingsOf(report);
  const samplesOf = (f: (typeof findings)[number]) => f.samples;
  return {
    text: {
      findings: findings.map((f) => ({
        detector: f.detector,
        impact: f.impact,
        key: f.key,
        note: f.note,
        rule: f.rule,
        spread: f.spread,
        table: f.table,
        title: f.title,
      })),
      suggested: suggestedRules(report),
      prompts: {
        report: prompt({ table: "report" }),
        cause: record(
          report.causes.map((r) => [
            r.key,
            prompt({ table: "cause", key: r.key }),
          ])
        ),
        fail: record(
          report.fails.map((r) => [
            r.key,
            prompt({ table: "fail", key: r.key }),
          ])
        ),
        time: record(
          report.time.map((r) => [r.key, prompt({ table: "time", key: r.key })])
        ),
      },
      ruleLines: record(
        report.causes.map((r) => [
          r.key,
          r.rule === null ? null : ruleLine(r.rule),
        ])
      ),
      suggestedRule: record(
        report.causes.map((r) => [r.key, suggestedRuleOf(r)])
      ),
      timeNotes: record(report.time.map((r) => [r.key, timeNote(r)])),
      generated: at(report.generatedAt),
    },
    view: viewOf(report, titles),
    showMore:
      findings.length > VISIBLE
        ? `Show ${findings.length - VISIBLE} more`
        : null,
    findingChips: findings.map((f) => chips(samplesOf(f), titles)),
    retryWindow: RETRY_WINDOW,
  };
};

/** The inspector's answer for the drill page Rust ran. */
export const drillAnswer = (d: DrillResult, titles: SessionTitles) => {
  const next = d.offset + d.hits.length;
  const left = d.total - next;
  return {
    hits: d.hits.map((h: DrillHit) => ({
      label: drillLabel(h, titles),
      dur: h.durSec === null ? "—" : hms(h.durSec),
      meta: `${drillLabel(h, titles)} · ${h.kind} · ${h.ts ?? "no timestamp"}`,
      title: `${h.project} · ${h.parent ?? h.sid}`,
    })),
    note: `${n(d.offset + 1)}–${n(next)} of ${n(d.total)} calls`,
    more:
      left > 0 ? [`Load ${n(Math.min(left, DRILL_PAGE))} more`, next] : null,
    back: d.offset > 0 ? Math.max(0, d.offset - DRILL_PAGE) : null,
  };
};
