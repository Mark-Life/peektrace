/** Full history: one filtered event list, two readings of it.
 *
 * The shell owns everything shared — search, the type filter, the redaction
 * toggle and its banner, expand-all, and the single `visible` array — then hands
 * the identical rows to either renderer. Layout, sort and the type filter come
 * from the history prefs shared across sessions; the search query belongs to
 * this view and clears when another session opens. The chat layout is the default, the
 * readable pass; the dense table is the forensic answer ("what did this cost,
 * in what order") over the same events. Keeping the filtering here is what
 * stops the two from drifting apart. The transcript is REDACTED BY DEFAULT
 * behind a persistent "review before sharing" banner; the reveal toggle
 * re-fetches with `redact:false` (handled by the parent atom).
 */
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import { Button } from "@workspace/ui/components/button";
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
import {
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { chatIdsOf } from "../../lib/chat-lane";
import { useHistoryPrefs } from "../../lib/history-prefs";
import {
  type StepDir,
  searchIndexOf,
  stepHit,
  visibleRows,
} from "../../lib/history-search";
import { DEFAULT_HISTORY_SORT } from "../../lib/history-sort";
import { setHashParam, useHashParam } from "../../lib/routes";
import { useScrollToCall, useToolMarks } from "../../lib/session-markers";
import {
  eventCollapseId,
  type SessionView,
  subagentCollapseId,
} from "../../lib/session-view";
import { indexTools } from "../../lib/tool-event";
import { cardOf, useSearchHighlight } from "../../lib/use-search-highlight";
import { HistorySearch } from "./history-search";
import { Subagents } from "./subagents";
import { TranscriptChat } from "./transcript-chat";
import { TranscriptTable } from "./transcript-table";

/** Event-kind options for the history type filter. */
const KIND_OPTIONS = [
  { value: "all", label: "All types" },
  { value: "system-prompt", label: "System prompt" },
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
  const { redacted } = view.state;
  const prefs = useHistoryPrefs();
  const { kind, sort, mode } = prefs;
  // Like browser find: the query and current hit belong to this session and
  // reset when another one opens (this view stays mounted across sessions).
  const [query, setQuery] = useState("");
  const [currentId, setCurrentId] = useState<string | null>(null);
  const [step, setStep] = useState(0);
  const [querySid, setQuerySid] = useState(sid);
  if (querySid !== sid) {
    setQuerySid(sid);
    setQuery("");
    setCurrentId(null);
  }
  const deferredQuery = useDeferredValue(query);
  const searchIndex = useMemo(() => searchIndexOf(a.events), [a.events]);
  const turns = useMemo(() => turnNumbers(a), [a]);
  const tools = useMemo(() => indexTools(a), [a]);
  // Stats marks are an annotation on the same calls the tables counted; a
  // failed or pending marker query leaves the transcript exactly as it was.
  const marks = useToolMarks(sid);
  const activeMark = useHashParam("mark");
  const callId = useHashParam("call");

  // A conversation has one order — its own. The persisted sort is left alone, so
  // switching back to the table restores the column sort it had.
  const effectiveSort = mode === "chat" ? DEFAULT_HISTORY_SORT : sort;

  const crossEvtIdx =
    a.dumbZoneCrossTurn >= 0
      ? a.events.findIndex(
          (e) => e.requestId === a.turns[a.dumbZoneCrossTurn]?.requestId
        )
      : -1;

  const visible = useMemo(
    () =>
      visibleRows({
        events: a.events,
        index: searchIndex,
        kind,
        query: deferredQuery,
        sort: effectiveSort,
      }),
    [a.events, searchIndex, kind, deferredQuery, effectiveSort]
  );

  useScrollToCall(callId, visible);
  // A stats link names a call the search or type filter can hide: say
  // so, rather than leave the reader at the top of the transcript.
  const callHidden =
    callId !== null &&
    a.events.some((e) => e.toolUseId === callId) &&
    !visible.some(({ e }) => e.toolUseId === callId);
  const showCall = () => {
    setQuery("");
    prefs.setKind("all");
  };

  // Bulk expand lives in the URL (`?expand=all`) so an all-open view is a
  // shareable link; individual rows persist in the per-session set instead.
  const expandAll = useHashParam("expand") === "all";
  const transcriptIds = useMemo(
    () =>
      mode === "chat"
        ? chatIdsOf(visible)
        : visible.map(({ pos }) => eventCollapseId(pos)),
    [mode, visible]
  );
  const allIds = useMemo(
    () => [
      ...transcriptIds,
      ...a.subagents.map((s) => subagentCollapseId(s.id)),
    ],
    [transcriptIds, a.subagents]
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

  // The search filters the list, so every drawn card is a hit; the current one
  // is tracked by id so a live append keeps it, and falls back to none when a
  // layout or kind change drops it.
  const searching = deferredQuery.length > 0;
  const hits = searching ? transcriptIds : [];
  const currentIdx = currentId === null ? -1 : hits.indexOf(currentId);
  const liveCurrent = currentIdx < 0 ? null : currentId;
  const onQuery = (next: string) => {
    setQuery(next);
    setCurrentId(null);
  };
  const onStep = (dir: StepDir) => {
    const next = stepHit({ current: liveCurrent, dir, hits });
    if (next === null) {
      return;
    }
    if (!isOpen(next)) {
      onToggle(next, true);
    }
    setCurrentId(next);
    setStep((n) => n + 1);
  };

  const transcriptRef = useRef<HTMLDivElement>(null);
  useSearchHighlight({
    containerRef: transcriptRef,
    currentId: liveCurrent,
    ids: transcriptIds,
    query: deferredQuery,
  });
  const scrollIdsRef = useRef(transcriptIds);
  useEffect(() => {
    scrollIdsRef.current = transcriptIds;
  }, [transcriptIds]);
  // `step` re-runs this when Enter lands on the same lone hit again.
  // biome-ignore lint/correctness/useExhaustiveDependencies: step is the trigger
  useEffect(() => {
    const container = transcriptRef.current;
    if (container === null || liveCurrent === null) {
      return;
    }
    cardOf({
      container,
      id: liveCurrent,
      ids: scrollIdsRef.current,
    })?.scrollIntoView({ block: "center" });
  }, [liveCurrent, step]);

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
            sensitive data, and images are shown as recorded — review before
            sharing.
          </span>
        ) : (
          <span data-testid="redaction-off">
            Redaction is OFF — the raw transcript (including secrets) is shown.
            Do not share.
          </span>
        )}
      </div>

      <div className="flex flex-wrap items-center gap-3">
        <HistorySearch
          count={hits.length}
          current={currentIdx}
          onClear={() => onQuery("")}
          onQuery={onQuery}
          onStep={onStep}
          query={query}
        />
        <Select onValueChange={prefs.setKind} value={kind}>
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
        <fieldset
          aria-label="Transcript layout"
          className="ml-auto flex items-center gap-0.5 rounded-md border border-border p-0.5"
          data-testid="history-layout-toggle"
        >
          <Button
            data-testid="history-layout-chat"
            onClick={() => prefs.setMode("chat")}
            size="sm"
            variant={mode === "chat" ? "secondary" : "ghost"}
          >
            <MessagesSquareIcon className="size-3.5" />
            Chat
          </Button>
          <Button
            data-testid="history-layout-table"
            onClick={() => prefs.setMode("table")}
            size="sm"
            variant={mode === "table" ? "secondary" : "ghost"}
          >
            <TableIcon className="size-3.5" />
            Table
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

      {callHidden ? (
        <div
          className="flex flex-wrap items-center gap-2 rounded-md border border-border bg-muted/40 px-3 py-2 text-xs"
          data-testid="history-call-hidden"
        >
          <span>
            The call this link points to is hidden by the search or type filter.
          </span>
          <Button
            className="h-6 px-2 text-xs"
            onClick={showCall}
            size="sm"
            variant="outline"
          >
            Clear filters
          </Button>
        </div>
      ) : null}

      <div ref={transcriptRef}>
        {mode === "chat" ? (
          <TranscriptChat
            a={a}
            activeMark={activeMark}
            allOpen={allOpen}
            crossEvtIdx={crossEvtIdx}
            isOpen={isOpen}
            marks={marks}
            onToggle={onToggle}
            queryActive={searching}
            rows={visible}
            tools={tools}
            turns={turns}
          />
        ) : (
          <TranscriptTable
            a={a}
            crossEvtIdx={crossEvtIdx}
            isOpen={isOpen}
            onSort={prefs.setSort}
            onToggle={onToggle}
            rows={visible}
            sort={sort}
            tools={tools}
            turns={turns}
          />
        )}
      </div>

      <Subagents a={a} isOpen={isOpen} onToggle={onToggle} parent={sid} />
    </section>
  );
};
