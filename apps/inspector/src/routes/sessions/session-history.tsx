/** Full history: one filtered event list, two readings of it.
 *
 * The shell owns everything shared — search, the type filter, the redaction
 * toggle and its banner, expand-all, and the single `visible` array — then
 * hands the identical rows to either renderer. Layout, sort and the type
 * filter come from the history prefs shared across sessions; the search query
 * belongs to this view and clears when another session opens. The chat layout
 * is the default, the readable pass; the dense table is the forensic answer
 * ("what did this cost, in what order") over the same events. Keeping the
 * filtering here is what stops the two from drifting apart. Rows come from the
 * body-free history index; each drawn row's event page is fetched as it comes
 * on screen, and search runs on the server. The transcript is
 * REDACTED BY DEFAULT behind a persistent "review before sharing" banner; the
 * reveal toggle re-fetches with `redact:false` (handled by the parent atom).
 */
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
  ArrowUpIcon,
  ChevronsDownUpIcon,
  ChevronsUpDownIcon,
  MessagesSquareIcon,
  ShieldAlertIcon,
  TableIcon,
} from "lucide-react";
import {
  type RefObject,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import type {
  GrowthSeries,
  HistoryIndex,
  SessionSummary,
} from "../../lib/api-types.gen";
import { chatIdsOf } from "../../lib/chat-lane";
import {
  isHiddenJump,
  type JumpLanding,
  type JumpRequest,
  rowIndexOfTarget,
  toHistoryJump,
} from "../../lib/history-jump";
import {
  useScrollPadding,
  useStickyHeight,
  useSubagentsJump,
} from "../../lib/history-landing";
import { mergeRow, neededPages } from "../../lib/history-pages";
import { useHistoryPrefs } from "../../lib/history-prefs";
import { visibleRows } from "../../lib/history-search";
import { DEFAULT_HISTORY_SORT, isForwardOrder } from "../../lib/history-sort";
import { setHashParam, useHashParam } from "../../lib/routes";
import { useScrollRoot } from "../../lib/scroll-root";
import {
  type SessionKey,
  useBodyPages,
  useHistorySearch,
} from "../../lib/session-data";
import { useToolMarks } from "../../lib/session-markers";
import {
  eventCollapseId,
  type SessionView,
  subagentCollapseId,
} from "../../lib/session-view";
import { indexTools } from "../../lib/tool-event";
import {
  type RevealHit,
  useSearchQuery,
  useSearchStepper,
} from "../../lib/use-history-search";
import { HiddenNote } from "./hidden-note";
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

const NO_HITS: readonly string[] = [];
const NO_PAGES: readonly number[] = [];

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
  growth,
  index,
  k,
  landing,
  onStale,
  onSummary,
  req,
  s: a,
  sectionRef,
  seq,
  sid,
  view,
}: {
  /** Per-turn usage for the chat turn rules; null until it arrives. */
  readonly growth: GrowthSeries | null;
  readonly index: HistoryIndex;
  readonly k: SessionKey;
  readonly landing: JumpLanding;
  /** Asks for a new summary: a page came back newer than it. */
  readonly onStale: () => void;
  /** Scrolls back up to the summary. */
  readonly onSummary: () => void;
  /** Where a stats link or the summary asked to go, if anywhere. */
  readonly req: JumpRequest | null;
  /** The section element, for jumps from outside it. */
  readonly sectionRef: RefObject<HTMLElement | null>;
  /** Bumped on every request, so a second click on one chip lands again. */
  readonly seq: number;
  readonly s: SessionSummary;
  /** The session id, for the marker query — the summary carries none. */
  readonly sid: string;
  readonly view: SessionView;
}) => {
  const { redacted } = view.state;
  const prefs = useHistoryPrefs();
  const { kind, sort, mode } = prefs;
  const { query, deferredQuery, setQuery } = useSearchQuery(sid);
  const events = index.rows;
  const found = useHistorySearch({
    k,
    query: deferredQuery,
    rev: a.history.rev,
  });
  // The query the shown rows answer: it trails the typed one while the
  // server searches.
  const shownQuery = found.query;
  const searching = shownQuery.length > 0;
  const turns = useMemo(() => events.map((e) => e.turn), [events]);
  // Resolved once per index and request: the resolve walks every event,
  // and a fresh object would re-run the landing effects on each render.
  const jump = useMemo(
    () =>
      toHistoryJump(
        {
          compactionTurns: a.compactionTurns,
          events,
          subagents: a.subagents,
          turnCount: a.turnCount,
        },
        req,
        seq
      ),
    [a.compactionTurns, a.subagents, a.turnCount, events, req, seq]
  );
  const tools = useMemo(() => indexTools({ events }), [events]);
  const [needed, setNeeded] = useState(NO_PAGES);
  const total = a.history.total;
  const onDrawn = useCallback(
    (drawn: readonly number[]) => {
      const next = neededPages({ drawn, total });
      setNeeded((prev) =>
        prev.length === next.length && prev.every((p, i) => p === next[i])
          ? prev
          : next
      );
    },
    [total]
  );
  const pages = useBodyPages({
    hashes: a.history.pages,
    k,
    needed,
    onStale,
  });
  const eventAt = useCallback((pos: number) => mergeRow(pos, pages), [pages]);
  const chatSession = useMemo(
    () => ({
      contextWindow: a.contextWindow,
      dumbZoneCrossTurn: a.dumbZoneCrossTurn,
      dumbZoneFraction: a.dumbZoneFraction,
      turns: growth?.turns ?? [],
    }),
    [a.contextWindow, a.dumbZoneCrossTurn, a.dumbZoneFraction, growth]
  );
  // Stats marks are an annotation on the same calls the tables counted; a
  // failed or pending marker query leaves the transcript exactly as it was.
  const marks = useToolMarks(sid);
  const activeMark = useHashParam("mark");
  const [bar, setBar] = useState<HTMLDivElement | null>(null);
  const stickyHeight = useStickyHeight(bar);
  useScrollPadding(useScrollRoot(), stickyHeight);
  useSubagentsJump({
    done: landing.done,
    jump,
    section: sectionRef,
  });

  // A conversation has one order — its own. The persisted sort is left alone, so
  // switching back to the table restores the column sort it had.
  const effectiveSort = mode === "chat" ? DEFAULT_HISTORY_SORT : sort;

  const crossEvtIdx = index.crossEvent;

  const visible = useMemo(
    () =>
      visibleRows({
        events,
        hits: found.hits,
        kind,
        query: shownQuery,
        sort: effectiveSort,
      }),
    [events, found.hits, kind, shownQuery, effectiveSort]
  );
  // The input follows every keystroke; the transcript follows once the
  // server has answered, so typing never waits on a search.
  const stale = query !== deferredQuery || found.pending;

  // A jump the search or type filter hides gets a note, rather than leave the
  // reader at the top of the transcript.
  const jumpIndex = useMemo(
    () =>
      jump === null
        ? -1
        : rowIndexOfTarget({
            ordered: isForwardOrder(effectiveSort),
            rows: visible,
            target: jump.target,
            turns,
          }),
    [jump, visible, effectiveSort, turns]
  );
  const jumpHidden = isHiddenJump({
    filtered: searching || kind !== "all",
    index: jumpIndex,
    jump,
    landed: landing.done.current,
  });
  const clearFilters = () => {
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

  // The search filters the list, so every drawn card is a hit. The mounted
  // layout fills `revealHit`, since only it knows where a hit's row sits.
  const revealHit = useRef<RevealHit>(null);
  const search = useSearchStepper({
    hits: searching ? transcriptIds : NO_HITS,
    isOpen,
    onToggle,
    query: shownQuery,
    resetKey: query,
    reveal: revealHit,
  });

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
      className="flex flex-col gap-3"
      data-testid="session-history"
      ref={sectionRef}
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
          <h2 className="font-semibold text-base outline-none" tabIndex={-1}>
            Full history
          </h2>
          <span className="text-muted-foreground text-xs">
            {visible.length} events · {a.dumbZoneTurns}/{a.turnCount} turns in
            dumb zone
          </span>
        </div>
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
        className="@container top-0 z-20 -mx-1 flex flex-wrap items-center gap-2 border-border border-b bg-background/95 px-1 py-2 backdrop-blur [@media(min-height:500px)]:sticky"
        data-testid="history-toolbar"
        ref={setBar}
      >
        <HistorySearch
          count={search.count}
          current={search.current}
          onClear={() => setQuery("")}
          onQuery={setQuery}
          onStep={search.onStep}
          pending={stale}
          query={query}
        />
        <Select onValueChange={prefs.setKind} value={kind}>
          <SelectTrigger
            className="@2xl:w-40 w-36"
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
            <span className="@2xl:inline hidden">{expandLabel}</span>
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
        hidden={jumpHidden && (req?.kind === "call" ? "call" : "jump")}
        onClear={clearFilters}
      />

      <div
        aria-busy={stale}
        className={cn("transition-opacity", stale && "opacity-70")}
        ref={search.containerRef}
      >
        {mode === "chat" ? (
          <TranscriptChat
            a={chatSession}
            activeMark={activeMark}
            allOpen={allOpen}
            bulkKey={bulkKey}
            crossEvtIdx={crossEvtIdx}
            eventAt={eventAt}
            isOpen={search.isOpen}
            jump={jump}
            landing={landing}
            marks={marks}
            onDrawn={onDrawn}
            onToggle={search.onToggle}
            queryActive={searching}
            revealHit={revealHit}
            rows={visible}
            stickyHeight={stickyHeight}
            tools={tools}
            turns={turns}
          />
        ) : (
          <TranscriptTable
            a={a}
            bulkKey={bulkKey}
            crossEvtIdx={crossEvtIdx}
            eventAt={eventAt}
            isOpen={search.isOpen}
            jump={jump}
            jumpIndex={jumpIndex}
            landing={landing}
            onDrawn={onDrawn}
            onSort={prefs.setSort}
            onToggle={search.onToggle}
            revealHit={revealHit}
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
