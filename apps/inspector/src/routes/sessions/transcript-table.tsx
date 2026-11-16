/** The dense, one-row-per-event transcript — the forensics reading.
 *
 * Every event keeps its own row, in the column sort the header picks. Tool
 * calls and results render through the vendored AI Elements `Tool` primitives;
 * every other kind stays a plain disclosure row. One row per
 * event either way — pairing a call with its result into a single card would
 * reorder the transcript and merge two token figures, and this is a forensics
 * view. (The chat layout pairs them; it also never sums the two figures.)
 */
import {
  eventBadgeLabel,
  thinkingNote,
} from "@workspace/core/services/sessions/labels";
import type {
  AnalyzedSession,
  TimelineEvent,
} from "@workspace/core/services/sessions/schema";
import {
  eventTokens,
  tokenLabel,
} from "@workspace/core/services/sessions/tokens";
import {
  Tool,
  ToolContent,
  ToolHeader,
  type ToolState,
} from "@workspace/ui/components/ai-elements/tool";
import { Badge } from "@workspace/ui/components/badge";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@workspace/ui/components/collapsible";
import { fmt, PERCENT } from "@workspace/viz/lib/session-format";
import { ChevronRightIcon, CornerDownLeftIcon } from "lucide-react";
import { memo, type RefObject, useCallback, useMemo, useRef } from "react";
import type {
  EventRow as AnyEvent,
  TranscriptRowRef,
} from "../../lib/chat-lane";
import {
  alignOf,
  type HistoryJump,
  type JumpLanding,
} from "../../lib/history-jump";
import { type HydrateCache, hydrator } from "../../lib/history-pages";
import {
  type HistorySort,
  type HistorySortKey,
  isForwardOrder,
  nextSort,
} from "../../lib/history-sort";
import { eventCollapseId } from "../../lib/session-view";
import type { ToolIndex } from "../../lib/tool-event";
import {
  callView,
  EMPTY_BODY,
  isToolEvent,
  resultPane,
  toolNameOf,
  toolState,
} from "../../lib/tool-event";
import { estimateTableRow } from "../../lib/transcript-estimate";
import { sameEvent } from "../../lib/transcript-row";
import { tableTurnStops } from "../../lib/turn-nav";
import { type RevealHit, useRevealHit } from "../../lib/use-history-search";
import { useTurnNav } from "../../lib/use-turn-nav";
import { useVirtualJump } from "../../lib/virtual-jump";
import type { VirtualListHandle } from "../../lib/virtual-range";
import { ContextEditBadge, ContextEditNote } from "./context-edit-mark";
import { EventBody } from "./event-body";
import { EventImages } from "./event-images";
import { PendingRow } from "./pending-row";
import { ToolPaneView, ToolResultView } from "./tool-panes";
import { SortHeader } from "./transcript-sort-header";
import { VirtualList } from "./virtual-list";

/** Decimal places for the per-event window-share percent (token sort only). */
const PCT_DECIMALS = 2;

/** Turn-number gutter, shared by every transcript row. */
const TurnGutter = ({ turn }: { readonly turn: number }) => (
  <span className="w-8 shrink-0 font-mono text-muted-foreground text-xs">
    t{turn}
  </span>
);

/** Per-event token estimate, right-aligned; `share` is its window fraction. */
const TokenCount = ({
  e,
  share,
}: {
  readonly e: TimelineEvent;
  readonly share?: number | undefined;
}) => {
  const { tokens, measured } = eventTokens(e);
  return (
    <span
      className="ml-auto shrink-0 font-mono text-muted-foreground text-xs"
      title={measured ? "measured from usage" : "estimated (chars/4)"}
    >
      {tokens ? tokenLabel(e, fmt) : ""}
      {tokens && share !== undefined
        ? ` · ${(share * PERCENT).toFixed(PCT_DECIMALS)}%`
        : ""}
    </span>
  );
};

/** What one transcript row needs; `onToggle` must be stable (see `EventRow`). */
interface EventRowProps {
  readonly collapseId: string;
  readonly e: TimelineEvent;
  readonly onToggle: (id: string, open: boolean) => void;
  readonly open: boolean;
  /** Share of the context window, shown only when rows are sorted by tokens. */
  readonly share?: number;
  readonly turn: number;
}

/** Two renders of the same row: same state, same rendered event fields. */
const sameRow = (prev: EventRowProps, next: EventRowProps) =>
  prev.open === next.open &&
  prev.turn === next.turn &&
  prev.share === next.share &&
  prev.collapseId === next.collapseId &&
  prev.onToggle === next.onToggle &&
  sameEvent(prev.e, next.e);

/** A tool row also needs its name and outcome, which come from the paired event.
 *  The parent resolves both so the row compares two strings instead of the whole
 *  tool index — the index is rebuilt on every re-analysis of a live session. */
interface ToolRowProps extends EventRowProps {
  readonly name: string;
  readonly state: ToolState;
}

const sameToolRow = (prev: ToolRowProps, next: ToolRowProps) =>
  prev.name === next.name && prev.state === next.state && sameRow(prev, next);

/**
 * A tool call or its result, rendered with the AI Elements `Tool` primitives at
 * transcript density: the header keeps the turn, outcome, summary and token
 * figures a forensics read needs; the content labels each payload pane.
 */
const ToolEventRowBody = ({
  collapseId,
  e,
  name,
  onToggle,
  open,
  share,
  state,
  turn,
}: ToolRowProps) => {
  const onOpenChange = (next: boolean) => onToggle(collapseId, next);
  const view = e.kind === "tool-call" ? callView(e) : null;
  const result = view ? null : resultPane(e);
  return (
    <Tool
      className="rounded-none border-0 border-b"
      data-collapse-id={collapseId}
      data-kind={e.kind}
      data-sidechain={e.isSidechain ? "true" : "false"}
      data-testid="history-event"
      data-tool-use-id={e.toolUseId}
      onOpenChange={onOpenChange}
      open={open}
    >
      <ToolHeader
        className="px-2 py-1.5 text-sm hover:bg-muted/40"
        icon={
          view ? undefined : (
            <CornerDownLeftIcon className="size-3.5 shrink-0 text-muted-foreground" />
          )
        }
        lead={<TurnGutter turn={turn} />}
        name={name}
        state={state}
      >
        {e.isSidechain ? (
          <Badge className="shrink-0" variant="secondary">
            sidechain
          </Badge>
        ) : null}
        <ContextEditBadge e={e} />
        <span className="truncate text-muted-foreground text-xs">
          {(view ? view.summary : e.preview) || "(empty)"}
        </span>
        <TokenCount e={e} share={share} />
      </ToolHeader>
      <ToolContent className="max-h-96 overflow-auto">
        <ContextEditNote e={e} />
        {view?.panes.map((p) => (
          <ToolPaneView key={p.label} pane={p} />
        ))}
        {result ? (
          <ToolResultView isError={e.isError === true} pane={result} />
        ) : null}
        <div className="px-3 empty:hidden">
          <EventImages e={e} />
        </div>
      </ToolContent>
    </Tool>
  );
};

/** One tool row, re-rendered only when its own content or state changes. */
const ToolEventRow = memo(ToolEventRowBody, sameToolRow);

/** One collapsible non-tool event; open state is controlled by the parent. */
const EventRowBody = ({
  e,
  turn,
  open,
  collapseId,
  onToggle,
  share,
}: EventRowProps) => {
  const onOpenChange = (next: boolean) => onToggle(collapseId, next);
  const emptyText =
    e.kind === "assistant-thinking" ? thinkingNote(e) : EMPTY_BODY;
  return (
    <Collapsible
      className="border-border border-b"
      data-collapse-id={collapseId}
      data-kind={e.kind}
      data-sidechain={e.isSidechain ? "true" : "false"}
      data-testid="history-event"
      onOpenChange={onOpenChange}
      open={open}
    >
      <CollapsibleTrigger className="flex w-full items-center gap-2 px-2 py-1.5 text-left text-sm hover:bg-muted/40 [&[data-state=open]>svg]:rotate-90">
        <ChevronRightIcon className="size-3.5 shrink-0 text-muted-foreground transition-transform" />
        <TurnGutter turn={turn} />
        <Badge className="shrink-0" variant="outline">
          {eventBadgeLabel(e)}
        </Badge>
        {e.isSidechain ? (
          <Badge className="shrink-0" variant="secondary">
            sidechain
          </Badge>
        ) : null}
        <ContextEditBadge e={e} />
        <span className="truncate text-muted-foreground text-xs">
          {e.preview || "(empty)"}
        </span>
        <TokenCount e={e} share={share} />
      </CollapsibleTrigger>
      <CollapsibleContent>
        <ContextEditNote e={e} />
        <EventBody
          className="max-h-96 overflow-auto rounded-none border-0 border-t"
          e={e}
          empty={emptyText}
        />
      </CollapsibleContent>
    </Collapsible>
  );
};

/** One transcript row, re-rendered only when its own content or state changes. */
const EventRow = memo(EventRowBody, sameRow);

/** What the table reads besides the rows: the window and the dumb zone. */
export type TableSession = Pick<
  AnalyzedSession,
  "contextWindow" | "dumbZoneCrossTurn" | "dumbZoneFraction"
>;

/** The dumb-zone notice, drawn above the row where the session crossed it. */
const DumbZoneDivider = ({ a }: { readonly a: TableSession }) => (
  <div
    className="bg-red-500/15 px-3 py-1.5 text-center font-medium text-red-300 text-xs"
    data-testid="dumbzone-divider"
  >
    entered dumb zone — {Math.round(a.dumbZoneFraction * PERCENT)}% (
    {fmt(a.dumbZoneFraction * a.contextWindow)} tok) crossed at turn{" "}
    {a.dumbZoneCrossTurn + 1}
  </div>
);

/** One table row, with the divider above it when the session crossed into the
 *  dumb zone here. */
const TableRowViewBody = ({
  a,
  divider,
  isOpen,
  onToggle,
  row: { e, pos },
  share,
  tools,
  turn,
}: {
  readonly a: TableSession;
  readonly divider: boolean;
  readonly isOpen: (id: string) => boolean;
  readonly onToggle: (id: string, open: boolean) => void;
  readonly row: TranscriptRowRef;
  readonly share: number | undefined;
  readonly tools: ToolIndex;
  readonly turn: number;
}) => {
  const id = eventCollapseId(pos);
  return (
    <>
      {divider ? <DumbZoneDivider a={a} /> : null}
      {isToolEvent(e) ? (
        <ToolEventRow
          collapseId={id}
          e={e}
          name={toolNameOf(e, tools)}
          onToggle={onToggle}
          open={isOpen(id)}
          share={share}
          state={toolState(e, tools)}
          turn={turn}
        />
      ) : (
        <EventRow
          collapseId={id}
          e={e}
          onToggle={onToggle}
          open={isOpen(id)}
          share={share}
          turn={turn}
        />
      )}
    </>
  );
};

export const TableRowView = memo(TableRowViewBody);

/** The bordered list of event rows, with the dumb-zone divider inline. Only
 *  the rows near the screen are mounted; `jump` is the place a link or the
 *  summary asked to land on. */
export const TranscriptTable = ({
  a,
  bulkKey,
  crossEvtIdx,
  eventAt,
  isOpen,
  jump,
  jumpIndex,
  landing,
  onDrawn,
  onSort,
  onToggle,
  revealHit,
  rows,
  sort,
  stickyHeight,
  tools,
  turns,
}: {
  readonly a: TableSession;
  /** Bumped by expand or collapse all, when every height changes at once. */
  readonly bulkKey: number;
  readonly crossEvtIdx: number;
  /** The whole event at a position, when rows are index rows; null while
   *  its page loads. */
  readonly eventAt?: (pos: number) => TimelineEvent | null;
  readonly isOpen: (id: string) => boolean;
  /** Where a link or the summary asked to go, if anywhere. */
  readonly jump: HistoryJump | null;
  /** The row `jump` lands on; -1 when hidden or there is no jump. */
  readonly jumpIndex: number;
  /** How far the jump got, shared by both layouts. */
  readonly landing: JumpLanding;
  /** Event positions of the drawn rows, after each change. */
  readonly onDrawn?: (positions: readonly number[]) => void;
  readonly onSort: (next: HistorySort) => void;
  readonly onToggle: (id: string, open: boolean) => void;
  /** Filled here so search can step to a hit that is not drawn. */
  readonly revealHit: RefObject<RevealHit | null>;
  /** Rows already in `sort` order. */
  readonly rows: readonly TranscriptRowRef<AnyEvent>[];
  readonly sort: HistorySort;
  /** Height of the sticky toolbar a jump must land below. */
  readonly stickyHeight: number;
  readonly tools: ToolIndex;
  readonly turns: readonly number[];
}) => {
  // The window share answers "how much did this one event cost me", which only
  // matters while ranking by tokens — in transcript order it is just noise.
  const showShare = sort.key === "tokens";
  // The divider marks a point in time, so it only reads right in forward order.
  const showDivider = isForwardOrder(sort);
  const shareOf = (e: AnyEvent) =>
    showShare && a.contextWindow > 0
      ? eventTokens(e).tokens / a.contextWindow
      : undefined;
  const dividerAt = (pos: number) => showDivider && pos === crossEvtIdx;

  const keys = useMemo(
    () => rows.map(({ e, pos }) => `${e.index}-${pos}`),
    [rows]
  );
  const list = useRef<VirtualListHandle>(null);
  const indexOfHit = useCallback(
    (id: string) => rows.findIndex(({ pos }) => eventCollapseId(pos) === id),
    [rows]
  );
  useRevealHit({ indexOf: indexOfHit, list, reveal: revealHit });
  useVirtualJump({
    align: jump ? alignOf(jump.target) : "start",
    done: landing.done,
    index: jumpIndex,
    list,
    target: jump?.key ?? null,
  });
  const stops = useMemo(
    () => tableTurnStops({ rows, sort, turns }),
    [rows, sort, turns]
  );
  useTurnNav({ list, stops });
  const cache = useRef<HydrateCache>(new WeakMap());
  const hydrate = useMemo(() => hydrator(eventAt, cache.current), [eventAt]);
  const onRange = useCallback(
    (first: number, last: number) =>
      onDrawn?.(rows.slice(first, last + 1).map(({ pos }) => pos)),
    [rows, onDrawn]
  );
  const estimate = ({ pos }: TranscriptRowRef<AnyEvent>) =>
    estimateTableRow({
      divider: dividerAt(pos),
      open: isOpen(eventCollapseId(pos)),
    });

  return (
    <div className="rounded-md border border-border">
      <SortHeader
        onSort={(key: HistorySortKey) => onSort(nextSort(sort, key))}
        sort={sort}
      />
      <VirtualList
        estimate={estimate}
        items={rows}
        keys={keys}
        measureKey={bulkKey}
        onRange={onRange}
        ref={list}
        render={(row) => {
          const full = hydrate.row(row);
          return full === null ? (
            <PendingRow height={estimate(row)} />
          ) : (
            <TableRowView
              a={a}
              divider={dividerAt(row.pos)}
              isOpen={isOpen}
              onToggle={onToggle}
              row={full}
              share={shareOf(row.e)}
              tools={tools}
              turn={turns[row.pos] ?? 0}
            />
          );
        }}
        scrollPaddingStart={stickyHeight}
        testId="table-transcript"
      />
      {rows.length === 0 ? (
        <p className="px-3 py-6 text-center text-muted-foreground text-sm">
          No events match the filters.
        </p>
      ) : null}
    </div>
  );
};
