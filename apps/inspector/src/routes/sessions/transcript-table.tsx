/** The dense, one-row-per-event transcript — the forensics reading.
 *
 * Every event keeps its own row, in the order (or the size ranking) the shell
 * asked for. Tool calls and results render through the vendored AI Elements
 * `Tool` primitives; every other kind stays a plain disclosure row. One row per
 * event either way — pairing a call with its result into a single card would
 * reorder the transcript and merge two token figures, and this is a forensics
 * view. (The chat layout pairs them; it also never sums the two figures.)
 */
import { eventBadgeLabel } from "@workspace/core/services/sessions/labels";
import type {
  AnalyzedSession,
  TimelineEvent,
} from "@workspace/core/services/sessions/schema";
import {
  Tool,
  ToolContent,
  ToolDiff,
  ToolHeader,
  ToolInput,
  ToolOutput,
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
import { memo } from "react";
import type { TranscriptRowRef } from "../../lib/chat-lane";
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
import { sameEvent } from "../../lib/transcript-row";

/** Decimal places for the per-event window-share percent (size order only). */
const PCT_DECIMALS = 2;

/** Turn-number gutter, shared by every transcript row. */
const TurnGutter = ({ turn }: { readonly turn: number }) => (
  <span className="w-8 shrink-0 font-mono text-muted-foreground text-xs">
    t{turn}
  </span>
);

/** Per-event token estimate, right-aligned; `share` is its window fraction. */
const TokenCount = ({
  tokens,
  share,
}: {
  readonly tokens: number;
  readonly share?: number | undefined;
}) => (
  <span className="ml-auto shrink-0 font-mono text-muted-foreground text-xs">
    {tokens ? `~${fmt(tokens)}` : ""}
    {tokens && share !== undefined
      ? ` · ${(share * PERCENT).toFixed(PCT_DECIMALS)}%`
      : ""}
  </span>
);

/** What one transcript row needs; `onToggle` must be stable (see `EventRow`). */
interface EventRowProps {
  readonly collapseId: string;
  readonly e: TimelineEvent;
  readonly onToggle: (id: string, open: boolean) => void;
  readonly open: boolean;
  /** Share of the context window, shown only when rows are sorted by size. */
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
      data-kind={e.kind}
      data-sidechain={e.isSidechain ? "true" : "false"}
      data-testid="history-event"
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
        <span className="truncate text-muted-foreground text-xs">
          {(view ? view.summary : e.preview) || "(empty)"}
        </span>
        <TokenCount share={share} tokens={e.tokensEst} />
      </ToolHeader>
      <ToolContent className="max-h-96 overflow-auto">
        {view
          ? view.panes.map((p) =>
              p.type === "diff" ? (
                <ToolDiff
                  diff={p.diff}
                  key={p.label}
                  label={p.label}
                  language={p.language}
                  newCode={p.newCode}
                  oldCode={p.oldCode}
                />
              ) : (
                <ToolInput
                  code={p.code}
                  key={p.label}
                  label={p.label}
                  language={p.language}
                />
              )
            )
          : null}
        {result ? (
          <ToolOutput
            code={result.code}
            isError={e.isError === true}
            language={result.language}
          />
        ) : null}
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
  const hasBody = e.body.trim().length > 0;
  const emptyText =
    e.kind === "assistant-thinking"
      ? "Thinking content is not stored in the transcript (only a signature). Its token cost is in the timeline 'thinking' band."
      : EMPTY_BODY;
  return (
    <Collapsible
      className="border-border border-b"
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
        <span className="truncate text-muted-foreground text-xs">
          {e.preview || "(empty)"}
        </span>
        <TokenCount share={share} tokens={e.tokensEst} />
      </CollapsibleTrigger>
      <CollapsibleContent>
        <pre className="wrap-break-word max-h-96 overflow-auto whitespace-pre-wrap bg-muted/30 px-3 py-2 text-xs">
          {hasBody ? e.body : emptyText}
        </pre>
      </CollapsibleContent>
    </Collapsible>
  );
};

/** One transcript row, re-rendered only when its own content or state changes. */
const EventRow = memo(EventRowBody, sameRow);

/** The bordered list of event rows, with the dumb-zone divider inline. */
export const TranscriptTable = ({
  a,
  crossEvtIdx,
  isOpen,
  onToggle,
  rows,
  showShare,
  tools,
  turns,
}: {
  readonly a: AnalyzedSession;
  readonly crossEvtIdx: number;
  readonly isOpen: (id: string) => boolean;
  readonly onToggle: (id: string, open: boolean) => void;
  readonly rows: readonly TranscriptRowRef[];
  /** Window share per row: only meaningful while ranking by size. */
  readonly showShare: boolean;
  readonly tools: ToolIndex;
  readonly turns: readonly number[];
}) => {
  // The window share answers "how much did this one event cost me", which only
  // matters while ranking by size — in transcript order it is just noise.
  const shareOf = (e: TimelineEvent) =>
    showShare && a.contextWindow > 0
      ? e.tokensEst / a.contextWindow
      : undefined;

  return (
    <div className="rounded-md border border-border">
      {rows.map(({ e, pos }) => (
        <div key={`${e.index}-${pos}`}>
          {!showShare && pos === crossEvtIdx ? (
            <div
              className="bg-red-500/15 px-3 py-1.5 text-center font-medium text-red-300 text-xs"
              data-testid="dumbzone-divider"
            >
              entered dumb zone — {Math.round(a.dumbZoneFraction * PERCENT)}% (
              {fmt(a.dumbZoneFraction * a.contextWindow)} tok) crossed at turn{" "}
              {a.dumbZoneCrossTurn + 1}
            </div>
          ) : null}
          {isToolEvent(e) ? (
            <ToolEventRow
              collapseId={eventCollapseId(pos)}
              e={e}
              name={toolNameOf(e, tools)}
              onToggle={onToggle}
              open={isOpen(eventCollapseId(pos))}
              share={shareOf(e)}
              state={toolState(e, tools)}
              turn={turns[pos] ?? 0}
            />
          ) : (
            <EventRow
              collapseId={eventCollapseId(pos)}
              e={e}
              onToggle={onToggle}
              open={isOpen(eventCollapseId(pos))}
              share={shareOf(e)}
              turn={turns[pos] ?? 0}
            />
          )}
        </div>
      ))}
      {rows.length === 0 ? (
        <p className="px-3 py-6 text-center text-muted-foreground text-sm">
          No events match the filters.
        </p>
      ) : null}
    </div>
  );
};
