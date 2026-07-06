/** Session browser (Phase 8.1) — filter + search over `sessions.list`.
 *
 * Headers are the lightweight `SessionHeader` rows (no body parse). Filters span
 * project / gitBranch / model / start-date; free-text search matches the title
 * (and id). Rendered as a compact vertical list so it can live in the
 * master-detail left rail; the active row is highlighted via `selectedId`.
 */
import type { SessionHeader } from "@workspace/core/services/sessions/schema";
import { Badge } from "@workspace/ui/components/badge";
import { Input } from "@workspace/ui/components/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@workspace/ui/components/select";
import { cn } from "@workspace/ui/lib/utils";
import { useMemo, useState } from "react";
import { fmtBytes } from "../../lib/session-format";

/** Characters of the session id shown as a short handle. */
const ID_PREFIX = 8;
/** Slice length for `YYYY-MM-DDTHH:MM` (date + minute). */
const TS_MINUTE = 16;

/** Distinct, sorted non-empty values of one header field. */
const distinct = (
  headers: readonly SessionHeader[],
  pick: (h: SessionHeader) => string | undefined
): readonly string[] =>
  [...new Set(headers.map(pick).filter((v): v is string => Boolean(v)))].sort();

/** `YYYY-MM-DD` of an ISO timestamp, or `""`. */
const dayOf = (iso: string | undefined): string =>
  iso ? (iso.slice(0, 10) ?? "") : "";

/** `YYYY-MM-DD HH:MM` of an ISO timestamp, or `"—"`. */
const fmtStarted = (iso: string | undefined): string =>
  iso ? iso.slice(0, TS_MINUTE).replace("T", " ") : "—";

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
      <SelectItem value="all">{label}: all</SelectItem>
      {options.map((o) => (
        <SelectItem key={o} value={o}>
          {o}
        </SelectItem>
      ))}
    </SelectContent>
  </Select>
);

/** Predicate: header passes every active filter + the title/id search. */
const matches = ({
  h,
  query,
  project,
  branch,
  model,
  day,
}: {
  readonly h: SessionHeader;
  readonly query: string;
  readonly project: string;
  readonly branch: string;
  readonly model: string;
  readonly day: string;
}): boolean => {
  if (project !== "all" && h.project !== project) {
    return false;
  }
  if (branch !== "all" && h.gitBranch !== branch) {
    return false;
  }
  if (model !== "all" && h.model !== model) {
    return false;
  }
  if (day !== "all" && dayOf(h.startedAt) !== day) {
    return false;
  }
  if (query.length > 0) {
    const hay = `${h.title ?? ""} ${h.id}`.toLowerCase();
    return hay.includes(query.toLowerCase());
  }
  return true;
};

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
  <button
    aria-current={active ? "true" : undefined}
    className={cn(
      "flex w-full flex-col gap-1.5 rounded-lg border px-3 py-2.5 text-left transition-colors",
      active
        ? "border-primary/40 bg-primary/10"
        : "border-border hover:bg-muted"
    )}
    data-testid="session-row"
    onClick={() => onOpen(h.id)}
    type="button"
  >
    <div className="flex items-baseline justify-between gap-2">
      <span className="truncate font-medium text-sm">
        {h.title ?? "Untitled session"}
      </span>
      <span className="shrink-0 text-muted-foreground text-xs tabular-nums">
        {h.messageCount} msgs
      </span>
    </div>
    <div className="flex items-center gap-2 text-muted-foreground text-xs">
      <span className="truncate">{h.project}</span>
      {h.gitBranch ? (
        <Badge className="shrink-0" variant="secondary">
          {h.gitBranch}
        </Badge>
      ) : null}
    </div>
    <div className="flex items-center justify-between gap-2 text-muted-foreground text-xs">
      <span className="truncate font-mono">
        {h.model ?? h.id.slice(0, ID_PREFIX)}
      </span>
      <span className="shrink-0 whitespace-nowrap tabular-nums">
        {fmtStarted(h.startedAt)} · {fmtBytes(h.sizeBytes)}
      </span>
    </div>
  </button>
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
  const [query, setQuery] = useState("");
  const [project, setProject] = useState("all");
  const [branch, setBranch] = useState("all");
  const [model, setModel] = useState("all");
  const [day, setDay] = useState("all");

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
        .filter((h) => matches({ h, query, project, branch, model, day }))
        .sort((a, b) => (b.startedAt ?? "").localeCompare(a.startedAt ?? "")),
    [headers, query, project, branch, model, day]
  );

  return (
    <div className="flex flex-col gap-3" data-testid="session-list">
      <Input
        className="w-full"
        data-testid="session-search"
        onChange={(e) => setQuery(e.target.value)}
        placeholder="Search by title…"
        value={query}
      />
      <div className="grid grid-cols-2 gap-2">
        <FilterSelect
          label="Project"
          onChange={setProject}
          options={projects}
          testId="session-filter-project"
          value={project}
        />
        <FilterSelect
          label="Branch"
          onChange={setBranch}
          options={branches}
          testId="session-filter-branch"
          value={branch}
        />
        <FilterSelect
          label="Model"
          onChange={setModel}
          options={models}
          testId="session-filter-model"
          value={model}
        />
        <FilterSelect
          label="Date"
          onChange={setDay}
          options={days}
          testId="session-filter-date"
          value={day}
        />
      </div>

      <div className="px-1 text-muted-foreground text-xs">
        {rows.length} {rows.length === 1 ? "session" : "sessions"}
      </div>

      <div className="flex flex-col gap-2">
        {rows.map((h) => (
          <SessionRow
            active={h.id === selectedId}
            h={h}
            key={h.id}
            onOpen={onOpen}
          />
        ))}
        {rows.length === 0 ? (
          <p
            className="py-8 text-center text-muted-foreground text-sm"
            data-testid="session-no-matches"
          >
            No sessions match the current filters.
          </p>
        ) : null}
      </div>
    </div>
  );
};
