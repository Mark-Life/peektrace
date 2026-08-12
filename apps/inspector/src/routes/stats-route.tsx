/** Cross-session friction: what fails, and what costs time.
 *
 * One route, one call, two ranked tables and no blended score — a failure count
 * and a second are different units, and the seconds are carried by a thin tail
 * of long runs, so adding them would invent an ordering nobody measured. The
 * flagged findings come first because a number with no action attached is
 * decoration; the charts sit under the tables as secondary evidence.
 *
 * Window, agent and the open drill key all ride the hash, so any view here is a
 * link someone else can open.
 */
import type { Atom, Result } from "@effect-atom/atom-react";
import { useAtomValue } from "@effect-atom/atom-react";
import type { AgentId } from "@workspace/core/services/agent-id";
import type {
  DrillResult,
  DrillTable,
  StatsReport,
} from "@workspace/core/services/stats/schema";
import type { WireError } from "@workspace/rpc/contract";
import { useState } from "react";
import { ResultView } from "../lib/result-view";
import { setHashParam, useHashParam } from "../lib/routes";
import {
  useStatsDrill,
  useStatsRefresh,
  useStatsReport,
} from "../lib/stats-atoms";
import { findingsOf } from "../lib/stats-format";
import { DrillPanel } from "./stats/drill-panel";
import { Clusters, Waiting } from "./stats/evidence";
import { FailTable } from "./stats/fail-table";
import { Findings } from "./stats/findings";
import { agentOf, daysOf, ReportHeader } from "./stats/report-header";
import { TimeTable } from "./stats/time-table";

/** One open drill: which table, which key. */
interface DrillTarget {
  readonly key: string;
  readonly table: DrillTable;
}

/** A drill key can hold colons; only the first one separates the table. */
const parseDrill = (raw: string | null): DrillTarget | null => {
  if (raw === null) {
    return null;
  }
  const cut = raw.indexOf(":");
  const table = cut < 0 ? "" : raw.slice(0, cut);
  const key = cut < 0 ? "" : raw.slice(cut + 1);
  const known =
    table === "fail" ||
    table === "time" ||
    table === "cluster" ||
    table === "wait";
  return known && key.length > 0 ? { table, key } : null;
};

/** The detector id of the row a drill came from, so the sessions it opens are
 *  marked with the same glyph. Only the fail table names one. */
const detectorOf = (
  report: StatsReport,
  drill: DrillTarget | null
): string | null => {
  if (drill?.table !== "fail") {
    return null;
  }
  return report.fails.find((row) => row.key === drill.key)?.detector ?? null;
};

/** A section heading that states its ranking key: the ranking is the opinion. */
const SectionHead = ({
  by,
  title,
}: {
  readonly by: string;
  readonly title: string;
}) => (
  <div className="flex flex-wrap items-baseline gap-2">
    <h2 className="font-semibold text-base">{title}</h2>
    <span className="text-muted-foreground text-xs">ranked by {by}</span>
  </div>
);

/** The screen for one loaded report. */
const ReportView = ({
  agent,
  days,
  drill,
  drillAtom,
  onPage,
  onRescan,
  report,
}: {
  readonly agent: AgentId | null;
  readonly days: number;
  readonly drill: DrillTarget | null;
  readonly drillAtom: Atom.Atom<Result.Result<DrillResult, WireError>> | null;
  readonly onPage: (offset: number) => void;
  readonly onRescan: () => void;
  readonly report: StatsReport;
}) => {
  const [showAllFindings, setShowAllFindings] = useState(false);
  const openDrill = (table: DrillTable, key: string) =>
    setHashParam("drill", `${table}:${key}`);
  const keyOf = (table: DrillTable) =>
    drill?.table === table ? drill.key : null;
  return (
    <>
      <ReportHeader
        agent={agent}
        days={days}
        onAgent={(next) => setHashParam("agent", next)}
        onDays={(next) => setHashParam("days", String(next))}
        onRescan={onRescan}
        report={report}
      />

      <section className="flex flex-col gap-3">
        <SectionHead
          by="cheapest fix first, then by each table's own impact"
          title="Flagged findings"
        />
        <Findings
          findings={findingsOf(report)}
          onDrill={openDrill}
          onShowAll={() => setShowAllFindings(true)}
          showAll={showAllFindings}
        />
      </section>

      <section className="flex flex-col gap-3">
        <SectionHead by="sessions spanned, then calls" title="What fails" />
        <p className="text-muted-foreground text-xs">
          A row says a call printed a failure — never that something broke.
          Flagged counts the calls that set the agent's own error flag; the gap
          between it and Calls is what an error panel alone would miss.
        </p>
        <FailTable
          onSelect={(key) => openDrill("fail", key)}
          rows={report.fails}
          selectedKey={keyOf("fail")}
        />
      </section>

      <section className="flex flex-col gap-3">
        <SectionHead by="seconds charged" title="What costs time" />
        <p className="text-muted-foreground text-xs">
          Each command line is charged to exactly one job; Co-present counts the
          lines that hold the job and were charged to a higher-priority one.
          Tools that wait on a human are out of the denominator.
        </p>
        <TimeTable
          onSelect={(key) => openDrill("time", key)}
          rows={report.time}
          selectedKey={keyOf("time")}
        />
      </section>

      <section className="flex flex-col gap-3">
        <SectionHead by="seconds parked" title="Waiting" />
        <Waiting panel={report.waiting} />
      </section>

      <section className="flex flex-col gap-3">
        <SectionHead by="rows, then sessions spanned" title="Error clusters" />
        <Clusters
          onSelect={(key) => openDrill("cluster", key)}
          rows={report.clusters}
          selectedKey={keyOf("cluster")}
        />
      </section>

      <DrillPanel
        atom={drillAtom}
        detector={detectorOf(report, drill)}
        onClose={() => setHashParam("drill", null)}
        onPage={onPage}
        title={drill === null ? "" : drill.key}
      />
    </>
  );
};

/** The stats screen. */
export const StatsRoute = () => {
  const days = daysOf(useHashParam("days"));
  const agent = agentOf(useHashParam("agent"));
  const drill = parseDrill(useHashParam("drill"));
  const scope = { agent, days };
  const reportAtom = useStatsReport(scope);
  const rescan = useStatsRefresh(reportAtom);
  const report = useAtomValue(reportAtom);
  // The page resets by identity, not by an effect: a different row is a
  // different id, so its offset reads 0 without a write.
  const [page, setPage] = useState({ id: "", offset: 0 });
  const drillId = drill === null ? "" : `${drill.table}:${drill.key}`;
  const offset = page.id === drillId ? page.offset : 0;
  const drillAtom = useStatsDrill(
    drill === null ? null : { ...drill, offset },
    scope
  );
  return (
    <div className="flex flex-col gap-8" data-testid="stats-route">
      <ResultView
        pending={
          <p
            className="text-muted-foreground text-sm"
            data-testid="stats-pending"
          >
            Reading every transcript on disk. The first pass parses them all;
            the next reads the fact cache and only re-parses what changed.
          </p>
        }
        result={report}
      >
        {(value) => (
          <ReportView
            agent={agent}
            days={days}
            drill={drill}
            drillAtom={drillAtom}
            onPage={(next) => setPage({ id: drillId, offset: next })}
            onRescan={rescan}
            report={value}
          />
        )}
      </ResultView>
    </div>
  );
};
