/** Full collapsible history + subagents (Phase 8.3).
 *
 * Every transcript event in order (tool calls, results, attachments, assistant
 * text) collapsed by default, with search + type filter and the dumb-zone
 * divider rendered inline at the first crossing. Subagent (sidechain) transcripts
 * drill down as their own cards. The transcript is REDACTED BY DEFAULT behind a
 * persistent "review before sharing" banner; the reveal toggle re-fetches with
 * `redact:false` (handled by the parent atom).
 *
 * Tool calls and results render through the vendored AI Elements `Tool`
 * primitives; every other kind stays a plain disclosure row. One row per event
 * either way — pairing a call with its result into a single card would reorder
 * the transcript and merge two token figures, and this is a forensics view.
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
import { Button } from "@workspace/ui/components/button";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@workspace/ui/components/collapsible";
import { Input } from "@workspace/ui/components/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@workspace/ui/components/select";
import { Switch } from "@workspace/ui/components/switch";
import { cn } from "@workspace/ui/lib/utils";
import { fmt, fmtK, PERCENT } from "@workspace/viz/lib/session-format";
import {
  ChevronRightIcon,
  ChevronsDownUpIcon,
  ChevronsUpDownIcon,
  CornerDownLeftIcon,
  ShieldAlertIcon,
} from "lucide-react";
import { memo, useCallback, useEffect, useMemo, useRef } from "react";
import { setHashParam, useHashParam } from "../../lib/routes";
import {
  eventCollapseId,
  type SessionView,
  subagentCollapseId,
} from "../../lib/session-view";
import {
  callView,
  EMPTY_BODY,
  indexTools,
  isToolEvent,
  resultPane,
  toolNameOf,
  toolState,
} from "../../lib/tool-event";
import { sameEvent } from "../../lib/transcript-row";

/** Event-kind options for the history type filter. */
const KIND_OPTIONS = [
  { value: "all", label: "All types" },
  { value: "user-prompt", label: "User" },
  { value: "assistant-text", label: "Assistant" },
  { value: "tool-call", label: "Tool calls" },
  { value: "tool-result", label: "Tool results" },
  { value: "assistant-thinking", label: "Thinking" },
  { value: "attachment", label: "Attachments" },
] as const;

/** Map every event to the 1-based turn it belongs to (for grouping). */
const turnNumbers = (a: AnalyzedSession): number[] => {
  const reqToTurn = new Map(
    a.turns.map((t, i) => [t.requestId, i + 1] as const)
  );
  const out: number[] = [];
  let cur = 0;
  for (const e of a.events) {
    if (e.requestId && reqToTurn.has(e.requestId)) {
      cur = reqToTurn.get(e.requestId) ?? cur;
    }
    out.push(cur);
  }
  return out;
};

/** Turn-number gutter, shared by every transcript row. */
const TurnGutter = ({ turn }: { readonly turn: number }) => (
  <span className="w-8 shrink-0 font-mono text-muted-foreground text-xs">
    t{turn}
  </span>
);

/** Per-event token estimate, right-aligned. */
const TokenCount = ({ tokens }: { readonly tokens: number }) => (
  <span className="ml-auto shrink-0 font-mono text-muted-foreground text-xs">
    {tokens ? `~${fmt(tokens)}` : ""}
  </span>
);

/** What one transcript row needs; `onToggle` must be stable (see `EventRow`). */
interface EventRowProps {
  readonly collapseId: string;
  readonly e: TimelineEvent;
  readonly onToggle: (id: string, open: boolean) => void;
  readonly open: boolean;
  readonly turn: number;
}

/** Two renders of the same row: same state, same rendered event fields. */
const sameRow = (prev: EventRowProps, next: EventRowProps) =>
  prev.open === next.open &&
  prev.turn === next.turn &&
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
        <TokenCount tokens={e.tokensEst} />
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
        <TokenCount tokens={e.tokensEst} />
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

/** Subagent (sidechain) transcript cards — each runs in its own window. */
const Subagents = ({
  a,
  isOpen,
  onToggle,
}: {
  readonly a: AnalyzedSession;
  readonly isOpen: (id: string) => boolean;
  readonly onToggle: (id: string, open: boolean) => void;
}) => {
  if (a.subagents.length === 0) {
    return null;
  }
  return (
    <div className="flex flex-col gap-2" data-testid="subagents">
      <h3 className="font-medium text-sm">Subagents ({a.subagents.length})</h3>
      <p className="text-muted-foreground text-xs">
        Each runs in its own context window — these tokens do not count against
        the main session.
      </p>
      {a.subagents.map((s) => (
        <Collapsible
          className="rounded-md border border-border"
          data-testid="subagent-card"
          key={s.id}
          onOpenChange={(o) => onToggle(subagentCollapseId(s.id), o)}
          open={isOpen(subagentCollapseId(s.id))}
        >
          <CollapsibleTrigger className="flex w-full items-center gap-2 px-3 py-2 text-left text-sm hover:bg-muted/40 [&[data-state=open]>svg]:rotate-90">
            <ChevronRightIcon className="size-3.5 shrink-0 text-muted-foreground transition-transform" />
            <Badge variant="secondary">{s.agentType ?? "agent"}</Badge>
            <span className="font-mono text-xs">{s.id}</span>
            <span className="truncate text-muted-foreground text-xs">
              {s.description ?? "subagent"} · {s.turns} turns
            </span>
            <span className="ml-auto shrink-0 font-mono text-muted-foreground text-xs">
              peak {fmtK(s.peakContextTokens)}
            </span>
          </CollapsibleTrigger>
          <CollapsibleContent>
            <pre className="whitespace-pre-wrap break-words bg-muted/30 px-3 py-2 text-xs">
              {`agentType: ${s.agentType ?? "—"}
description: ${s.description ?? "—"}
toolUseId: ${s.toolUseId ?? "—"}
turns: ${s.turns}
peak context: ${fmt(s.peakContextTokens)}
path: ${s.path}`}
            </pre>
          </CollapsibleContent>
        </Collapsible>
      ))}
    </div>
  );
};

/** Full history section with filters, redaction banner, and subagent drill-down. */
export const SessionHistory = ({
  a,
  view,
}: {
  readonly a: AnalyzedSession;
  readonly view: SessionView;
}) => {
  const { query, kind, redacted } = view.state;
  const turns = useMemo(() => turnNumbers(a), [a]);
  const tools = useMemo(() => indexTools(a), [a]);

  const crossEvtIdx =
    a.dumbZoneCrossTurn >= 0
      ? a.events.findIndex(
          (e) => e.requestId === a.turns[a.dumbZoneCrossTurn]?.requestId
        )
      : -1;

  const visible = useMemo(
    () =>
      a.events
        .map((e, pos) => ({ e, pos }))
        .filter(({ e }) => e.kind !== "system")
        .filter(({ e }) => kind === "all" || e.kind === kind)
        .filter(({ e }) => {
          if (query.length === 0) {
            return true;
          }
          const hay = `${e.title} ${e.preview}`.toLowerCase();
          return hay.includes(query.toLowerCase());
        }),
    [a.events, kind, query]
  );

  // Bulk expand lives in the URL (`?expand=all`) so an all-open view is a
  // shareable link; individual rows persist in the per-session set instead.
  const expandAll = useHashParam("expand") === "all";
  const allIds = useMemo(
    () => [
      ...visible.map(({ pos }) => eventCollapseId(pos)),
      ...a.subagents.map((s) => subagentCollapseId(s.id)),
    ],
    [visible, a.subagents]
  );

  const isOpen = (id: string) => expandAll || view.isExpanded(id);

  // `allIds` changes whenever a live session grows, but the toggle handler must
  // not: every row is memoized on it, and a new handler would re-render the whole
  // transcript on each append. Read the current ids through a ref instead — the
  // handler only ever reads them on click.
  const allIdsRef = useRef(allIds);
  useEffect(() => {
    allIdsRef.current = allIds;
  }, [allIds]);

  const onToggle = useCallback(
    (id: string, open: boolean) => {
      if (expandAll) {
        if (open) {
          return;
        }
        // Collapsing one while all-open: materialize the rest, drop the flag.
        view.setExpanded(allIdsRef.current.filter((x) => x !== id));
        setHashParam("expand", null);
        return;
      }
      view.toggleExpanded(id, open);
    },
    [expandAll, view.setExpanded, view.toggleExpanded]
  );

  const allOpen =
    expandAll ||
    (allIds.length > 0 && allIds.every((id) => view.isExpanded(id)));
  const toggleAll = () => {
    if (allOpen) {
      setHashParam("expand", null);
      view.setExpanded([]);
    } else {
      setHashParam("expand", "all");
    }
  };

  return (
    <section className="flex flex-col gap-3" data-testid="session-history">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="font-semibold text-base">Full history</h2>
        <div className="flex items-center gap-2 text-sm">
          <span className="text-muted-foreground" id="redact-toggle-label">
            Reveal secrets
          </span>
          <Switch
            aria-labelledby="redact-toggle-label"
            checked={!redacted}
            data-testid="redact-toggle"
            onCheckedChange={(checked) => view.setRedacted(!checked)}
          />
        </div>
      </div>

      <div
        className={cn(
          "flex items-center gap-2 rounded-md border px-3 py-2 text-destructive text-xs",
          redacted
            ? "border-destructive/30 bg-destructive/5"
            : "border-destructive/60 bg-destructive/15 font-medium"
        )}
        data-testid="redaction-banner"
      >
        <ShieldAlertIcon className="size-4 shrink-0" />
        {redacted ? (
          <span>
            Secrets are redacted by default. This transcript may still contain
            sensitive data — review before sharing.
          </span>
        ) : (
          <span data-testid="redaction-off">
            Redaction is OFF — the raw transcript (including secrets) is shown.
            Do not share.
          </span>
        )}
      </div>

      <div className="flex flex-wrap items-center gap-3">
        <Input
          className="max-w-xs"
          data-testid="history-search"
          onChange={(e) => view.setQuery(e.target.value)}
          placeholder="Search history…"
          value={query}
        />
        <Select onValueChange={view.setKind} value={kind}>
          <SelectTrigger className="w-40" data-testid="history-kind-filter">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {KIND_OPTIONS.map((o) => (
              <SelectItem key={o.value} value={o.value}>
                {o.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <span className="text-muted-foreground text-xs">
          {visible.length} events · {a.dumbZoneTurns}/{a.turnCount} turns in
          dumb zone
        </span>
        {allIds.length > 0 ? (
          <Button
            className="ml-auto"
            data-testid="history-expand-all"
            onClick={toggleAll}
            size="sm"
            variant="outline"
          >
            {allOpen ? (
              <ChevronsDownUpIcon className="size-3.5" />
            ) : (
              <ChevronsUpDownIcon className="size-3.5" />
            )}
            {allOpen ? "Collapse all" : "Expand all"}
          </Button>
        ) : null}
      </div>

      <div className="rounded-md border border-border">
        {visible.map(({ e, pos }) => (
          <div key={`${e.index}-${pos}`}>
            {pos === crossEvtIdx ? (
              <div
                className="bg-red-500/15 px-3 py-1.5 text-center font-medium text-red-300 text-xs"
                data-testid="dumbzone-divider"
              >
                entered dumb zone — {Math.round(a.dumbZoneFraction * PERCENT)}%
                ({fmt(a.dumbZoneFraction * a.contextWindow)} tok) crossed at
                turn {a.dumbZoneCrossTurn + 1}
              </div>
            ) : null}
            {isToolEvent(e) ? (
              <ToolEventRow
                collapseId={eventCollapseId(pos)}
                e={e}
                name={toolNameOf(e, tools)}
                onToggle={onToggle}
                open={isOpen(eventCollapseId(pos))}
                state={toolState(e, tools)}
                turn={turns[pos] ?? 0}
              />
            ) : (
              <EventRow
                collapseId={eventCollapseId(pos)}
                e={e}
                onToggle={onToggle}
                open={isOpen(eventCollapseId(pos))}
                turn={turns[pos] ?? 0}
              />
            )}
          </div>
        ))}
        {visible.length === 0 ? (
          <p className="px-3 py-6 text-center text-muted-foreground text-sm">
            No events match the filters.
          </p>
        ) : null}
      </div>

      <Subagents a={a} isOpen={isOpen} onToggle={onToggle} />
    </section>
  );
};
