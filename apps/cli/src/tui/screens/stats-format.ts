/** Pure layout, ranking and text helpers for the terminal stats screen.
 *
 * Everything here is a function of the report plus the terminal size: the screen
 * derives no statistic of its own, it only picks columns and formats cells.
 * Durations and shares come from the CLI's own `render` helpers so `peektrace
 * stats` and this screen print byte-identical numbers.
 *
 * Which rows lead a cold start, and what each one says, is core's answer
 * (`services/stats/findings`); this file binds it to the CLI's own number
 * formatting so the screen, `peektrace stats` and the web inspector cannot drift
 * from one another by editing a sentence.
 */
import type { AgentId } from "@workspace/core";
import {
  failSpread as coreFailSpread,
  findingsOf as coreFindingsOf,
  timeNote as coreTimeNote,
  timeSpread as coreTimeSpread,
  type Finding,
  type FindingFormat,
} from "@workspace/core/services/stats/findings";
import type {
  DrillTable,
  FailRow,
  SampleRef,
  StatsReport,
  TimeRow,
} from "@workspace/core/services/stats/schema";
import { hms, percent, shortId } from "../../render";
import { AGENT_LABEL, clip, fmt, sanitize } from "../theme";

/** What the screen asks the server for. Also what a drill page must repeat. */
export interface StatsScope {
  readonly agent: AgentId | undefined;
  readonly days: number;
}

/** Which pane owns the selection. */
export type Pane = "fails" | "findings" | "time";

/** One aggregate row, addressed the way `stats.drill` addresses it. */
export interface DrillTarget {
  readonly key: string;
  readonly label: string;
  readonly note: string | null;
  /** The row's own second number, printed under the drill title. */
  readonly spread: string;
  readonly table: DrillTable;
}

const DAYS_WEEK = 7;
const DAYS_MONTH = 30;
const DAYS_TWO_MONTHS = 60;
const DAYS_QUARTER = 90;

/** History windows `d` cycles through. */
export const WINDOW_DAYS = [
  DAYS_WEEK,
  DAYS_MONTH,
  DAYS_TWO_MONTHS,
  DAYS_QUARTER,
] as const;

// --- Vertical layout ---

/** Rows the app shell takes for its nav and footer. */
const SHELL_ROWS = 2;
/** Every non-row line of the screen at full size (see `layoutOf`). */
const FIXED_ROWS = 21;
/** Below this the screen cannot show a table and says so instead. */
export const MIN_HEIGHT = 24;
/**
 * Narrowest terminal the narrow layout still fits: the time table's fixed cells
 * plus a readable job label, and the key-hint line. Below it the rows wrap and
 * every row under a wrapped one moves, so the screen refuses instead.
 */
export const MIN_WIDTH = 80;

const FINDINGS_MAX = 3;
const FINDINGS_2 = 12;
const FINDINGS_1 = 10;
const FINDINGS_0 = 8;
/** Rows below which the note strip starts shedding lines. */
const NOTE_FLOOR = 6;
const NOTE_LINES_FULL = 4;
const NOTE_LINES_SHORT = 2;
const TIME_SHARE = 0.45;
const TIME_MIN = 3;
const TIME_MAX = 12;
const FAIL_MIN = 3;
const FAIL_MAX = 20;

const clamp = (lo: number, hi: number, value: number): number =>
  Math.max(lo, Math.min(hi, value));

/** How many rows each band gets at this terminal height. */
export interface StatsLayout {
  readonly failRows: number;
  /** Findings shown two lines each; 0 collapses the band to its title. */
  readonly findings: number;
  readonly noteLines: number;
  readonly timeRows: number;
}

/**
 * Split the terminal height between the two tables. The findings band is the
 * lead, so it keeps its rows until three would not leave both tables a body;
 * the note strip is the last thing to shed lines.
 */
const findingsFor = (avail: number): number => {
  if (avail >= FINDINGS_2) {
    return FINDINGS_MAX;
  }
  if (avail >= FINDINGS_1) {
    return 2;
  }
  return avail >= FINDINGS_0 ? 1 : 0;
};

export const layoutOf = (height: number): StatsLayout => {
  const avail = height - SHELL_ROWS - FIXED_ROWS;
  const findings = findingsFor(avail);
  let rows = findings === 0 ? avail + 1 : avail - 2 * findings;
  let noteLines = NOTE_LINES_FULL;
  if (rows < NOTE_FLOOR) {
    noteLines = NOTE_LINES_SHORT;
    rows += NOTE_LINES_SHORT;
  }
  if (rows < NOTE_FLOOR) {
    noteLines = 0;
    rows += NOTE_LINES_SHORT + 1;
  }
  const timeRows = clamp(TIME_MIN, TIME_MAX, Math.round(rows * TIME_SHARE));
  return {
    failRows: clamp(FAIL_MIN, FAIL_MAX, rows - timeRows),
    findings,
    noteLines,
    timeRows,
  };
};

// --- Horizontal layout ---

/** Content cells the shell leaves the screen (one cell of padding each side). */
const SHELL_PAD = 2;
/** Below this the wide columns are dropped rather than clipped in place. */
const WIDE_MIN = 96;
/** `not measured` needs its own room; the label column pays for it. */
const SHARE_W = 6;
const SHARE_W_UNMEASURED = 12;
const LABEL_MIN = 8;
/** Wide-only columns: the tool a row fired in, and the two durations. */
const WHERE_W = 16;
const MEDIAN_W = 7;
const P95_W = 6;

/** Every cell of the fail table but the gutter. `where: 0` means dropped. */
export interface FailCols {
  readonly calls: number;
  readonly flagged: number;
  readonly gap: string;
  readonly proj: number;
  readonly sessions: number;
  readonly what: number;
  readonly where: number;
}

/** Every cell of the time table but the gutter. `median`/`p95` 0 means dropped. */
export interface TimeCols {
  readonly bg: number;
  readonly capped: number;
  readonly coPresent: number;
  readonly gap: string;
  readonly job: number;
  readonly median: number;
  readonly p95: number;
  readonly runs: number;
  readonly share: number;
  readonly time: number;
}

export interface StatsCols {
  readonly content: number;
  readonly fail: FailCols;
  readonly time: TimeCols;
  readonly wide: boolean;
}

const FAIL_FIXED_WIDE = 54;
const FAIL_FIXED_NARROW = 32;
const TIME_FIXED_WIDE = 82;
const TIME_FIXED_NARROW = 59;

/** Content width of the screen inside the shell's padding. */
export const contentWidth = (width: number): number =>
  Math.max(0, width - SHELL_PAD);

/**
 * Column widths for one content width. Below `WIDE_MIN` the fail `WHERE` and the
 * time `MEDIAN`/`P95` columns go — the row label still names the tool, and the
 * two durations reappear in the note strip.
 */
export const columnsOf = (
  content: number,
  measuredShare: boolean
): StatsCols => {
  const wide = content >= WIDE_MIN;
  const gap = wide ? "  " : " ";
  const share = measuredShare ? SHARE_W : SHARE_W_UNMEASURED;
  const slack = share - SHARE_W;
  return {
    content,
    wide,
    fail: {
      gap,
      sessions: 9,
      calls: 6,
      flagged: 7,
      proj: 4,
      where: wide ? WHERE_W : 0,
      what: Math.max(
        LABEL_MIN,
        content - (wide ? FAIL_FIXED_WIDE : FAIL_FIXED_NARROW)
      ),
    },
    time: {
      gap,
      time: 7,
      share,
      runs: 7,
      coPresent: 10,
      median: wide ? MEDIAN_W : 0,
      p95: wide ? P95_W : 0,
      capped: 11,
      bg: 10,
      job: Math.max(
        LABEL_MIN,
        content - (wide ? TIME_FIXED_WIDE : TIME_FIXED_NARROW) - slack
      ),
    },
  };
};

/** `1 row` / `2 rows` — a count that reads as a sentence. */
export const plural = (count: number, word: string): string =>
  `${fmt(count)} ${word}${count === 1 ? "" : "s"}`;

/** A left-aligned cell: clipped to `width`, then padded to it. */
export const cell = (text: string, width: number): string =>
  clip(sanitize(text), width).padEnd(width);

/** A right-aligned cell, never truncated: a number that lies is worse. */
export const num = (text: string, width: number): string =>
  text.padStart(width);

// --- Cells ---

/** A share of shell seconds, or the honest refusal when there are none. */
export const shareCell = (fraction: number, bashSec: number): string =>
  bashSec > 0 ? percent(fraction) : "not measured";

/**
 * `15/1h41m`, or an em dash when the count is zero. A four-digit count plus a
 * three-digit hour reads wider than the column; rather than wrap the row (which
 * shifts every row under it) the cell drops to the count alone. The seconds are
 * never rounded away — the note strip prints them in full for the selected row.
 */
export const pairCell = (runs: number, sec: number, width: number): string => {
  if (runs === 0) {
    return "—";
  }
  const pair = `${fmt(runs)}/${hms(sec)}`;
  return pair.length <= width ? pair : fmt(runs);
};

/** Shortest prefix of a session id that still names it (`agent-` ids are longer). */
const SID_HEAD_MIN = 8;
const SID_MAX = 14;
export const shortSid = (sid: string): string => {
  const head = shortId(sid);
  return head.length >= SID_HEAD_MIN ? head : sid.slice(0, SID_MAX);
};

const REFS_PER_ROW = 3;

/** The session ids behind a row: the route from a number back to a transcript. */
export const refsOf = (samples: readonly SampleRef[]): string => {
  const ids = [...new Set(samples.map((s) => shortSid(s.sid)))].slice(
    0,
    REFS_PER_ROW
  );
  return ids.length === 0 ? "—" : ids.join(", ");
};

// --- Findings ---

/** How this surface spells a number, for the sentences core composes. */
const FORMAT: FindingFormat = { count: fmt, hms, pct: percent };

export type { Finding } from "@workspace/core/services/stats/findings";

/** The spread line of a fail row, shared by the band and the note strip. */
export const failSpread = (row: FailRow): string => coreFailSpread(row, FORMAT);

/** The spread line of a time row. */
export const timeSpread = (row: TimeRow, bashSec: number): string =>
  coreTimeSpread(row, bashSec, FORMAT);

/** What a time row can say for itself, from its own columns. */
export const timeNote = (row: TimeRow): string | null =>
  coreTimeNote(row, FORMAT);

/** The cold-start list: rows that name a change, cheapest change first. */
export const findingsOf = (report: StatsReport): readonly Finding[] =>
  coreFindingsOf(report, FORMAT);

// --- The note strip ---

const WAIT_JOB = "wait/poll";

const waitLines = (report: StatsReport): readonly string[] => {
  const w = report.waiting;
  const shapes = w.byShape
    .map((s) => `${s.key} ${fmt(s.calls)}/${hms(s.totalSec)}`)
    .join(" · ");
  return [
    shapes === "" ? `${fmt(w.calls)} waiting calls` : shapes,
    `${fmt(w.bgRuns)} backgrounded waits charged ${hms(w.bgSec)}, declaring ${hms(w.bgDeclaredSec)} of sleep (never summed in)`,
  ];
};

const seenIn = (samples: readonly SampleRef[], tail: string): string =>
  `seen in ${refsOf(samples)} · ⏎ ${tail}`;

const failNoteLines = (row: FailRow): readonly string[] => [
  row.note ?? "no rule fired for this row — ⏎ reads its calls",
  failSpread(row),
  seenIn(row.samples, `reads all ${fmt(row.rows)} calls`),
];

const timeNoteLines = (
  row: TimeRow,
  report: StatsReport
): readonly string[] => [
  timeNote(row) ??
    `${fmt(row.runs)} runs charged ${hms(row.totalSec)} — ⏎ reads them`,
  `${fmt(row.runs)} runs · ${fmt(row.coPresentRuns)} co-present · median ${hms(row.medianSec)} · p95 ${hms(row.p95Sec)}`,
  ...(row.key === WAIT_JOB
    ? waitLines(report)
    : [seenIn(row.samples, `reads all ${fmt(row.runs)} runs`)]),
];

const findingNoteLines = (finding: Finding): readonly string[] => [
  finding.note,
  finding.spread,
  seenIn(finding.samples, "reads the calls behind it"),
];

/**
 * The strip under the tables: what the selected row measured, how wide it
 * reaches, and the way back into the transcripts. Never advice — a note only
 * exists when core minted one.
 */
export const noteLinesOf = (args: {
  readonly findings: readonly Finding[];
  readonly index: number;
  readonly pane: Pane;
  readonly report: StatsReport;
}): readonly string[] => {
  const { findings, index, pane, report } = args;
  if (pane === "findings") {
    const finding = findings[index];
    return finding === undefined ? [] : findingNoteLines(finding);
  }
  if (pane === "fails") {
    const row = report.fails[index];
    return row === undefined ? [] : failNoteLines(row);
  }
  const row = report.time[index];
  return row === undefined ? [] : timeNoteLines(row, report);
};

/**
 * The strip, padded to the height the layout gave it: a fixed-height strip keeps
 * the tables from jumping as the selection moves.
 */
export const noteStripOf = (
  args: Parameters<typeof noteLinesOf>[0] & { readonly max: number }
): readonly string[] =>
  args.max === 0
    ? []
    : [...noteLinesOf(args), "", "", "", ""].slice(0, args.max);

// --- Drill targets ---

export const failTarget = (row: FailRow): DrillTarget => ({
  key: row.key,
  label: row.label,
  note: row.note,
  spread: `${fmt(row.rows)} calls · ${fmt(row.sessions)} sessions · ${fmt(row.projects)} projects · ${fmt(row.flagged)} flagged`,
  table: "fail",
});

export const timeTarget = (row: TimeRow, bashSec: number): DrillTarget => ({
  key: row.key,
  label: row.label,
  note: timeNote(row),
  spread: `${fmt(row.runs)} runs · ${hms(row.totalSec)} · ${timeSpread(row, bashSec)}`,
  table: "time",
});

/** The drill target for a finding, resolved back to the row it came from. */
export const findingTarget = (
  finding: Finding,
  report: StatsReport
): DrillTarget | null => {
  if (finding.table === "fail") {
    const row = report.fails.find((r) => r.key === finding.key);
    return row === undefined ? null : failTarget(row);
  }
  const row = report.time.find((r) => r.key === finding.key);
  return row === undefined ? null : timeTarget(row, report.corpus.bashSec);
};

// --- Header text ---

const MS_PER_MIN = 60_000;
const MIN_PER_HOUR = 60;
const HOURS_PER_DAY = 24;

/** How long ago the report was computed, from its own `generatedAt`. */
export const agoOf = (generatedAt: number, now: number): string => {
  const mins = Math.floor((now - generatedAt) / MS_PER_MIN);
  if (mins < 1) {
    return "just now";
  }
  if (mins < MIN_PER_HOUR) {
    return `${mins}m ago`;
  }
  const hours = Math.floor(mins / MIN_PER_HOUR);
  return hours < HOURS_PER_DAY
    ? `${hours}h ago`
    : `${Math.floor(hours / HOURS_PER_DAY)}d ago`;
};

/** `30d · all agents · scanned 2m ago`, plus the live-state markers. */
export const scopeText = (args: {
  readonly ago: string;
  readonly agent: AgentId | undefined;
  readonly days: number;
  readonly loading: boolean;
  readonly stale: boolean;
}): string => {
  const who =
    args.agent === undefined
      ? "all agents"
      : (AGENT_LABEL[args.agent] ?? args.agent);
  return [
    `${args.days}d`,
    who,
    `scanned ${args.ago}`,
    ...(args.loading ? ["↻"] : []),
    ...(args.stale ? ["● stale · R"] : []),
  ].join(" · ");
};

/** Corpus totals. Every share on the screen divides by a number on this line. */
export const corpusLine = (report: StatsReport, wide: boolean): string => {
  const c = report.corpus;
  return [
    `${fmt(c.transcripts)} transcripts`,
    `${fmt(c.projects)} projects`,
    ...(wide ? [`${fmt(c.toolCalls)} tool calls`] : []),
    `${fmt(c.bashCalls)} shell calls`,
    `${hms(c.bashSec)} shell time`,
  ].join(" · ");
};

/** What the numbers were made by, and how many caveats travel with them. */
export const provenanceLine = (report: StatsReport, wide: boolean): string =>
  [
    wide ? report.schemaVersion : "stats/v1",
    `detector v${report.detectorVersion}`,
    wide ? "durations ≈ the call→result gap" : "durations ≈ call→result gap",
    `${report.caveats.length + (report.skipped.length > 0 ? 1 : 0)} caveats (c)`,
  ].join(" · ");

/** Every caveat, plus the skipped files, wrapped to `width` for the overlay. */
export const caveatLines = (
  report: StatsReport,
  width: number
): readonly string[] => {
  const skipped =
    report.skipped.length === 0
      ? []
      : [
          `${fmt(report.skipped.length)} files could not be read and are not counted:`,
          ...report.skipped,
        ];
  return [...report.caveats, ...skipped].flatMap((line) =>
    wrap(`- ${sanitize(line)}`, width)
  );
};

const SPACES = /\s+/;

/** Word-wrap to `width`, hanging the continuation by two cells. */
export const wrap = (text: string, width: number): readonly string[] => {
  const cols = Math.max(LABEL_MIN, width);
  const out: string[] = [];
  let line = "";
  for (const word of text.split(SPACES).filter((w) => w !== "")) {
    const next = line === "" ? word : `${line} ${word}`;
    if (next.length > cols && line !== "") {
      out.push(line);
      line = `  ${word}`;
    } else {
      line = next;
    }
  }
  if (line !== "") {
    out.push(line);
  }
  return out.length === 0 ? [""] : out;
};
