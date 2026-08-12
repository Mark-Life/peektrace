/** The frame around the two tables: header, first paint, caveats, key hints.
 *
 * Split out of the screen so the screen file is state and keys. Nothing here
 * derives a number — the header prints the corpus totals the report carries, and
 * the caveats overlay prints `report.caveats` verbatim, in order, nothing
 * filtered and nothing reworded.
 */
import type { StatsReport } from "@workspace/core/services/stats/schema";
import { ErrorLine, KeyHints, Loading, Panel } from "../components";
import { C, clip, fmt } from "../theme";
import { windowSlice } from "../use-list";
import { corpusLine, provenanceLine } from "./stats-format";

const TITLE = "Stats ▸ what fails, what costs time";
const MIN_TITLE_W = 12;
/** Border + padding + footer line of an overlay panel. */
export const OVERLAY_CHROME = 5;

/** Header: the screen, the scope, the corpus, and what made the numbers. */
export const StatsHeader = ({
  content,
  report,
  scope,
  wide,
}: {
  readonly content: number;
  readonly report: StatsReport | undefined;
  readonly scope: string;
  readonly wide: boolean;
}) => (
  <box style={{ flexDirection: "column", flexShrink: 0 }}>
    <box style={{ flexDirection: "row" }}>
      <text attributes={1} fg={C.primary}>
        {clip(TITLE, Math.max(MIN_TITLE_W, content - scope.length - 2))}
      </text>
      <box style={{ flexGrow: 1 }} />
      <text fg={C.textDim}>{scope}</text>
    </box>
    <text fg={C.textDim}>
      {report === undefined ? " " : clip(corpusLine(report, wide), content)}
    </text>
    <text fg={C.textFaint}>
      {report === undefined ? " " : clip(provenanceLine(report, wide), content)}
    </text>
    <text> </text>
  </box>
);

/** `report.caveats` verbatim, in order, word-wrapped and scrollable. */
export const Caveats = ({
  height,
  index,
  lines,
}: {
  readonly height: number;
  readonly index: number;
  readonly lines: readonly string[];
}) => {
  const visible = Math.max(1, height - OVERLAY_CHROME);
  const win = windowSlice(index, lines.length, visible);
  return (
    <Panel
      flexGrow={1}
      focused
      title="CAVEATS  everything that travels with these numbers · esc close"
    >
      {lines.slice(win.start, win.end).map((line, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: wrapped text, the row index is the identity
        <text fg={C.textDim} key={`caveat:${win.start + i}`}>
          {line}
        </text>
      ))}
      <text fg={C.textFaint}>
        {win.end < lines.length
          ? `… ${fmt(lines.length - win.end)} more · ↑↓ scroll`
          : " "}
      </text>
    </Panel>
  );
};

/** First paint: say which pass this is, because a cold one parses everything. */
export const FirstPaint = ({
  error,
  loading,
}: {
  readonly error: Error | undefined;
  readonly loading: boolean;
}) => (
  <box style={{ flexDirection: "column", flexGrow: 1, minHeight: 0 }}>
    {loading ? (
      <Loading label="Reading the corpus — a first pass parses every transcript (minutes), later passes read the fact cache (~2s)." />
    ) : null}
    {error === undefined ? null : <ErrorLine error={error} />}
  </box>
);

const WIDE_HINTS: readonly (readonly [string, string])[] = [
  ["↑↓/jk", "move"],
  ["tab", "pane"],
  ["⏎", "drill"],
  ["d", "days"],
  ["a", "agent"],
  ["R", "re-read"],
  ["F", "re-extract"],
  ["c", "caveats"],
  ["f", "findings"],
];

/** `F` re-extract is off this line: nine hints do not fit 78 cells. It is still
 * live, and the CLI README lists it. */
const NARROW_HINTS: readonly (readonly [string, string])[] = [
  ["↑↓", "move"],
  ["tab", "pane"],
  ["⏎", "drill"],
  ["d", "days"],
  ["a", "agent"],
  ["f", "findings"],
  ["R", "re-read"],
  ["c", "caveats"],
];

/** Key hints for the current mode. */
export const Hints = ({
  caveats,
  drill,
  wide,
}: {
  readonly caveats: boolean;
  readonly drill: boolean;
  readonly wide: boolean;
}) => {
  if (drill) {
    return <KeyHints hints={[["esc", "close drill"]]} />;
  }
  if (caveats) {
    return (
      <KeyHints
        hints={[
          ["↑↓", "scroll"],
          ["esc/c", "close"],
        ]}
      />
    );
  }
  return <KeyHints hints={wide ? WIDE_HINTS : NARROW_HINTS} />;
};
