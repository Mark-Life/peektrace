/** The two ranked tables, the findings band and the note strip.
 *
 * Rows arrive pre-ranked from core and are never re-sorted here: the ranking key
 * is printed in the pane title and marked with `▼` on the column it sorts by, so
 * what the screen shows and what `peektrace stats` prints are the same order.
 * The two rankings are never combined — sessions spanned and seconds charged are
 * different units, and no row carries a blended score.
 */
import type {
  FailRow,
  StatsReport,
  TimeRow,
} from "@workspace/core/services/stats/schema";
import { hms } from "../../render";
import { Empty } from "../components";
import { C, clip, fmt, sanitize } from "../theme";
import { type Window, windowSlice } from "../use-list";
import {
  cell,
  type FailCols,
  type Finding,
  num,
  type Pane,
  pairCell,
  plural,
  type StatsCols,
  type StatsLayout,
  shareCell,
  type TimeCols,
} from "./stats-format";

/** Impact column of the findings band. */
const IMPACT_W = 12;
const FIND_TITLE_WIDE = 29;
const FIND_TITLE_NARROW = 24;
const MARKER_W = 2;
/** Pane bar + the two spaces before the title's note. */
const TITLE_CHROME = 3;
/** Width of the `NOTE  ` label the strip indents under. */
const NOTE_LABEL_W = 6;

/** Pane heading: a bright bar when the pane owns the selection. */
export const PaneTitle = ({
  focused,
  note,
  title,
  width,
}: {
  readonly focused: boolean;
  readonly note: string;
  readonly title: string;
  readonly width: number;
}) => (
  <box style={{ flexDirection: "row", flexShrink: 0 }}>
    <text fg={focused ? C.primary : C.border}>▌</text>
    <text fg={focused ? C.text : C.textDim}>{title}</text>
    <text fg={C.textFaint}>
      {`  ${clip(note, Math.max(0, width - title.length - TITLE_CHROME))}`}
    </text>
  </box>
);

/** The line under a windowed table: what is off screen, never silently cut. */
export const MoreRows = ({
  count,
  win,
}: {
  readonly count: number;
  readonly win: Window;
}) => {
  const above = win.start;
  const below = count - win.end;
  const parts = [
    ...(above > 0 ? [`↑ ${fmt(above)} above`] : []),
    ...(below > 0 ? [`↓ ${plural(below, "more row")}`] : []),
  ];
  return (
    <text fg={C.textFaint}>
      {parts.length === 0 ? " " : `  ${parts.join(" · ")}`}
    </text>
  );
};

// --- Findings ---

/** One finding: what it costs on line 1, what was measured on line 2. */
const FindingRow = ({
  content,
  finding,
  selected,
  wide,
}: {
  readonly content: number;
  readonly finding: Finding;
  readonly selected: boolean;
  readonly wide: boolean;
}) => {
  const titleW = wide ? FIND_TITLE_WIDE : FIND_TITLE_NARROW;
  const gap = wide ? "  " : " ";
  const impact = finding.impact.padStart(IMPACT_W);
  const spreadW = Math.max(
    0,
    content - MARKER_W - titleW - impact.length - gap.length
  );
  return (
    <box
      style={{
        flexDirection: "column",
        flexShrink: 0,
        ...(selected ? { backgroundColor: C.panelSel } : {}),
      }}
    >
      <box style={{ flexDirection: "row" }}>
        <text fg={finding.detector === null ? C.textFaint : C.warn}>
          {finding.detector === null ? "· " : "▲ "}
        </text>
        <text fg={selected ? C.primary : C.text}>
          {cell(finding.title, titleW)}
        </text>
        <text fg={C.text}>{impact}</text>
        <text fg={C.textDim}>{gap + cell(finding.spread, spreadW)}</text>
      </box>
      <text fg={C.textFaint}>
        {`  ${clip(sanitize(finding.note), Math.max(0, content - MARKER_W))}`}
      </text>
    </box>
  );
};

/**
 * The lead band: rows that name a change, cheapest change first. Collapsed it is
 * one line, because at a short terminal the tables are the body.
 */
export const FindingsBand = ({
  content,
  findings,
  focused,
  index,
  shown,
  wide,
}: {
  readonly content: number;
  readonly findings: readonly Finding[];
  readonly focused: boolean;
  readonly index: number;
  readonly shown: number;
  readonly wide: boolean;
}) => {
  if (shown === 0) {
    return (
      <PaneTitle
        focused={focused}
        note={`${plural(findings.length, "row")} name a change · f to open`}
        title="FIX FIRST"
        width={content}
      />
    );
  }
  const win = windowSlice(index, findings.length, shown);
  const rest = findings.length - win.end;
  return (
    <box style={{ flexDirection: "column", flexShrink: 0 }}>
      <PaneTitle
        focused={focused}
        note="rows that name a change  ·  ▲ = a named rule fired"
        title="FIX FIRST"
        width={content}
      />
      {findings.slice(win.start, win.end).map((f, i) => (
        <FindingRow
          content={content}
          finding={f}
          key={`${f.table}:${f.key}`}
          selected={win.start + i === index}
          wide={wide}
        />
      ))}
      <text fg={C.textFaint}>
        {rest > 0 ? `  ${plural(rest, "more finding")} · f` : " "}
      </text>
    </box>
  );
};

// --- What fails ---

const FailHeader = ({ cols }: { readonly cols: FailCols }) => (
  <box style={{ flexDirection: "row", flexShrink: 0 }}>
    <text fg={C.textDim}>{`  ${num("SESSIONS▼", cols.sessions)}`}</text>
    <text fg={C.textFaint}>
      {cols.gap +
        num("CALLS", cols.calls) +
        cols.gap +
        num("FLAGGED", cols.flagged) +
        cols.gap +
        num("PROJ", cols.proj) +
        cols.gap +
        (cols.where > 0 ? "WHERE".padEnd(cols.where) + cols.gap : "") +
        "WHAT"}
    </text>
  </box>
);

const FailTableRow = ({
  cols,
  row,
  selected,
}: {
  readonly cols: FailCols;
  readonly row: FailRow;
  readonly selected: boolean;
}) => (
  <box
    style={{
      flexDirection: "row",
      flexShrink: 0,
      ...(selected ? { backgroundColor: C.panelSel } : {}),
    }}
  >
    <text fg={C.primary}>{selected ? "▸" : " "}</text>
    <text fg={C.warn}>{row.detector === null ? " " : "▲"}</text>
    <text fg={C.text}>{num(fmt(row.sessions), cols.sessions)}</text>
    <text fg={C.textDim}>{cols.gap + num(fmt(row.rows), cols.calls)}</text>
    <text fg={C.textDim}>{cols.gap + num(fmt(row.flagged), cols.flagged)}</text>
    <text fg={C.textFaint}>{cols.gap + num(fmt(row.projects), cols.proj)}</text>
    {cols.where > 0 ? (
      <text fg={C.textFaint}>{cols.gap + cell(row.where, cols.where)}</text>
    ) : null}
    <text fg={selected ? C.primary : C.text}>
      {cols.gap + cell(row.label, cols.what)}
    </text>
  </box>
);

/** Ranked by sessions spanned — core's order, redrawn. */
export const FailTable = ({
  cols,
  focused,
  index,
  rows,
  visible,
}: {
  readonly cols: StatsCols;
  readonly focused: boolean;
  readonly index: number;
  readonly rows: readonly FailRow[];
  readonly visible: number;
}) => {
  const win = windowSlice(index, rows.length, visible);
  return (
    <box style={{ flexDirection: "column", flexShrink: 0 }}>
      <PaneTitle
        focused={focused}
        note={`ranked by sessions spanned  ·  ${plural(rows.length, "row")} kept`}
        title="WHAT FAILS"
        width={cols.content}
      />
      <FailHeader cols={cols.fail} />
      {rows.length === 0 ? (
        <Empty label="  No failing calls in this window." />
      ) : null}
      {rows.slice(win.start, win.end).map((row, i) => (
        <FailTableRow
          cols={cols.fail}
          key={row.key}
          row={row}
          selected={win.start + i === index}
        />
      ))}
      <MoreRows count={rows.length} win={win} />
    </box>
  );
};

// --- What costs time ---

const TimeHeader = ({ cols }: { readonly cols: TimeCols }) => (
  <box style={{ flexDirection: "row", flexShrink: 0 }}>
    <text fg={C.textFaint}>{`  ${"JOB".padEnd(cols.job)}`}</text>
    <text fg={C.textDim}>{cols.gap + num("TIME▼", cols.time)}</text>
    <text fg={C.textFaint}>
      {cols.gap +
        num("SHARE", cols.share) +
        cols.gap +
        num("RUNS", cols.runs) +
        cols.gap +
        num("CO-PRESENT", cols.coPresent) +
        (cols.median > 0 ? cols.gap + num("MEDIAN", cols.median) : "") +
        (cols.p95 > 0 ? cols.gap + num("P95", cols.p95) : "") +
        cols.gap +
        num("CAPPED", cols.capped) +
        cols.gap +
        num("BG", cols.bg)}
    </text>
  </box>
);

const TimeTableRow = ({
  bashSec,
  cols,
  row,
  selected,
}: {
  readonly bashSec: number;
  readonly cols: TimeCols;
  readonly row: TimeRow;
  readonly selected: boolean;
}) => {
  const capped = pairCell(row.cappedRuns, row.cappedSec, cols.capped);
  const bg = pairCell(row.bgRuns, row.bgSec, cols.bg);
  return (
    <box
      style={{
        flexDirection: "row",
        flexShrink: 0,
        ...(selected ? { backgroundColor: C.panelSel } : {}),
      }}
    >
      <text fg={C.primary}>{selected ? "▸ " : "  "}</text>
      <text fg={selected ? C.primary : C.text}>
        {cell(row.label, cols.job)}
      </text>
      <text fg={C.text}>{cols.gap + num(hms(row.totalSec), cols.time)}</text>
      <text fg={C.textDim}>
        {cols.gap + num(shareCell(row.share, bashSec), cols.share)}
      </text>
      <text fg={C.textFaint}>
        {cols.gap +
          num(fmt(row.runs), cols.runs) +
          cols.gap +
          num(fmt(row.coPresentRuns), cols.coPresent)}
      </text>
      {cols.median > 0 ? (
        <text fg={C.textDim}>
          {cols.gap + num(hms(row.medianSec), cols.median)}
        </text>
      ) : null}
      {cols.p95 > 0 ? (
        <text fg={C.textDim}>{cols.gap + num(hms(row.p95Sec), cols.p95)}</text>
      ) : null}
      <text fg={capped === "—" ? C.textFaint : C.warn}>
        {cols.gap + num(capped, cols.capped)}
      </text>
      <text fg={bg === "—" ? C.textFaint : C.warn}>
        {cols.gap + num(bg, cols.bg)}
      </text>
    </box>
  );
};

/** Ranked by seconds charged — every line charged to exactly one job. */
export const TimeTable = ({
  cols,
  focused,
  index,
  report,
  visible,
}: {
  readonly cols: StatsCols;
  readonly focused: boolean;
  readonly index: number;
  readonly report: StatsReport;
  readonly visible: number;
}) => {
  const rows = report.time;
  const win = windowSlice(index, rows.length, visible);
  return (
    <box style={{ flexDirection: "column", flexShrink: 0 }}>
      <PaneTitle
        focused={focused}
        note={
          cols.wide
            ? "ranked by seconds charged  ·  every line charged to exactly one job"
            : `by seconds charged · one line, one job · ${plural(rows.length, "row")}`
        }
        title="WHAT COSTS TIME"
        width={cols.content}
      />
      <TimeHeader cols={cols.time} />
      {rows.length === 0 ? (
        <Empty label="  No shell seconds in this window." />
      ) : null}
      {rows.slice(win.start, win.end).map((row, i) => (
        <TimeTableRow
          bashSec={report.corpus.bashSec}
          cols={cols.time}
          key={row.key}
          row={row}
          selected={win.start + i === index}
        />
      ))}
      <MoreRows count={rows.length} win={win} />
    </box>
  );
};

// --- The note strip ---

/** What the selected row measured, indented under a `NOTE` label. */
export const NoteStrip = ({
  content,
  lines,
}: {
  readonly content: number;
  readonly lines: readonly string[];
}) => (
  <box style={{ flexDirection: "column", flexShrink: 0 }}>
    {lines.map((line, i) => (
      <box
        // biome-ignore lint/suspicious/noArrayIndexKey: fixed-height strip, index is the row
        key={`note:${i}`}
        style={{ flexDirection: "row" }}
      >
        <text fg={i === 0 ? C.accent : C.textFaint}>
          {i === 0 ? "NOTE  " : "      "}
        </text>
        <text fg={i === 0 ? C.text : C.textFaint}>
          {clip(sanitize(line), Math.max(0, content - NOTE_LABEL_W))}
        </text>
      </box>
    ))}
  </box>
);

/** The body proper: findings band, the two tables, the note strip. */
export const StatsBody = ({
  cols,
  findings,
  indexes,
  layout,
  noteLines,
  pane,
  report,
  shown,
}: {
  readonly cols: StatsCols;
  readonly findings: readonly Finding[];
  readonly indexes: Record<Pane, number>;
  readonly layout: StatsLayout;
  readonly noteLines: readonly string[];
  readonly pane: Pane;
  readonly report: StatsReport;
  readonly shown: number;
}) => (
  <box style={{ flexDirection: "column", flexGrow: 1, minHeight: 0 }}>
    <FindingsBand
      content={cols.content}
      findings={findings}
      focused={pane === "findings"}
      index={indexes.findings}
      shown={shown}
      wide={cols.wide}
    />
    <text> </text>
    <FailTable
      cols={cols}
      focused={pane === "fails"}
      index={indexes.fails}
      rows={report.fails}
      visible={layout.failRows}
    />
    <text> </text>
    <TimeTable
      cols={cols}
      focused={pane === "time"}
      index={indexes.time}
      report={report}
      visible={layout.timeRows}
    />
    <box style={{ flexGrow: 1, minHeight: 0 }} />
    {noteLines.length === 0 ? null : (
      <box style={{ flexDirection: "column", flexShrink: 0 }}>
        <text> </text>
        <NoteStrip content={cols.content} lines={noteLines} />
      </box>
    )}
  </box>
);
