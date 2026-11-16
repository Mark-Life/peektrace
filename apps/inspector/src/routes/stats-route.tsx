/** Cross-session friction: which named causes cost retries, what fails, and
 * what costs time.
 *
 * One route, one call, ranked tables and no blended score — a failure count
 * and a second are different units, and the seconds are carried by a thin tail
 * of long runs, so adding them would invent an ordering nobody measured. The
 * flagged findings come first because a number with no action attached is
 * decoration; the charts sit under the tables as secondary evidence.
 *
 * Window, agent and the open drill key all ride the hash, so any view here is a
 * link someone else can open.
 */
import { useAtomValue } from "@effect-atom/atom-react";
// oxlint-disable-next-line import/no-duplicates, eslint/no-duplicate-imports -- type-only import kept separate: the type-specifier-style rule forbids inline `type`
import type { Atom, Result } from "@effect-atom/atom-react";
import type {
  AgentId,
  DrillResult,
  DrillTable,
  StatsReport,
} from "@workspace/view-core/api-types.gen";
import { STATS_SECTIONS } from "@workspace/view-core/constants.gen";
import { useState } from "react";

import type { ApiFailure } from "../lib/api";
import { ResultView } from "../lib/result-view";
import { setHashParam, useHashParam } from "../lib/routes";
import {
  firstPassStart,
  useStatsDrill,
  useStatsRefresh,
  useStatsReport,
} from "../lib/stats-atoms";
import { promptsOf, samplesOf } from "../lib/stats-format";
import { CauseTable } from "./stats/cause-table";
import { DrillPanel } from "./stats/drill-panel";
import { Clusters, Waiting } from "./stats/evidence";
import { FailTable } from "./stats/fail-table";
import { Findings } from "./stats/findings";
import { agentOf, daysOf, ReportHeader } from "./stats/report-header";
import { StatsPending } from "./stats/stats-pending";
import { TimeTable } from "./stats/time-table";

/** One open drill: which table, which key. */
interface DrillTarget {
  readonly key: string;
  readonly table: DrillTable;
}

/** A drill key can hold colons; only the first one separates the table. */
export const parseDrill = (raw: string | null): DrillTarget | null => {
  if (raw === null) {
    return null;
  }

  const cut = raw.indexOf(":");
  const table = cut === -1 ? "" : raw.slice(0, cut);
  const key = cut === -1 ? "" : raw.slice(cut + 1);

  const known =
    table === "cause" ||
    table === "fail" ||
    table === "time" ||
    table === "cluster" ||
    table === "wait";

  return known && key.length > 0 ? { key, table } : null;
};

/** The detector id of the row a drill came from, so the sessions it opens are
 *  marked with the same glyph. The cause and fail tables name one. */
export const detectorOf = (
  report: StatsReport,
  drill: DrillTarget | null
): string | null => {
  if (drill?.table === "cause") {
    return report.causes.find((row) => row.key === drill.key)?.cause ?? null;
  }

  if (drill?.table !== "fail") {
    return null;
  }

  return report.fails.find((row) => row.key === drill.key)?.detector ?? null;
};

/** A section heading that states its ranking key, then its lead note. */
const SectionHead = ({
  section,
}: {
  readonly section: keyof typeof STATS_SECTIONS;
}) => {
  const { note, rankedBy, title } = STATS_SECTIONS[section];

  return (
    <>
      <div className="flex flex-wrap items-baseline gap-2">
        <h2 className="text-base font-semibold">{title}</h2>
        <span className="text-muted-foreground text-xs">
          ranked by {rankedBy}
        </span>
      </div>
      {note === null ? null : (
        <p className="text-muted-foreground text-xs">{note}</p>
      )}
    </>
  );
};

const openDrill = (table: DrillTable, key: string) => {
  setHashParam("drill", `${table}:${key}`);
};

/** The screen for one loaded report. */
const ReportView = ({
  agent,
  days,
  drill,
  drillAtom,
  onPage,
  onRescan,
  report,
  rescanning,
}: {
  readonly agent: AgentId | null;
  readonly days: number;
  readonly drill: DrillTarget | null;
  readonly drillAtom: Atom.Atom<Result.Result<DrillResult, ApiFailure>> | null;
  readonly onPage: (offset: number) => void;
  readonly onRescan: () => void;
  readonly report: StatsReport;
  readonly rescanning: boolean;
}) => {
  const [showAllFindings, setShowAllFindings] = useState(false);

  const keyOf = (table: DrillTable) =>
    drill?.table === table ? drill.key : null;

  const { text } = report;
  const prompt = promptsOf(text);

  return (
    <>
      <ReportHeader
        agent={agent}
        days={days}
        onAgent={(next) => {
          setHashParam("agent", next);
        }}
        onDays={(next) => {
          setHashParam("days", String(next));
        }}
        onRescan={onRescan}
        prompt={prompt({ table: "report" })}
        report={report}
        rescanning={rescanning}
      />

      <section className="flex flex-col gap-3">
        <SectionHead section="findings" />
        <Findings
          findings={text.findings}
          onDrill={openDrill}
          onShowAll={() => {
            setShowAllFindings(true);
          }}
          prompt={prompt}
          samples={(finding) => samplesOf(report, finding.table, finding.key)}
          showAll={showAllFindings}
          suggested={text.suggested}
        />
      </section>

      <section className="flex flex-col gap-3">
        <SectionHead section="causes" />
        <CauseTable
          onSelect={(key) => {
            openDrill("cause", key);
          }}
          prompt={prompt}
          rows={report.causes}
          text={text}
          selectedKey={keyOf("cause")}
        />
      </section>

      <section className="flex flex-col gap-3">
        <SectionHead section="fails" />
        <FailTable
          onSelect={(key) => {
            openDrill("fail", key);
          }}
          prompt={prompt}
          rows={report.fails}
          selectedKey={keyOf("fail")}
        />
      </section>

      <section className="flex flex-col gap-3">
        <SectionHead section="time" />
        <TimeTable
          onSelect={(key) => {
            openDrill("time", key);
          }}
          prompt={prompt}
          rows={report.time}
          timeNotes={text.timeNotes}
          selectedKey={keyOf("time")}
        />
      </section>

      <section className="flex flex-col gap-3">
        <SectionHead section="waiting" />
        <Waiting panel={report.waiting} />
      </section>

      <section className="flex flex-col gap-3">
        <SectionHead section="clusters" />
        <Clusters
          onSelect={(key) => {
            openDrill("cluster", key);
          }}
          rows={report.clusters}
          selectedKey={keyOf("cluster")}
        />
      </section>

      <DrillPanel
        atom={drillAtom}
        detector={detectorOf(report, drill)}
        onClose={() => {
          setHashParam("drill", null);
        }}
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
        onRetry={rescan}
        pending={<StatsPending since={firstPassStart(scope)} />}
        result={report}
      >
        {(value) => (
          <ReportView
            agent={agent}
            days={days}
            drill={drill}
            drillAtom={drillAtom}
            onPage={(next) => {
              setPage({ id: drillId, offset: next });
            }}
            onRescan={rescan}
            report={value}
            rescanning={report.waiting}
          />
        )}
      </ResultView>
    </div>
  );
};
