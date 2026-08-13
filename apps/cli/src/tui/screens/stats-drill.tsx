/** The calls behind one ranked stats row, as a modal overlay.
 *
 * A ranked row is a claim; these are the calls it counted. The wire pages: it
 * takes an `offset` and a `pageSize` and returns the `offset` and the true
 * `total` back, so the footer states the reader's real position rather than a
 * guess. Scrolling never fetches — a page is a whole corpus pass — so `n` and
 * `p` are keys. Command lines are clipped to one row each; `⏎` opens the
 * selected one in full.
 *
 * Opening the transcript a hit came from is not wired here: the shell owns the
 * active section in its own state and exposes no setter, so no screen can route
 * to another screen. Until one can, the selected hit prints the command that
 * gets there.
 */
import type { ScrollBoxRenderable } from "@opentui/core";
import { useKeyboard } from "@opentui/react";
import type { DrillHit } from "@workspace/core/services/stats/schema";
import { useEffect, useRef, useState } from "react";
import { hms, shortId } from "../../render";
import { Empty, ErrorLine, KeyHints, Loading, Panel } from "../components";
import { useQuery } from "../runtime";
import { C, clip, firstLine, fmt, sanitize } from "../theme";
import { useListSelection } from "../use-list";
import type { DrillTarget, StatsScope } from "./stats-format";

/** What the drill overlay is handed: the row, the scope, and its own box. */
interface StatsDrillProps {
  readonly height: number;
  readonly onClose: () => void;
  readonly scope: StatsScope;
  readonly target: DrillTarget;
  readonly width: number;
}

/** Hits fetched per page. One page is one corpus pass. */
const PAGE_SIZE = 100;

/** Cells the overlay's own border and padding claim of the content width. */
const PANE_CHROME = 4;
/** Floor on the overlay's inner width, so a tiny terminal still renders. */
const MIN_INNER = 40;
/** Content width at and above which a hit shows its project (the screen's own). */
const PROJECT_MIN_W = 96;

/** Meta-line column widths (each cell pads one cell wider, for the gap). */
const TS_W = 12;
const SID_W = 14;
const PROJ_W = 16;
const TOOL_W = 10;
const DUR_W = 6;
/** Indent under the meta line, and the cells it costs the evidence. */
const INDENT = "   ";
const EVIDENCE_INDENT = INDENT.length;
/** Rows an expanded command may claim, and where a line may break. */
const WRAP_MAX = 6;
const WRAP_BREAK_MIN = 0.6;
/** Rows the selection jumps on PageUp / PageDown. */
const PAGE_JUMP = 10;
/** Chars of a session id before the first-segment rule gives up. */
const SID_HEAD_MIN = 8;

const ISO_STAMP = /^\d{4}-(\d{2})-(\d{2})T(\d{2}:\d{2})/;

/** `MM-DD HH:MM`, read literally off the ISO string, never Date-parsed. */
const stamp = (ts: string | null): string => {
  if (ts === null) {
    return "no timestamp";
  }
  const m = ISO_STAMP.exec(ts);
  return m === null ? clip(sanitize(ts), TS_W) : `${m[1]}-${m[2]} ${m[3]}`;
};

/** A session id narrow enough for a column: first uuid segment, else the head.
 * Both come off a transcript, so both are sanitized and hard-clipped. */
const shortSid = (sid: string): string => {
  const head = sanitize(shortId(sid));
  return clip(head.length >= SID_HEAD_MIN ? head : sanitize(sid), SID_W);
};

/**
 * Hard-wrap one command to `width`, breaking on a late space when there is one
 * so a path is not split mid-word. The last line is clipped, so an enormous
 * command costs a known number of rows instead of the whole pane.
 */
const wrapped = (text: string, width: number): readonly string[] => {
  const out: string[] = [];
  let rest = text;
  while (rest.length > width && out.length < WRAP_MAX - 1) {
    const cut = rest.slice(0, width).lastIndexOf(" ");
    const at = cut > width * WRAP_BREAK_MIN ? cut : width;
    out.push(rest.slice(0, at));
    rest = rest.slice(at).trimStart();
  }
  out.push(clip(rest, width));
  return out;
};

/** The command a hit ran: one clipped line, or the whole thing when expanded. */
const evidence = (line: string, width: number, expand: boolean): string => {
  if (!expand) {
    return `${INDENT}${firstLine(line, width)}`;
  }
  const rows = sanitize(line)
    .split("\n")
    .flatMap((one) => wrapped(one.trim(), width))
    .slice(0, WRAP_MAX);
  return rows.map((row) => `${INDENT}${row}`).join("\n");
};

/** Project paths differ at their tail, so clip from the front. */
const projectTail = (project: string): string =>
  project.length > PROJ_W
    ? `…${project.slice(project.length - (PROJ_W - 1))}`
    : project;

/** One call: when it ran, where, which tool, how long, and the line itself. */
const HitRow = ({
  expanded,
  hit,
  lineMax,
  pos,
  selected,
  showProject,
}: {
  readonly expanded: boolean;
  readonly hit: DrillHit;
  readonly lineMax: number;
  readonly pos: number;
  readonly selected: boolean;
  readonly showProject: boolean;
}) => (
  <box
    id={`drill:${pos}`}
    style={{
      flexDirection: "column",
      ...(selected ? { backgroundColor: C.panelSel } : {}),
    }}
  >
    <box style={{ flexDirection: "row" }}>
      <text fg={C.primary}>{selected ? "▸ " : "  "}</text>
      <text fg={C.textFaint}>{stamp(hit.ts).padEnd(TS_W + 1)}</text>
      <text fg={C.textDim}>{shortSid(hit.sid).padEnd(SID_W + 1)}</text>
      {showProject ? (
        <text fg={C.textFaint}>
          {projectTail(sanitize(hit.project)).padEnd(PROJ_W + 1)}
        </text>
      ) : null}
      <text fg={C.text}>
        {clip(sanitize(hit.tool), TOOL_W).padEnd(TOOL_W + 1)}
      </text>
      <text fg={C.textDim}>
        {(hit.durSec === null ? "—" : hms(hit.durSec)).padStart(DUR_W)}
      </text>
      <text fg={hit.failed ? C.bad : C.textFaint}>
        {hit.failed ? "  fail" : "  —"}
      </text>
    </box>
    <text fg={selected ? C.text : C.textFaint}>
      {evidence(hit.line, lineMax, expanded)}
    </text>
  </box>
);

/** Where the reader is in the match set, straight off the wire. */
const position = (offset: number, shown: number, total: number): string => {
  if (total === 0) {
    return "no calls";
  }
  return `hits ${fmt(offset + 1)}–${fmt(offset + shown)} of ${fmt(total)} calls`;
};

/**
 * The drill overlay, sized by the screen that opened it. Mounted only while
 * open, so its keys live exactly as long as it is on screen and the owning
 * screen ignores its own keymap meanwhile.
 */
export const StatsDrill = ({
  height,
  onClose,
  scope,
  target,
  width,
}: StatsDrillProps) => {
  // The page is held against the row it belongs to, so a new row reads as
  // offset 0 in the same render — resetting it in an effect would spend a
  // corpus pass on the old offset first.
  const targetId = `${target.table}:${target.key}`;
  const [paged, setPaged] = useState({ id: targetId, offset: 0 });
  const offset = paged.id === targetId ? paged.offset : 0;
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const boxRef = useRef<ScrollBoxRenderable>(null);

  const { agent, days } = scope;
  const q = useQuery(
    (c) =>
      c.stats.drill({
        ...(agent === undefined ? {} : { agents: [agent] }),
        days,
        key: target.key,
        offset,
        pageSize: PAGE_SIZE,
        table: target.table,
      }),
    [target.table, target.key, days, agent, offset]
  );

  const hits = q.data?.hits ?? [];
  const total = q.data?.total ?? 0;
  const at = q.data?.offset ?? offset;
  const [index, setIndex] = useListSelection(hits.length, true);

  const selected = hits[index];

  useEffect(() => {
    boxRef.current?.scrollChildIntoView(`drill:${index}`);
  }, [index]);

  const page = (next: number) => {
    setPaged({ id: targetId, offset: next });
    setIndex(0);
  };

  useKeyboard((key) => {
    if (key.name === "escape" || key.name === "left") {
      onClose();
      return;
    }
    if (key.name === "return" && selected) {
      const id = `${selected.sid}:${selected.seq}`;
      setExpanded((prev) => {
        const nextSet = new Set(prev);
        if (nextSet.has(id)) {
          nextSet.delete(id);
        } else {
          nextSet.add(id);
        }
        return nextSet;
      });
      return;
    }
    if (key.name === "n" && at + hits.length < total) {
      page(at + hits.length);
      return;
    }
    if (key.name === "p" && at > 0) {
      page(Math.max(0, at - PAGE_SIZE));
      return;
    }
    if (key.name === "pageup") {
      setIndex(Math.max(0, index - PAGE_JUMP));
      return;
    }
    if (key.name === "pagedown") {
      setIndex(Math.min(hits.length - 1, index + PAGE_JUMP));
    }
  });

  const inner = Math.max(MIN_INNER, width - PANE_CHROME);
  const hasMore = at + hits.length < total;
  const routeOut = selected
    ? `open it: peektrace sessions show ${sanitize(selected.sid)}`
    : "";

  return (
    <Panel
      focused
      // A fixed box, not flexGrow: a growing panel claims the screen's hint row
      // and paints over the shell footer.
      height={height}
      title={clip(`DRILL  --${target.table} ${sanitize(target.key)}`, inner)}
    >
      <box style={{ flexDirection: "column", flexShrink: 0 }}>
        <text fg={C.text}>{clip(sanitize(target.label), inner)}</text>
        {target.note === null ? null : (
          <text fg={C.textDim}>{clip(sanitize(target.note), inner)}</text>
        )}
        <text fg={C.textFaint}>{clip(sanitize(target.spread), inner)}</text>
      </box>
      {q.loading && hits.length === 0 ? (
        <Loading label="Reading the calls behind this row…" />
      ) : null}
      {q.error ? <ErrorLine error={q.error} /> : null}
      {!(q.loading || q.error) && hits.length === 0 ? (
        <Empty label="No calls behind this row." />
      ) : null}
      {hits.length > 0 ? (
        <scrollbox
          focused
          ref={boxRef}
          style={{ flexGrow: 1, minHeight: 0, paddingTop: 1 }}
        >
          <box style={{ flexDirection: "column" }}>
            {hits.map((hit, i) => (
              <HitRow
                expanded={expanded.has(`${hit.sid}:${hit.seq}`)}
                hit={hit}
                key={`${hit.sid}:${hit.seq}`}
                lineMax={inner - EVIDENCE_INDENT - 1}
                pos={i}
                selected={i === index}
                showProject={width >= PROJECT_MIN_W}
              />
            ))}
          </box>
        </scrollbox>
      ) : null}
      <box style={{ flexDirection: "column", flexShrink: 0 }}>
        <text fg={C.textDim}>{position(at, hits.length, total)}</text>
        <text fg={C.textFaint}>{clip(routeOut, inner)}</text>
        <KeyHints
          hints={[
            ["↑↓/jk", "move"],
            ["⏎", "full line"],
            ...(hasMore ? ([["n", "next page"]] as const) : []),
            ...(at > 0 ? ([["p", "prev page"]] as const) : []),
            ["esc", "close"],
          ]}
        />
      </box>
    </Panel>
  );
};
