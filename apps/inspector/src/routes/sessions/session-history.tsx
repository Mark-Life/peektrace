/** Full history: one filtered event list, two readings of it.
 *
 * The shell owns everything shared — search, the type filter, the redaction
 * toggle and its banner, expand-all, and the single `visible` array — then hands
 * the identical rows to either renderer. Layout, sort and filters come from the
 * history prefs shared across sessions. The chat layout is the default, the
 * readable pass; the dense table is the forensic answer ("what did this cost,
 * in what order") over the same events. Keeping the filtering here is what
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
  ArrowUpIcon,
  ChevronsDownUpIcon,
  ChevronsUpDownIcon,
  MessagesSquareIcon,
  ShieldAlertIcon,
  TableIcon,
} from "lucide-react";
import {
  type CSSProperties,
  type RefObject,
  useCallback,
  useDeferredValue,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { chatIdsOf } from "../../lib/chat-lane";
import { filterRows, searchIndex } from "../../lib/history-filter";
import { type HistoryJump, isHiddenJump } from "../../lib/history-jump";
import { useStickyHeight, useSubagentsJump } from "../../lib/history-landing";
import { useHistoryPrefs } from "../../lib/history-prefs";
import { DEFAULT_HISTORY_SORT, sortRows } from "../../lib/history-sort";
import { setHashParam, useHashParam } from "../../lib/routes";
import { useToolMarks } from "../../lib/session-markers";
import {
  eventCollapseId,
  type SessionView,
  subagentCollapseId,
} from "../../lib/session-view";
import { indexTools } from "../../lib/tool-event";
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

/** Says a link or a summary jump points at something the search or type
 *  filter hides, with the way to show it. */
const HiddenNote = ({
  hidden,
  onClear,
}: {
  readonly hidden: "call" | "jump" | false;
  readonly onClear: () => void;
}) =>
  hidden ? (
    <div
      className="flex flex-wrap items-center gap-2 rounded-md border border-border bg-muted/40 px-3 py-2 text-xs"
      data-testid="history-call-hidden"
    >
      <span>
        {hidden === "call"
          ? "The call this link points to is hidden by the search or type filter."
          : "The place you jumped to is hidden by the search or type filter."}
      </span>
      <Button
        className="h-6 px-2 text-xs"
        onClick={onClear}
        size="sm"
        variant="outline"
      >
        Clear filters
      </Button>
    </div>
  ) : null;

/** Keeps the warning in sight once the banner has scrolled away. */
const RedactionOffBadge = ({ redacted }: { readonly redacted: boolean }) =>
  redacted ? null : (
    <span
      className="inline-flex items-center gap-1 rounded-md border border-destructive/60 bg-destructive/15 px-1.5 py-0.5 font-medium text-destructive text-xs"
      data-testid="sticky-redaction-off"
    >
      <ShieldAlertIcon className="size-3.5" />
      Redaction off
    </span>
  );

/** Full history section with filters, redaction banner, and subagent drill-down. */
export const SessionHistory = ({
  a,
  jump,
  onSummary,
  sectionRef,
  sid,
  view,
}: {
  readonly a: AnalyzedSession;
  /** Where the summary asked to go, if anywhere. */
  readonly jump: HistoryJump | null;
  /** Scrolls back up to the summary. */
  readonly onSummary: () => void;
  /** The section element, for jumps from outside it. */
  readonly sectionRef: RefObject<HTMLElement | null>;
  /** The session id, for the marker query — `AnalyzedSession` carries none. */
  readonly sid: string;
  readonly view: SessionView;
}) => {
  const { redacted } = view.state;
  const prefs = useHistoryPrefs();
  const { query, kind, sort, mode } = prefs;
  const turns = useMemo(() => turnNumbers(a), [a]);
  const tools = useMemo(() => indexTools(a), [a]);
  // Stats marks are an annotation on the same calls the tables counted; a
  // failed or pending marker query leaves the transcript exactly as it was.
  const marks = useToolMarks(sid);
  const activeMark = useHashParam("mark");
  const callId = useHashParam("call");
  // Lives here, not in a layout, so switching layouts does not land again.
  const jumpDone = useRef<string | null>(null);
  const chipDone = useRef<string | null>(null);
  const [bar, setBar] = useState<HTMLDivElement | null>(null);
  const stickyHeight = useStickyHeight(bar);
  useSubagentsJump({ done: chipDone, jump, section: sectionRef });

  // A conversation has one order — its own. The persisted sort is left alone, so
  // switching back to the table restores the column sort it had.
  const effectiveSort = mode === "chat" ? DEFAULT_HISTORY_SORT : sort;

  const crossEvtIdx =
    a.dumbZoneCrossTurn >= 0
      ? a.events.findIndex(
          (e) => e.requestId === a.turns[a.dumbZoneCrossTurn]?.requestId
        )
      : -1;

  // The input follows every keystroke; the transcript follows when React has
  // time, so typing never waits on a re-filter of thousands of events.
  const deferredQuery = useDeferredValue(query);
  const stale = query !== deferredQuery;
  const hay = useMemo(() => searchIndex(a.events), [a.events]);
  const visible = useMemo(
    () =>
      sortRows(
        filterRows({ events: a.events, hay, kind, query: deferredQuery }),
        effectiveSort
      ),
    [a.events, hay, kind, deferredQuery, effectiveSort]
  );

  // A stats link names a call the saved search or type filter can hide: say
  // so, rather than leave the reader at the top of the transcript.
  const callHidden =
    callId !== null &&
    a.events.some((e) => e.toolUseId === callId) &&
    !visible.some(({ e }) => e.toolUseId === callId);
  // A summary jump the filters hide gets the same note, until it has landed.
  const allRows = useMemo(
    () => a.events.map((e, pos) => ({ e, pos })),
    [a.events]
  );
  const jumpHidden = isHiddenJump({
    all: allRows,
    jump,
    landed: chipDone.current,
    sort: effectiveSort,
    turns,
    visible,
  });
  const showCall = () => {
    prefs.setQuery("");
    prefs.setKind("all");
  };

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
  const expandLabel = allOpen ? "Collapse all" : "Expand all";
  // Re-measures every block only on a bulk action, not when `allOpen` flips
  // because one block was opened by hand or the filter changed.
  const [bulkKey, setBulkKey] = useState(0);
  const toggleAll = () => {
    setBulkKey((k) => k + 1);
    if (allOpen) {
      setHashParam("expand", null);
      view.setExpanded([]);
    } else {
      setHashParam("expand", "all");
    }
  };

  return (
    <section
      className="flex flex-col gap-3 [&_[data-tool-use-id]]:scroll-mt-(--history-sticky) [&_li]:scroll-mt-(--history-sticky)"
      data-testid="session-history"
      ref={sectionRef}
      style={{ "--history-sticky": `${stickyHeight}px` } as CSSProperties}
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="font-semibold text-base outline-none" tabIndex={-1}>
          Full history
        </h2>
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

      <div
        className="sticky top-0 z-10 -mx-1 flex flex-wrap items-center gap-2 border-border border-b bg-background/95 px-1 py-2 backdrop-blur sm:gap-3"
        data-testid="history-toolbar"
        ref={setBar}
      >
        <Input
          className="min-w-40 flex-1 sm:max-w-xs"
          data-testid="history-search"
          onChange={(e) => prefs.setQuery(e.target.value)}
          placeholder="Search history…"
          value={query}
        />
        <Select onValueChange={prefs.setKind} value={kind}>
          <SelectTrigger
            className="w-36 sm:w-40"
            data-testid="history-kind-filter"
          >
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
        <span className="hidden text-muted-foreground text-xs sm:inline">
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
            aria-label={expandLabel}
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
            <span className="hidden sm:inline">{expandLabel}</span>
          </Button>
        ) : null}
        <RedactionOffBadge redacted={redacted} />
        <Button
          aria-label="Back to summary"
          data-testid="history-to-summary"
          onClick={onSummary}
          size="icon-sm"
          title="Back to summary"
          variant="ghost"
        >
          <ArrowUpIcon className="size-3.5" />
        </Button>
      </div>

      <HiddenNote
        hidden={callHidden ? "call" : jumpHidden && "jump"}
        onClear={showCall}
      />

      <div
        aria-busy={stale}
        className={cn("transition-opacity", stale && "opacity-70")}
      >
        {mode === "chat" ? (
          <TranscriptChat
            a={a}
            activeMark={activeMark}
            allOpen={allOpen}
            bulkKey={bulkKey}
            callId={callId}
            chipDone={chipDone}
            crossEvtIdx={crossEvtIdx}
            isOpen={isOpen}
            jump={jump}
            jumpDone={jumpDone}
            marks={marks}
            onToggle={onToggle}
            queryActive={deferredQuery.length > 0}
            rows={visible}
            stickyHeight={stickyHeight}
            tools={tools}
            turns={turns}
          />
        ) : (
          <TranscriptTable
            a={a}
            bulkKey={bulkKey}
            callId={callId}
            chipDone={chipDone}
            crossEvtIdx={crossEvtIdx}
            isOpen={isOpen}
            jump={jump}
            jumpDone={jumpDone}
            onSort={prefs.setSort}
            onToggle={onToggle}
            rows={visible}
            sort={sort}
            stickyHeight={stickyHeight}
            tools={tools}
            turns={turns}
          />
        )}
      </div>

      <Subagents a={a} isOpen={isOpen} onToggle={onToggle} parent={sid} />
    </section>
  );
};
