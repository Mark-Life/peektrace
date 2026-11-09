/** Session browser: filter + search over `sessions.list`.
 *
 * Headers are the lightweight `SessionHeader` rows (no body parse). Filters span
 * agent / project / gitBranch / model / activity-date range; free-text search matches
 * the title (and id). Rows are newest activity first, two lines each, and only
 * the window in view is mounted. The active row is highlighted via `selectedId`.
 * When nothing matches, an `Empty` offers a one-click filter reset.
 */

import {
  defaultRangeExtractor,
  type Range,
  useVirtualizer,
} from "@tanstack/react-virtual";
import type { SessionHeader } from "@workspace/core/services/sessions/schema";
import { Badge } from "@workspace/ui/components/badge";
import { Button } from "@workspace/ui/components/button";
import {
  Empty,
  EmptyContent,
  EmptyDescription,
  EmptyHeader,
  EmptyMedia,
  EmptyTitle,
} from "@workspace/ui/components/empty";
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
} from "@workspace/ui/components/input-group";
import { Item } from "@workspace/ui/components/item";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@workspace/ui/components/select";
import { cn } from "@workspace/ui/lib/utils";
import { fmtBytes } from "@workspace/viz/lib/session-format";
import { FilterXIcon, SearchIcon, SearchXIcon, XIcon } from "lucide-react";
import type { RefObject } from "react";
import {
  memo,
  useCallback,
  useEffect,
  useEffectEvent,
  useId,
  useLayoutEffect,
  useMemo,
  useState,
} from "react";
import { AGENT_LABEL } from "../../lib/agent-label";
import {
  sessionHash,
  setHashParams,
  useListParams,
  useListQuery,
} from "../../lib/routes";
import { ariaKeys } from "../../lib/shortcuts";
import { DateRangeFilter } from "./date-range-filter";
import {
  ANY,
  activeCount,
  activeDays,
  distinct,
  type FilterOption,
  type Filters,
  filtersFromParams,
  filtersToParams,
  fmtStamp,
  hidingFilters,
  isLive,
  matches,
  NO_FILTERS,
  projectLabel,
  projectOptions,
  sortByRecent,
  stampTitle,
  titleOf,
  visibleBranch,
} from "./session-list-model";
import { useListRoving, useListSearch } from "./use-list-roving";

/** Per-harness chip tint, so a rail of mixed sessions is scannable at a glance.
 * Unknown agents fall through to the plain `outline` badge. */
const AGENT_BADGE: Record<string, string> = {
  claude:
    "border-orange-500/30 bg-orange-500/10 text-orange-700 dark:text-orange-300",
  codex: "border-blue-500/30 bg-blue-500/10 text-blue-700 dark:text-blue-300",
  pi: "border-cyan-500/25 bg-slate-500/10 text-cyan-700 dark:text-cyan-300",
  opencode:
    "border-violet-500/30 bg-violet-500/10 text-violet-700 dark:text-violet-300",
};

/** Fixed row height in rem, so rows grow with the browser font size; must
 * match `ROW_HEIGHT_CLASS`. */
const ROW_REM = 3.75;
const ROW_HEIGHT_CLASS = "h-[3.75rem]";
/** Root font size assumed on the server and when it cannot be read. */
const DEFAULT_ROOT_PX = 16;
/** Space between rows in px. */
const GAP_PX = 4;
/** Rows mounted beyond each edge of the viewport. */
const OVERSCAN = 8;
/** Viewport assumed before the first measure (and on the server). */
const INITIAL_RECT = { width: 320, height: 800 };
/** How often the live markers are re-evaluated. */
const NOW_TICK_MS = 30_000;

/** Root font size in px, read once per list mount. */
const rootFontPx = () => {
  if (typeof document === "undefined") {
    return DEFAULT_ROOT_PX;
  }
  const px = Number.parseFloat(
    getComputedStyle(document.documentElement).fontSize
  );
  return Number.isFinite(px) && px > 0 ? px : DEFAULT_ROOT_PX;
};

const toOptions = (values: readonly string[]): readonly FilterOption[] =>
  values.map((v) => ({ value: v, label: v }));

/** `options`, plus `value` itself when a shared link names one not loaded here,
 * so the trigger shows it instead of going blank. */
const withCurrent = (options: readonly FilterOption[], value: string) =>
  value === ANY || options.some((o) => o.value === value)
    ? options
    : [...options, { value, label: value }];

/** A labelled filter `Select` over `all` + the option values. */
const FilterSelect = ({
  label,
  value,
  options,
  onChange,
  testId,
}: {
  readonly label: string;
  readonly value: string;
  readonly options: readonly FilterOption[];
  readonly onChange: (v: string) => void;
  readonly testId: string;
}) => (
  <Select onValueChange={onChange} value={value}>
    <SelectTrigger className="w-full" data-testid={testId}>
      <SelectValue placeholder={label} />
    </SelectTrigger>
    <SelectContent>
      <SelectItem value={ANY}>{label}: all</SelectItem>
      {withCurrent(options, value).map((o) => (
        <SelectItem key={o.value} value={o.value}>
          {o.label}
        </SelectItem>
      ))}
    </SelectContent>
  </Select>
);

const ROW_KEYS = (["listDown", "listUp", "listFirst", "listLast"] as const)
  .map(ariaKeys)
  .join(" ");

/** One two-line session row; full values show as native tooltips on hover.
 *  Memoized: list reloads keep unchanged headers as the same objects, and
 *  moving the Tab stop re-renders only the two rows it touches. */
const SessionRow = memo(
  ({
    h,
    active,
    hintId,
    href,
    live,
    tabbable,
  }: {
    readonly h: SessionHeader;
    readonly active: boolean;
    /** Id of the hint that says how to move between rows. */
    readonly hintId: string;
    /** The session's hash, with the list filters kept. */
    readonly href: string;
    readonly live: boolean;
    /** The list's one Tab stop. */
    readonly tabbable: boolean;
  }) => {
    const title = titleOf(h);
    const stamp = h.updatedAt ?? h.startedAt;
    const branch = visibleBranch(h.gitBranch);
    return (
      // Layout classes go on `Item` so they merge over its defaults; the
      // `asChild` slot only concatenates the child's classes.
      <Item
        asChild
        className={cn(
          ROW_HEIGHT_CLASS,
          "flex-col flex-nowrap items-stretch justify-center gap-1 py-1.5 text-left",
          active
            ? "border-primary/40 bg-primary/10 [a]:hover:bg-primary/10"
            : "hover:bg-muted focus-visible:bg-muted"
        )}
        size="sm"
        variant="outline"
      >
        <a
          aria-current={active ? "page" : undefined}
          aria-describedby={hintId}
          aria-keyshortcuts={ROW_KEYS}
          data-session-id={h.id}
          data-testid="session-row"
          href={href}
          tabIndex={tabbable ? 0 : -1}
        >
          <span className="flex min-w-0 items-center gap-2">
            {live ? (
              <span
                aria-label="Live"
                className="size-2 shrink-0 rounded-full bg-emerald-500"
                data-testid="session-live"
                role="img"
                title="Live: wrote a line in the last 5 min"
              />
            ) : null}
            <span
              className="min-w-0 flex-1 truncate font-medium text-sm"
              title={title}
            >
              {title}
            </span>
            <span
              className="shrink-0 text-muted-foreground tabular-nums"
              title={stampTitle(h)}
            >
              {fmtStamp(stamp)}
            </span>
          </span>
          <span className="flex min-w-0 items-center gap-2 text-muted-foreground">
            <Badge className={cn(AGENT_BADGE[h.agent])} variant="outline">
              {AGENT_LABEL[h.agent] ?? h.agent}
            </Badge>
            {h.sourceLabel ? (
              <Badge variant="secondary">{h.sourceLabel}</Badge>
            ) : null}
            <span
              className="min-w-0 max-w-1/2 shrink-0 truncate"
              title={h.cwd ?? h.project}
            >
              {projectLabel(h)}
            </span>
            {branch ? (
              <span className="min-w-0 truncate font-mono" title={branch}>
                {branch}
              </span>
            ) : null}
            <span
              className="ml-auto shrink-0 whitespace-nowrap tabular-nums"
              title={`${fmtBytes(h.sizeBytes)} · ${h.model ?? h.id}`}
            >
              {h.messageCount} msgs
            </span>
          </span>
        </a>
      </Item>
    );
  }
);

/** Shown when every session is filtered out; the action clears all facets. */
const NoMatches = ({ onReset }: { readonly onReset: () => void }) => (
  <Empty className="border" data-testid="session-no-matches">
    <EmptyHeader>
      <EmptyMedia variant="icon">
        <SearchXIcon />
      </EmptyMedia>
      <EmptyTitle>No matching sessions</EmptyTitle>
      <EmptyDescription>
        No session matches the current search and filters.
      </EmptyDescription>
    </EmptyHeader>
    <EmptyContent>
      <Button
        data-testid="session-filters-reset"
        onClick={onReset}
        size="sm"
        variant="outline"
      >
        <FilterXIcon />
        Reset filters
      </Button>
    </EmptyContent>
  </Empty>
);

/** Current epoch ms, refreshed every `ms`. */
const useNow = (ms: number) => {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now()), ms);
    return () => clearInterval(id);
  }, [ms]);
  return now;
};

/** Nearest ancestor that scrolls vertically, else the document scroller. */
const scrollParentOf = (el: HTMLElement) => {
  for (let p = el.parentElement; p; p = p.parentElement) {
    const overflow = getComputedStyle(p).overflowY;
    if (overflow === "auto" || overflow === "scroll") {
      return p;
    }
  }
  return document.scrollingElement as HTMLElement | null;
};

/** The element that scrolls the list and the list's offset inside it. Which
 * ancestor scrolls depends on the breakpoint, and the filters above the list
 * set its offset, so both are re-read on resize of the window, the list's
 * wrapper or the page. A hidden list keeps its last values. */
const useScrollParent = (ref: RefObject<HTMLElement | null>) => {
  const [scroller, setScroller] = useState<HTMLElement | null>(null);
  const [margin, setMargin] = useState(0);
  const measure = useEffectEvent(() => {
    const el = ref.current;
    if (!el || el.offsetParent === null) {
      return;
    }
    const parent = scrollParentOf(el);
    setScroller(parent);
    if (parent) {
      const next =
        el.getBoundingClientRect().top -
        parent.getBoundingClientRect().top +
        parent.scrollTop;
      setMargin((prev) => (Math.abs(prev - next) < 1 ? prev : next));
    }
  });
  useLayoutEffect(() => {
    measure();
    const onResize = () => measure();
    window.addEventListener("resize", onResize);
    const observer = new ResizeObserver(onResize);
    const wrapper = ref.current?.parentElement;
    if (wrapper) {
      observer.observe(wrapper);
    }
    observer.observe(document.body);
    return () => {
      window.removeEventListener("resize", onResize);
      observer.disconnect();
    };
  }, [ref]);
  return { scroller, margin };
};

/** Only the rows in view (plus overscan), absolutely placed in a full-height
 *  track. The rows are one Tab stop, moved by the list keys; the stop row is
 *  always mounted so Tab can reach it. Owns the roving state, so a focus move
 *  re-renders only this and the rows it touches. */
const VirtualRows = ({
  rows,
  selectedId,
}: {
  readonly rows: readonly SessionHeader[];
  readonly selectedId: string | null;
}) => {
  const ids = useMemo(() => rows.map((h) => h.id), [rows]);
  const list = useListParams();
  const roving = useListRoving({ ids, selectedId });
  const hintId = useId();
  const ref = roving.group.ref;
  const { scroller, margin } = useScrollParent(ref);
  const now = useNow(NOW_TICK_MS);
  const [rowPx] = useState(() => ROW_REM * rootFontPx() + GAP_PX);
  const stopIndex = roving.stop === null ? -1 : ids.indexOf(roving.stop);
  const rangeExtractor = useCallback(
    (range: Range) => {
      const indexes = defaultRangeExtractor(range);
      return stopIndex < 0 || indexes.includes(stopIndex)
        ? indexes
        : [...indexes, stopIndex].sort((a, b) => a - b);
    },
    [stopIndex]
  );
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scroller,
    estimateSize: () => rowPx,
    getItemKey: (i) => rows[i]?.id ?? i,
    overscan: OVERSCAN,
    rangeExtractor,
    scrollMargin: margin,
    initialRect: INITIAL_RECT,
  });

  const revealSelected = useEffectEvent((id: string) => {
    if (ref.current?.offsetParent === null) {
      return;
    }
    const index = rows.findIndex((h) => h.id === id);
    if (index >= 0) {
      virtualizer.scrollToIndex(index, { align: "auto" });
    }
  });
  useEffect(() => {
    if (selectedId && scroller) {
      revealSelected(selectedId);
    }
  }, [selectedId, scroller]);

  return (
    <>
      <p className="sr-only" id={hintId}>
        Arrow keys or J and K move between sessions.
      </p>
      <ul
        aria-label="Sessions"
        className="relative w-full"
        style={{ height: virtualizer.getTotalSize() }}
        {...roving.group}
      >
        {virtualizer.getVirtualItems().map((v) => {
          const h = rows[v.index];
          return h ? (
            <li
              className="absolute inset-x-0 top-0"
              key={v.key}
              style={{ transform: `translateY(${v.start - margin}px)` }}
            >
              <SessionRow
                active={h.id === selectedId}
                h={h}
                hintId={hintId}
                href={sessionHash({ id: h.id, parent: null }, list)}
                live={isLive(h, now)}
                tabbable={h.id === roving.stop}
              />
            </li>
          ) : null;
        })}
      </ul>
    </>
  );
};

/** The list filters, read from and written to the hash so they survive
 * reload, Back and leaving the section. Writes replace the history entry and
 * touch only the keys being changed. */
const useListFilters = () => {
  const query = useListQuery();
  const filters = useMemo(
    () => filtersFromParams(new URLSearchParams(query)),
    [query]
  );
  const patch = (next: Partial<Filters>) =>
    setHashParams(filtersToParams(next));
  return [filters, patch] as const;
};

/** Wait after the last keystroke before writing the search to the hash:
 * browsers cap `history.replaceState` calls and WebKit throws past the cap. */
const SEARCH_WRITE_MS = 250;

/** The search box text: local while typing, written to the hash after a pause
 * (or right away via `flush`), and reset when the hash changes from outside. */
const useSearchDraft = (stored: string, write: (query: string) => void) => {
  const [draft, setDraft] = useState(stored);
  const [seen, setSeen] = useState(stored);
  if (stored !== seen) {
    setSeen(stored);
    setDraft(stored);
  }
  const onWrite = useEffectEvent(write);
  useEffect(() => {
    if (draft === stored) {
      return;
    }
    const id = setTimeout(() => onWrite(draft), SEARCH_WRITE_MS);
    return () => clearTimeout(id);
  }, [draft, stored]);
  const flush = (value: string) => {
    setDraft(value);
    if (value !== stored) {
      write(value);
    }
  };
  return [draft, setDraft, flush] as const;
};

/** The filter controls + a compact, virtualized session list; rows link to their session. */
const SessionListView = ({
  headers,
  selectedId,
}: {
  readonly headers: readonly SessionHeader[];
  readonly selectedId: string | null;
}) => {
  const [stored, patch] = useListFilters();
  const [draft, setDraft, flush] = useSearchDraft(stored.query, (query) =>
    patch({ query })
  );
  const filters = useMemo(() => ({ ...stored, query: draft }), [stored, draft]);
  const reset = () => {
    setDraft(NO_FILTERS.query);
    patch(NO_FILTERS);
  };
  const set =
    <K extends keyof Filters>(key: K) =>
    (v: Filters[K]) =>
      patch({ [key]: v });

  const agents = useMemo(
    () => toOptions(distinct(headers, (h) => h.agent)),
    [headers]
  );
  const projects = useMemo(() => projectOptions(headers), [headers]);
  const branches = useMemo(
    () => toOptions(distinct(headers, (h) => h.gitBranch)),
    [headers]
  );
  const models = useMemo(
    () => toOptions(distinct(headers, (h) => h.model)),
    [headers]
  );
  const sources = useMemo(
    () => toOptions(distinct(headers, (h) => h.sourceLabel)),
    [headers]
  );
  const days = useMemo(
    () => [...new Set(headers.flatMap(activeDays))].sort(),
    [headers]
  );

  const sorted = useMemo(() => sortByRecent(headers), [headers]);
  const rows = useMemo(
    () => sorted.filter((h) => matches(h, filters)),
    [sorted, filters]
  );

  const active = activeCount(filters);
  const search = useListSearch();

  // A session opened from outside the rail (stats, a shared link) may be
  // hidden by the filters; drop the ones that hide it, once per selection.
  // The key is null until the selected header has loaded.
  const selectedKey = headers.some((h) => h.id === selectedId)
    ? selectedId
    : null;
  const unhide = useEffectEvent((id: string) => {
    const h = headers.find((x) => x.id === id);
    const fix = h ? hidingFilters(h, filters) : {};
    if (Object.keys(fix).length === 0) {
      return;
    }
    if (fix.query !== undefined) {
      setDraft(fix.query);
    }
    patch(fix);
  });
  useEffect(() => {
    if (selectedKey) {
      unhide(selectedKey);
    }
  }, [selectedKey]);

  return (
    <div
      className="flex flex-col gap-3"
      data-testid="session-list"
      ref={search.root}
    >
      <InputGroup>
        <InputGroupAddon>
          <SearchIcon />
        </InputGroupAddon>
        <InputGroupInput
          aria-keyshortcuts={ariaKeys("search")}
          aria-label="Search sessions"
          data-testid="session-search"
          onBlur={() => flush(draft)}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              flush(draft);
              return;
            }
            search.onSearchKeyDown(e);
          }}
          placeholder="Search by title…"
          ref={search.search}
          value={filters.query}
        />
        {filters.query.length > 0 ? (
          <InputGroupAddon align="inline-end">
            <InputGroupButton
              aria-label="Clear search"
              data-testid="session-search-clear"
              onClick={() => flush("")}
              size="icon-xs"
            >
              <XIcon />
            </InputGroupButton>
          </InputGroupAddon>
        ) : null}
      </InputGroup>

      <div className="grid grid-cols-2 gap-2">
        <FilterSelect
          label="Agent"
          onChange={set("agent")}
          options={agents}
          testId="session-filter-agent"
          value={filters.agent}
        />
        <FilterSelect
          label="Project"
          onChange={set("project")}
          options={projects}
          testId="session-filter-project"
          value={filters.project}
        />
        <FilterSelect
          label="Branch"
          onChange={set("branch")}
          options={branches}
          testId="session-filter-branch"
          value={filters.branch}
        />
        <FilterSelect
          label="Model"
          onChange={set("model")}
          options={models}
          testId="session-filter-model"
          value={filters.model}
        />
        {sources.length > 1 ? (
          <FilterSelect
            label="Source"
            onChange={set("source")}
            options={sources}
            testId="session-filter-source"
            value={filters.source}
          />
        ) : null}
        <DateRangeFilter
          days={days}
          onChange={set("range")}
          range={filters.range}
        />
      </div>

      <div className="flex items-center justify-between gap-2 px-1 text-muted-foreground text-xs">
        <span>
          {rows.length} {rows.length === 1 ? "session" : "sessions"}
        </span>
        {active > 0 ? (
          <Button
            className="h-6 px-2 text-xs"
            data-testid="session-filters-clear"
            onClick={reset}
            size="sm"
            variant="ghost"
          >
            <FilterXIcon />
            Clear {active}
          </Button>
        ) : null}
      </div>

      {rows.length === 0 ? (
        <NoMatches onReset={reset} />
      ) : (
        <VirtualRows rows={rows} selectedId={selectedId} />
      )}
    </div>
  );
};

/** Memoized: an unchanged header list (same array) skips the whole rail. */
export const SessionList = memo(SessionListView);
