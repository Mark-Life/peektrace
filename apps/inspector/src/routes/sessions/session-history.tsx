/** Full history: one filtered event list, two readings of it.
 *
 * The shell owns everything shared — search, the type filter, the redaction
 * toggle and its banner, expand-all, and the single `visible` array — then hands
 * the identical rows to either renderer. The dense table stays the default and
 * the forensic answer ("what did this cost, in what order"); the chat layout is
 * the readable pass over the same events. Keeping the filtering here is what
 * stops the two from drifting apart. The transcript is REDACTED BY DEFAULT
 * behind a persistent "review before sharing" banner; the reveal toggle
 * re-fetches with `redact:false` (handled by the parent atom).
 */
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import { Button } from "@workspace/ui/components/button";
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
import {
  ChevronsDownUpIcon,
  ChevronsUpDownIcon,
  MessagesSquareIcon,
  ShieldAlertIcon,
  TableIcon,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef } from "react";
import { chatIdsOf } from "../../lib/chat-lane";
import { setHashParam, useHashParam } from "../../lib/routes";
import { useToolMarks } from "../../lib/session-markers";
import {
  eventCollapseId,
  type HistorySort,
  type SessionView,
  subagentCollapseId,
} from "../../lib/session-view";
import { indexTools } from "../../lib/tool-event";
import {
  setTranscriptMode,
  useTranscriptMode,
} from "../../lib/transcript-mode";
import { Subagents } from "./subagents";
import { TranscriptChat } from "./transcript-chat";
import { TranscriptTable } from "./transcript-table";

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

/** Row-order options; sorting by size replaces a separate "biggest items" list. */
const SORT_OPTIONS: readonly { value: HistorySort; label: string }[] = [
  { value: "order", label: "Transcript order" },
  { value: "size", label: "Biggest first" },
];

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

/** Full history section with filters, redaction banner, and subagent drill-down. */
export const SessionHistory = ({
  a,
  sid,
  view,
}: {
  readonly a: AnalyzedSession;
  /** The session id, for the marker query — `AnalyzedSession` carries none. */
  readonly sid: string;
  readonly view: SessionView;
}) => {
  const { query, kind, redacted, sort } = view.state;
  const mode = useTranscriptMode();
  const turns = useMemo(() => turnNumbers(a), [a]);
  const tools = useMemo(() => indexTools(a), [a]);
  // Stats marks are an annotation on the same calls the tables counted; a
  // failed or pending marker query leaves the transcript exactly as it was.
  const marks = useToolMarks(sid);
  const activeMark = useHashParam("mark");

  // A conversation has one order — its own. The persisted sort is left alone, so
  // switching back to the table restores "Biggest first" if that is how it was.
  const effectiveSort = mode === "chat" ? "order" : sort;

  const crossEvtIdx =
    a.dumbZoneCrossTurn >= 0
      ? a.events.findIndex(
          (e) => e.requestId === a.turns[a.dumbZoneCrossTurn]?.requestId
        )
      : -1;

  const visible = useMemo(() => {
    const rows = a.events
      .map((e, pos) => ({ e, pos }))
      .filter(({ e }) => e.kind !== "system")
      .filter(({ e }) => kind === "all" || e.kind === kind)
      .filter(({ e }) => {
        if (query.length === 0) {
          return true;
        }
        const hay = `${e.title} ${e.preview}`.toLowerCase();
        return hay.includes(query.toLowerCase());
      });
    // Size order keeps each row's original position, so open rows, the turn
    // gutter and deep links all survive the re-sort.
    return effectiveSort === "size"
      ? rows.sort((x, y) => y.e.tokensEst - x.e.tokensEst)
      : rows;
  }, [a.events, kind, query, effectiveSort]);

  // Bulk expand lives in the URL (`?expand=all`) so an all-open view is a
  // shareable link; individual rows persist in the per-session set instead.
  const expandAll = useHashParam("expand") === "all";
  const allIds = useMemo(
    () => [
      ...(mode === "chat"
        ? chatIdsOf(visible)
        : visible.map(({ pos }) => eventCollapseId(pos))),
      ...a.subagents.map((s) => subagentCollapseId(s.id)),
    ],
    [mode, visible, a.subagents]
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
        {mode === "table" ? (
          <Select
            onValueChange={(v) => view.setSort(v as HistorySort)}
            value={sort}
          >
            <SelectTrigger className="w-44" data-testid="history-sort">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {SORT_OPTIONS.map((o) => (
                <SelectItem key={o.value} value={o.value}>
                  {o.label}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        ) : null}
        <span className="text-muted-foreground text-xs">
          {visible.length} events · {a.dumbZoneTurns}/{a.turnCount} turns in
          dumb zone
        </span>
        <fieldset
          aria-label="Transcript layout"
          className="ml-auto flex items-center gap-0.5 rounded-md border border-border p-0.5"
          data-testid="history-layout-toggle"
        >
          <Button
            data-testid="history-layout-table"
            onClick={() => setTranscriptMode("table")}
            size="sm"
            variant={mode === "table" ? "secondary" : "ghost"}
          >
            <TableIcon className="size-3.5" />
            Table
          </Button>
          <Button
            data-testid="history-layout-chat"
            onClick={() => setTranscriptMode("chat")}
            size="sm"
            variant={mode === "chat" ? "secondary" : "ghost"}
          >
            <MessagesSquareIcon className="size-3.5" />
            Chat
          </Button>
        </fieldset>
        {allIds.length > 0 ? (
          <Button
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

      {mode === "chat" ? (
        <TranscriptChat
          a={a}
          activeMark={activeMark}
          allOpen={allOpen}
          crossEvtIdx={crossEvtIdx}
          isOpen={isOpen}
          marks={marks}
          onToggle={onToggle}
          queryActive={query.length > 0}
          rows={visible}
          tools={tools}
          turns={turns}
        />
      ) : (
        <TranscriptTable
          a={a}
          crossEvtIdx={crossEvtIdx}
          isOpen={isOpen}
          onToggle={onToggle}
          rows={visible}
          showShare={effectiveSort === "size"}
          tools={tools}
          turns={turns}
        />
      )}

      <Subagents a={a} isOpen={isOpen} onToggle={onToggle} />
    </section>
  );
};
