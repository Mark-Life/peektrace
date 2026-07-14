/** Session browser (Phase 8.1) — filter + search over `sessions.list`.
 *
 * Headers are the lightweight `SessionHeader` rows (no body parse). Filters span
 * agent / project / gitBranch / model / start-date range; free-text search matches
 * the title (and id). Rows are `Item`s in an `ItemGroup` so they can live in the
 * master-detail left rail; the active row is highlighted via `selectedId`. When
 * nothing matches, an `Empty` offers a one-click filter reset.
 */
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
import {
  Item,
  ItemActions,
  ItemContent,
  ItemDescription,
  ItemFooter,
  ItemGroup,
  ItemTitle,
} from "@workspace/ui/components/item";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@workspace/ui/components/select";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@workspace/ui/components/tooltip";
import { cn } from "@workspace/ui/lib/utils";
import { fmtBytes } from "@workspace/viz/lib/session-format";
import { FilterXIcon, SearchIcon, SearchXIcon, XIcon } from "lucide-react";
import type { ReactNode } from "react";
import { useMemo, useState } from "react";
import type { DateRange } from "react-day-picker";
import { DateRangeFilter, toDayKey } from "./date-range-filter";

/** Human-readable label per agent id for the badge. */
const AGENT_LABEL: Record<string, string> = {
  claude: "Claude",
  codex: "Codex",
  pi: "Pi",
  opencode: "OpenCode",
};

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

/** Characters of the session id shown as a short handle. */
const ID_PREFIX = 8;
/** Sentinel option meaning "this facet is not constraining the list". */
const ANY = "all";
/** Short month names, indexed by `MM - 1` from an ISO timestamp. */
const MONTHS = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
];

/** The five facets a header can be narrowed by, plus the free-text query. */
interface Filters {
  readonly agent: string;
  readonly branch: string;
  readonly model: string;
  readonly project: string;
  readonly query: string;
  readonly range: DateRange | undefined;
}

/** No facet selected, no query — the state the reset action restores. */
const NO_FILTERS: Filters = {
  query: "",
  agent: ANY,
  project: ANY,
  branch: ANY,
  model: ANY,
  range: undefined,
};

/** How many facets (query counts as one) currently constrain the list. */
const activeCount = (f: Filters): number =>
  (f.query.length > 0 ? 1 : 0) +
  (f.range?.from ? 1 : 0) +
  [f.agent, f.project, f.branch, f.model].filter((v) => v !== ANY).length;

/** Distinct, sorted non-empty values of one header field. */
const distinct = (
  headers: readonly SessionHeader[],
  pick: (h: SessionHeader) => string | undefined
): readonly string[] =>
  [...new Set(headers.map(pick).filter((v): v is string => Boolean(v)))].sort();

/** `YYYY-MM-DD` of an ISO timestamp, or `""`. */
const dayOf = (iso: string | undefined): string =>
  iso ? (iso.slice(0, 10) ?? "") : "";

/** Captures `YYYY`-`MM`-`DD`T`HH:MM` from the head of an ISO timestamp. */
const ISO_STAMP = /^\d{4}-(\d{2})-(\d{2})T(\d{2}:\d{2})/;

/** `Jul 9, 08:18` of an ISO timestamp, or `"—"`. Read off the string rather than
 * `Date`-parsed, so the rendered wall-clock matches what the transcript recorded. */
const fmtStarted = (iso: string | undefined): string => {
  const parts = iso?.match(ISO_STAMP);
  const month = parts ? MONTHS[Number(parts[1]) - 1] : undefined;
  if (!(parts && month)) {
    return "—";
  }
  return `${month} ${Number(parts[2])}, ${parts[3]}`;
};

/** A labelled filter `Select` over `all` + the distinct option values. */
const FilterSelect = ({
  label,
  value,
  options,
  onChange,
  testId,
}: {
  readonly label: string;
  readonly value: string;
  readonly options: readonly string[];
  readonly onChange: (v: string) => void;
  readonly testId: string;
}) => (
  <Select onValueChange={onChange} value={value}>
    <SelectTrigger className="w-full" data-testid={testId}>
      <SelectValue placeholder={label} />
    </SelectTrigger>
    <SelectContent>
      <SelectItem value={ANY}>{label}: all</SelectItem>
      {options.map((o) => (
        <SelectItem key={o} value={o}>
          {o}
        </SelectItem>
      ))}
    </SelectContent>
  </Select>
);

/** The row's headline: the transcript title, else the harness that produced it
 * (`Codex session`) — mirrors how the detail header names untitled sessions. */
const titleOf = (h: SessionHeader): string =>
  h.title || `${AGENT_LABEL[h.agent] ?? h.agent} session`;

/** Predicate: the session started on a day inside the picked range. A range with
 * no `to` yet (mid-drag, or a single click) matches that one day. */
const inRange = (h: SessionHeader, range: DateRange): boolean => {
  if (!range.from) {
    return true;
  }
  const day = dayOf(h.startedAt);
  if (!day) {
    return false;
  }
  const from = toDayKey(range.from);
  const to = range.to ? toDayKey(range.to) : from;
  return day >= from && day <= to;
};

/** Predicate: header passes every active filter + the title/id search. */
const matches = (h: SessionHeader, f: Filters): boolean => {
  if (f.agent !== ANY && h.agent !== f.agent) {
    return false;
  }
  if (f.project !== ANY && h.project !== f.project) {
    return false;
  }
  if (f.branch !== ANY && h.gitBranch !== f.branch) {
    return false;
  }
  if (f.model !== ANY && h.model !== f.model) {
    return false;
  }
  if (f.range && !inRange(h, f.range)) {
    return false;
  }
  if (f.query.length > 0) {
    const hay = `${titleOf(h)} ${h.id}`.toLowerCase();
    return hay.includes(f.query.toLowerCase());
  }
  return true;
};

/** A value the rail has to truncate; hovering (or focusing) reveals it in full. */
const Truncated = ({
  full,
  className,
  children,
}: {
  readonly full: string;
  readonly className?: string;
  readonly children: ReactNode;
}) => (
  <Tooltip>
    <TooltipTrigger asChild>
      <span className={cn("truncate", className)}>{children}</span>
    </TooltipTrigger>
    <TooltipContent className="max-w-xs break-all">{full}</TooltipContent>
  </Tooltip>
);

/** One session row in the rail; highlighted when it is the active selection. */
const SessionRow = ({
  h,
  active,
  onOpen,
}: {
  readonly h: SessionHeader;
  readonly active: boolean;
  readonly onOpen: (id: string) => void;
}) => (
  <Item asChild size="sm" variant="outline">
    <button
      aria-current={active ? "true" : undefined}
      className={cn(
        "text-left",
        active
          ? "border-primary/40 bg-primary/10"
          : "hover:bg-muted focus-visible:bg-muted"
      )}
      data-testid="session-row"
      onClick={() => onOpen(h.id)}
      type="button"
    >
      <ItemContent className="min-w-0">
        <ItemTitle className="line-clamp-2 block w-full min-w-0 text-sm">
          {titleOf(h)}
        </ItemTitle>
        <ItemDescription className="flex min-w-0 items-center gap-2">
          <Badge
            className={cn("shrink-0", AGENT_BADGE[h.agent])}
            variant="outline"
          >
            {AGENT_LABEL[h.agent] ?? h.agent}
          </Badge>
          <Truncated full={h.project}>{h.project}</Truncated>
        </ItemDescription>
        {h.gitBranch ? (
          <div className="flex min-w-0">
            <Badge className="max-w-full" variant="secondary">
              <Truncated full={h.gitBranch}>{h.gitBranch}</Truncated>
            </Badge>
          </div>
        ) : null}
      </ItemContent>
      <ItemActions className="shrink-0 self-start whitespace-nowrap text-muted-foreground tabular-nums">
        {h.messageCount} msgs
      </ItemActions>
      <ItemFooter className="min-w-0 gap-2 text-muted-foreground">
        <Truncated className="min-w-0 font-mono" full={h.model ?? h.id}>
          {h.model ?? h.id.slice(0, ID_PREFIX)}
        </Truncated>
        <span className="shrink-0 whitespace-nowrap tabular-nums">
          <Truncated full={h.startedAt ?? "unknown start time"}>
            {fmtStarted(h.startedAt)}
          </Truncated>
          {" · "}
          {fmtBytes(h.sizeBytes)}
        </span>
      </ItemFooter>
    </button>
  </Item>
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

/** The filter controls + a compact, scrollable session list; rows call `onOpen`. */
export const SessionList = ({
  headers,
  selectedId,
  onOpen,
}: {
  readonly headers: readonly SessionHeader[];
  readonly selectedId: string | null;
  readonly onOpen: (id: string) => void;
}) => {
  const [filters, setFilters] = useState<Filters>(NO_FILTERS);
  const set =
    <K extends keyof Filters>(key: K) =>
    (v: Filters[K]) =>
      setFilters((prev) => ({ ...prev, [key]: v }));

  const agents = useMemo(() => distinct(headers, (h) => h.agent), [headers]);
  const projects = useMemo(
    () => distinct(headers, (h) => h.project),
    [headers]
  );
  const branches = useMemo(
    () => distinct(headers, (h) => h.gitBranch),
    [headers]
  );
  const models = useMemo(() => distinct(headers, (h) => h.model), [headers]);
  const days = useMemo(
    () => distinct(headers, (h) => dayOf(h.startedAt) || undefined),
    [headers]
  );

  const rows = useMemo(
    () =>
      [...headers]
        .filter((h) => matches(h, filters))
        .sort((a, b) => (b.startedAt ?? "").localeCompare(a.startedAt ?? "")),
    [headers, filters]
  );

  const active = activeCount(filters);

  return (
    <div className="flex flex-col gap-3" data-testid="session-list">
      <InputGroup>
        <InputGroupAddon>
          <SearchIcon />
        </InputGroupAddon>
        <InputGroupInput
          data-testid="session-search"
          onChange={(e) => setFilters((p) => ({ ...p, query: e.target.value }))}
          placeholder="Search by title…"
          value={filters.query}
        />
        {filters.query.length > 0 ? (
          <InputGroupAddon align="inline-end">
            <InputGroupButton
              aria-label="Clear search"
              data-testid="session-search-clear"
              onClick={() => setFilters((p) => ({ ...p, query: "" }))}
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
            onClick={() => setFilters(NO_FILTERS)}
            size="sm"
            variant="ghost"
          >
            <FilterXIcon />
            Clear {active}
          </Button>
        ) : null}
      </div>

      {rows.length === 0 ? (
        <NoMatches onReset={() => setFilters(NO_FILTERS)} />
      ) : (
        <ItemGroup className="gap-2">
          {rows.map((h) => (
            <SessionRow
              active={h.id === selectedId}
              h={h}
              key={h.id}
              onOpen={onOpen}
            />
          ))}
        </ItemGroup>
      )}
    </div>
  );
};
