/** Stats screen — cross-session friction: what fails, and what costs time.
 *
 * One screen, no scrolling: a three-line header carrying the corpus every share
 * divides by, the findings band that names a change, then the two ranked tables
 * stacked so both are on screen at once, then the note strip for the selected
 * row. `enter` opens the calls behind a row, `c` the caveats verbatim.
 *
 * The report is expensive — a cold pass parses every transcript — so the query
 * deliberately leaves the filesystem watch counter out of its deps: a session
 * write never triggers a rescan behind this screen. The header marks the report
 * stale instead, `R` re-reads it (warm, off the fact cache) and `F` re-extracts
 * first. Redaction is never turned off here: the screen shows only the redacted
 * strings core minted.
 */
import { useKeyboard, useTerminalDimensions } from "@opentui/react";
import { AGENT_IDS, type AgentId } from "@workspace/core";
import type { StatsReport } from "@workspace/core/services/stats/schema";
import { useEffect, useMemo, useRef, useState } from "react";
import { useBridge, useQuery, useWatch } from "../runtime";
import { C, clip } from "../theme";
import { useTyping } from "../typing";
import { useListSelection } from "../use-list";
import {
  Caveats,
  FirstPaint,
  Hints,
  OVERLAY_CHROME,
  StatsHeader,
} from "./stats-chrome";
import { StatsDrill } from "./stats-drill";
import {
  agoOf,
  caveatLines,
  columnsOf,
  contentWidth,
  type DrillTarget,
  type Finding,
  failTarget,
  findingsOf,
  findingTarget,
  layoutOf,
  MIN_HEIGHT,
  MIN_WIDTH,
  noteStripOf,
  type Pane,
  scopeText,
  timeTarget,
  WINDOW_DAYS,
} from "./stats-format";
import { StatsBody } from "./stats-tables";

/** Default window: the one core uses when a caller names none, so this screen,
 * `peektrace stats` and the web inspector open on the same report. */
const DEFAULT_DAYS = 60;
/** Header block: three lines plus the blank under it. */
const HEADER_ROWS = 4;
const HINT_ROWS = 1;
/** Smallest overlay worth drawing: a border plus one line. */
const MIN_OVERLAY_ROWS = 3;
/** The overlay's own border rows. */
const OVERLAY_BORDER = 2;

const PANES: readonly Pane[] = ["findings", "fails", "time"];
const AGENT_CYCLE: readonly (AgentId | undefined)[] = [undefined, ...AGENT_IDS];

const messageOf = (err: unknown): string =>
  err instanceof Error ? err.message : String(err);

/** Cycle a value through a fixed list, wrapping at the end. */
const next = <T,>(values: readonly T[], current: T): T =>
  values[(values.indexOf(current) + 1) % values.length] as T;

/**
 * One selection per pane plus the caveats scroll. Only the focused pane's keys
 * are live, and an open overlay freezes all three.
 */
const usePaneSelection = ({
  caveatCount,
  caveats,
  drilling,
  failCount,
  findingCount,
  pane,
  timeCount,
}: {
  readonly caveatCount: number;
  readonly caveats: boolean;
  readonly drilling: boolean;
  readonly failCount: number;
  readonly findingCount: number;
  readonly pane: Pane;
  readonly timeCount: number;
}) => {
  const idle = !(caveats || drilling);
  const [findings, setFindings] = useListSelection(
    findingCount,
    idle && pane === "findings"
  );
  const [fails, setFails] = useListSelection(
    failCount,
    idle && pane === "fails"
  );
  const [time, setTime] = useListSelection(timeCount, idle && pane === "time");
  const [caveatIndex] = useListSelection(caveatCount, caveats && !drilling);
  return {
    caveats: caveatIndex,
    indexes: { fails, findings, time } as Record<Pane, number>,
    reset: () => {
      setFindings(0);
      setFails(0);
      setTime(0);
    },
  };
};

/** The row the selection points at, addressed the way `stats.drill` wants it. */
const targetFor = ({
  findings,
  indexes,
  pane,
  report,
}: {
  readonly findings: readonly Finding[];
  readonly indexes: Record<Pane, number>;
  readonly pane: Pane;
  readonly report: StatsReport | undefined;
}): DrillTarget | null => {
  if (report === undefined) {
    return null;
  }
  if (pane === "findings") {
    const finding = findings[indexes.findings];
    return finding === undefined ? null : findingTarget(finding, report);
  }
  if (pane === "fails") {
    const row = report.fails[indexes.fails];
    return row === undefined ? null : failTarget(row);
  }
  const row = report.time[indexes.time];
  return row === undefined ? null : timeTarget(row, report.corpus.bashSec);
};

/** What the screen can be asked to do from the keyboard. */
interface StatsActions {
  readonly closeAll: () => void;
  readonly cycleAgent: () => void;
  readonly cycleDays: () => void;
  readonly cyclePane: () => void;
  readonly openCaveats: () => void;
  readonly openDrill: () => void;
  readonly reExtract: () => void;
  readonly reRead: () => void;
  readonly toggleFindings: () => void;
}

/** The key event `useKeyboard` hands a screen. */
type KeyEvent = Parameters<Parameters<typeof useKeyboard>[0]>[0];

interface KeyMode {
  readonly caveats: boolean;
  readonly drilling: boolean;
  readonly typing: boolean;
}

const bareKey = (name: string, actions: StatsActions) => {
  switch (name) {
    case "tab":
      actions.cyclePane();
      break;
    case "return":
      actions.openDrill();
      break;
    case "d":
      actions.cycleDays();
      break;
    case "a":
      actions.cycleAgent();
      break;
    case "c":
      actions.openCaveats();
      break;
    case "f":
      actions.toggleFindings();
      break;
    default:
      break;
  }
};

/**
 * The screen keymap. An overlay owns the keyboard while it is open — the drill
 * mounts its own handlers, so the screen answers nothing but `esc` meanwhile.
 * Lowercase `r` is deliberately unbound: it reveals secrets on the sessions
 * screen, and a misfire here should do nothing.
 */
const handleKey = (key: KeyEvent, mode: KeyMode, actions: StatsActions) => {
  if (mode.typing) {
    return;
  }
  if (key.name === "escape") {
    actions.closeAll();
    return;
  }
  if (mode.drilling) {
    return;
  }
  if (mode.caveats) {
    if (key.name === "c") {
      actions.closeAll();
    }
    return;
  }
  if (key.shift && key.name === "r") {
    actions.reRead();
    return;
  }
  if (key.shift && key.name === "f") {
    actions.reExtract();
    return;
  }
  bareKey(key.name, actions);
};

/** The Stats screen. */
export const StatsScreen = () => {
  const { height, width } = useTerminalDimensions();
  const bridge = useBridge();
  const { typing } = useTyping();

  const [days, setDays] = useState<number>(DEFAULT_DAYS);
  const [agent, setAgent] = useState<AgentId | undefined>(undefined);
  const [rawPane, setPane] = useState<Pane>("findings");
  const [collapsed, setCollapsed] = useState(false);
  const [caveats, setCaveats] = useState(false);
  const [drill, setDrill] = useState<DrillTarget | null>(null);
  const [extracting, setExtracting] = useState<string | null>(null);

  // The watch counter is read but never threaded into these deps: a session
  // write marks the report stale, it does not rescan the corpus behind it.
  const q = useQuery(
    (c) =>
      c.stats.report({
        days,
        ...(agent === undefined ? {} : { agents: [agent] }),
      }),
    [days, agent]
  );
  const report = q.data;

  const watch = useWatch();
  const watchRef = useRef(watch.sessions);
  watchRef.current = watch.sessions;
  // Every landed report re-baselines the staleness marker against the watch
  // version at fetch time — including one landed by a window or agent change.
  const [scanned, setScanned] = useState(watch.sessions);
  useEffect(() => {
    if (report !== undefined) {
      setScanned(watchRef.current);
    }
  }, [report]);

  const content = contentWidth(width);
  const findings = useMemo(
    () => (report === undefined ? [] : findingsOf(report)),
    [report]
  );
  const caveatText = useMemo(
    () =>
      report === undefined ? [] : caveatLines(report, content - OVERLAY_CHROME),
    [report, content]
  );

  const status = extracting ?? (q.error === undefined ? null : q.error.message);
  const statusRows = status === null ? 0 : 1;
  const layout = layoutOf(height - statusRows);
  const shown = collapsed ? 0 : layout.findings;
  const pane = rawPane === "findings" && shown === 0 ? "fails" : rawPane;
  const cols = columnsOf(content, (report?.corpus.bashSec ?? 0) > 0);

  const sel = usePaneSelection({
    caveatCount: caveatText.length,
    caveats,
    drilling: drill !== null,
    failCount: report?.fails.length ?? 0,
    findingCount: findings.length,
    pane,
    timeCount: report?.time.length ?? 0,
  });
  const indexes = sel.indexes;

  const reRead = () => {
    setScanned(watchRef.current);
    q.refetch();
  };

  const reExtract = () => {
    setExtracting("re-extracting every transcript — a cold pass, minutes");
    bridge
      .run((c) =>
        c.stats.refresh({
          force: true,
          ...(agent === undefined ? {} : { agents: [agent] }),
        })
      )
      .then(() => {
        setExtracting(null);
        reRead();
      })
      .catch((err: unknown) => {
        setExtracting(`re-extract failed: ${messageOf(err)}`);
      });
  };

  const closeAll = () => {
    setDrill(null);
    setCaveats(false);
  };

  useKeyboard((key) =>
    handleKey(
      key,
      { caveats, drilling: drill !== null, typing },
      {
        closeAll,
        cycleAgent: () => {
          setAgent((a) => next(AGENT_CYCLE, a));
          sel.reset();
        },
        cycleDays: () => {
          setDays((d) => next(WINDOW_DAYS as readonly number[], d));
          sel.reset();
        },
        cyclePane: () => setPane((p) => next(PANES, p)),
        openCaveats: () => setCaveats(true),
        openDrill: () =>
          setDrill(targetFor({ findings, indexes, pane, report })),
        reExtract,
        reRead,
        toggleFindings: () => setCollapsed((v) => !v),
      }
    )
  );

  if (height < MIN_HEIGHT || width < MIN_WIDTH) {
    return (
      <text
        fg={C.warn}
      >{`terminal too small: stats needs ${MIN_WIDTH}×${MIN_HEIGHT}, this one is ${width}×${height}`}</text>
    );
  }

  const scope = scopeText({
    ago: report === undefined ? "…" : agoOf(report.generatedAt, Date.now()),
    agent,
    days,
    loading: q.loading,
    stale: report !== undefined && watch.sessions > scanned,
  });
  const overlayHeight = Math.max(
    MIN_OVERLAY_ROWS,
    height - HEADER_ROWS - HINT_ROWS - statusRows - OVERLAY_BORDER
  );
  const noteLines =
    report === undefined
      ? []
      : noteStripOf({
          findings,
          index: indexes[pane],
          max: layout.noteLines,
          pane,
          report,
        });

  const body = () => {
    if (report === undefined) {
      return <FirstPaint error={q.error} loading={q.loading} />;
    }
    if (drill !== null) {
      return (
        <StatsDrill
          height={overlayHeight}
          onClose={() => setDrill(null)}
          scope={{ agent, days }}
          target={drill}
          width={content}
        />
      );
    }
    if (caveats) {
      return (
        <Caveats
          height={overlayHeight}
          index={sel.caveats}
          lines={caveatText}
        />
      );
    }
    return (
      <StatsBody
        cols={cols}
        findings={findings}
        indexes={indexes}
        layout={layout}
        noteLines={noteLines}
        pane={pane}
        report={report}
        shown={shown}
      />
    );
  };

  return (
    <box style={{ flexDirection: "column", flexGrow: 1, minHeight: 0 }}>
      <StatsHeader
        content={content}
        report={report}
        scope={scope}
        wide={cols.wide}
      />
      {status === null ? null : (
        <text fg={extracting === null ? C.bad : C.warn}>
          {clip(status, content)}
        </text>
      )}
      {body()}
      <Hints caveats={caveats} drill={drill !== null} wide={cols.wide} />
    </box>
  );
};
