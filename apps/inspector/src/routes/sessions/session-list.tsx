/** Session browser (Phase 8.1) — filter + search over `sessions.list`.
 *
 * Headers are the lightweight `SessionHeader` rows (no body parse). Filters span
 * project / gitBranch / model / start-date; free-text search matches the title
 * (and id). Clicking a row hands its id up to open the debug view.
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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@workspace/ui/components/table";
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
    <SelectTrigger className="w-44" data-testid={testId}>
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

/** The filter bar + sortable header table; rows call `onOpen(id)`. */
export const SessionList = ({
  headers,
  onOpen,
}: {
  readonly headers: readonly SessionHeader[];
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
    <div className="flex flex-col gap-4" data-testid="session-list">
      <div className="flex flex-wrap items-center gap-3">
        <Input
          className="max-w-xs"
          data-testid="session-search"
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search by title…"
          value={query}
        />
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

      <div className="rounded-lg border border-border">
        <Table>
          <TableHeader>
            <TableRow>
              <TableHead>Title</TableHead>
              <TableHead>Project</TableHead>
              <TableHead>Branch</TableHead>
              <TableHead>Model</TableHead>
              <TableHead className="text-right">Msgs</TableHead>
              <TableHead className="text-right">Size</TableHead>
              <TableHead>Started</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {rows.map((h) => (
              <TableRow
                className="cursor-pointer"
                data-testid="session-row"
                key={h.id}
                onClick={() => onOpen(h.id)}
              >
                <TableCell className="max-w-xs">
                  <div className="truncate font-medium text-sm">
                    {h.title ?? "Untitled session"}
                  </div>
                  <div className="truncate font-mono text-muted-foreground text-xs">
                    {h.id.slice(0, ID_PREFIX)}
                  </div>
                </TableCell>
                <TableCell className="text-sm">{h.project}</TableCell>
                <TableCell>
                  {h.gitBranch ? (
                    <Badge variant="secondary">{h.gitBranch}</Badge>
                  ) : (
                    <span className="text-muted-foreground">—</span>
                  )}
                </TableCell>
                <TableCell className="font-mono text-xs">
                  {h.model ?? "—"}
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {h.messageCount}
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {fmtBytes(h.sizeBytes)}
                </TableCell>
                <TableCell className="whitespace-nowrap text-muted-foreground text-xs">
                  {h.startedAt?.slice(0, TS_MINUTE).replace("T", " ") ?? "—"}
                </TableCell>
              </TableRow>
            ))}
            {rows.length === 0 ? (
              <TableRow>
                <TableCell
                  className="py-8 text-center text-muted-foreground text-sm"
                  colSpan={7}
                  data-testid="session-no-matches"
                >
                  No sessions match the current filters.
                </TableCell>
              </TableRow>
            ) : null}
          </TableBody>
        </Table>
      </div>
    </div>
  );
};
