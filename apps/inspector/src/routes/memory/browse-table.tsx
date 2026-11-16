/** Sortable / expandable memory browse table for one vault.
 *
 * Columns: title · type · description · size · modified · in-index · links.
 * Click a header to sort; click a row to expand its full body + clickable
 * `[[wikilinks]]` (a link click calls `onNavigate(targetSlug)` to jump to that
 * memory). Edit/delete affordances render only when `canWrite` (supported
 * capability, writable server).
 */
import { Badge } from "@workspace/ui/components/badge";
import { Button } from "@workspace/ui/components/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@workspace/ui/components/table";
import { cn } from "@workspace/ui/lib/utils";
import type { MemoryEntry } from "@workspace/view-core/api-types.gen";
import {
  ArrowDownIcon,
  ArrowUpIcon,
  CheckIcon,
  PencilIcon,
  Trash2Icon,
  XIcon,
} from "lucide-react";
import { Fragment, useMemo, useState } from "react";

import {
  bodyParts,
  fmtDate,
  fmtSize,
  type SortKey,
  sortEntries,
  titleOf,
} from "./memory-model";

/** A click handler set shared by every row. */
interface RowActions {
  readonly canWrite: boolean;
  readonly onDelete: (entry: MemoryEntry) => void;
  readonly onEdit: (entry: MemoryEntry) => void;
  readonly onNavigate: (slug: string) => void;
}

/** Render the body with `[[wikilinks]]` turned into navigation buttons. */
const LinkedBody = ({
  entry,
  onNavigate,
}: {
  readonly entry: MemoryEntry;
  readonly onNavigate: (slug: string) => void;
}) => {
  const parts = bodyParts(entry);
  return (
    <div className="font-mono text-xs leading-relaxed whitespace-pre-wrap">
      {parts.map((part, i) => {
        if ("link" in part) {
          return (
            <button
              className="text-sky-400 underline decoration-dotted hover:text-sky-300"
              // biome-ignore lint/suspicious/noArrayIndexKey: body is static per render
              key={i}
              onClick={() => onNavigate(part.slug)}
              type="button"
            >
              {part.link}
            </button>
          );
        }
        // biome-ignore lint/suspicious/noArrayIndexKey: body is static per render
        return <Fragment key={i}>{part.text}</Fragment>;
      })}
    </div>
  );
};

/** Expanded detail row: full metadata + linked body. */
const DetailRow = ({
  entry,
  actions,
}: {
  readonly entry: MemoryEntry;
  readonly actions: RowActions;
}) => (
  <TableRow className="bg-muted/30 hover:bg-muted/30">
    <TableCell colSpan={8}>
      <div className="flex flex-col gap-3 py-1" data-testid="memory-detail">
        <div className="flex items-center justify-between gap-2">
          <code className="text-muted-foreground text-xs">
            {entry.fileName}
          </code>
          {actions.canWrite ? (
            <div className="flex gap-2">
              <Button
                onClick={() => actions.onEdit(entry)}
                size="sm"
                variant="outline"
              >
                <PencilIcon className="size-3.5" /> Edit
              </Button>
              <Button
                onClick={() => actions.onDelete(entry)}
                size="sm"
                variant="outline"
              >
                <Trash2Icon className="size-3.5" /> Delete
              </Button>
            </div>
          ) : null}
        </div>
        {entry.links.length > 0 ? (
          <div className="flex flex-wrap items-center gap-1.5 text-xs">
            <span className="text-muted-foreground">Links:</span>
            {entry.links.map((l) => (
              <button
                className="border-border hover:bg-muted rounded border px-1.5 py-0.5 font-mono text-sky-400"
                key={`${l.targetSlug}:${l.line}`}
                onClick={() => actions.onNavigate(l.targetSlug)}
                type="button"
              >
                {l.rawTarget}
              </button>
            ))}
          </div>
        ) : null}
        <LinkedBody entry={entry} onNavigate={actions.onNavigate} />
      </div>
    </TableCell>
  </TableRow>
);

/** The active sort direction indicator. */
const SortArrow = ({ asc }: { readonly asc: boolean }) =>
  asc ? (
    <ArrowUpIcon className="size-3" />
  ) : (
    <ArrowDownIcon className="size-3" />
  );

/** A sortable column header button. */
const SortHead = ({
  label,
  col,
  active,
  asc,
  onSort,
  className,
}: {
  readonly label: string;
  readonly col: SortKey;
  readonly active: SortKey;
  readonly asc: boolean;
  readonly onSort: (k: SortKey) => void;
  readonly className?: string;
}) => (
  <TableHead className={className}>
    <button
      className="hover:text-foreground inline-flex items-center gap-1"
      onClick={() => onSort(col)}
      type="button"
    >
      {label}
      {active === col ? <SortArrow asc={asc} /> : null}
    </button>
  </TableHead>
);

/** The browse table. `highlightSlug` auto-expands a navigated-to entry. */
export const BrowseTable = ({
  entries,
  actions,
  highlightSlug,
  onExpandChange,
}: {
  readonly entries: readonly MemoryEntry[];
  readonly actions: RowActions;
  readonly highlightSlug?: string;
  readonly onExpandChange?: (slug: string | undefined) => void;
}) => {
  const [sort, setSort] = useState<SortKey>("mtime");
  const [asc, setAsc] = useState(false);
  const [open, setOpen] = useState<string | undefined>(undefined);
  const expanded = highlightSlug ?? open;

  const onSort = (k: SortKey) => {
    if (k === sort) {
      setAsc((v) => !v);
    } else {
      setSort(k);
      setAsc(true);
    }
  };

  const toggle = (slug: string) => {
    const next = expanded === slug ? undefined : slug;
    setOpen(next);
    onExpandChange?.(next);
  };

  const sorted = useMemo(
    () => sortEntries(entries, sort, asc),
    [entries, sort, asc]
  );

  if (entries.length === 0) {
    return (
      <p className="text-muted-foreground text-sm" data-testid="table-empty">
        No memories match the current filters.
      </p>
    );
  }

  return (
    <div className="border-border rounded-lg border" data-testid="browse-table">
      <Table>
        <TableHeader>
          <TableRow>
            <SortHead
              active={sort}
              asc={asc}
              col="title"
              label="Title"
              onSort={onSort}
            />
            <SortHead
              active={sort}
              asc={asc}
              col="type"
              label="Type"
              onSort={onSort}
            />
            <TableHead className="hidden md:table-cell">Description</TableHead>
            <SortHead
              active={sort}
              asc={asc}
              className="text-right"
              col="size"
              label="Size"
              onSort={onSort}
            />
            <SortHead
              active={sort}
              asc={asc}
              className="hidden lg:table-cell"
              col="mtime"
              label="Modified"
              onSort={onSort}
            />
            <SortHead
              active={sort}
              asc={asc}
              col="inIndex"
              label="In index"
              onSort={onSort}
            />
            <SortHead
              active={sort}
              asc={asc}
              className="text-right"
              col="links"
              label="Links"
              onSort={onSort}
            />
          </TableRow>
        </TableHeader>
        <TableBody>
          {sorted.map((entry) => (
            <Fragment key={entry.slug}>
              <TableRow
                className={cn(
                  "cursor-pointer",
                  expanded === entry.slug && "bg-muted/40"
                )}
                data-testid="memory-row"
                onClick={() => toggle(entry.slug)}
              >
                <TableCell className="font-medium">{titleOf(entry)}</TableCell>
                <TableCell>
                  {entry.type ? (
                    <Badge className="capitalize" variant="secondary">
                      {entry.type}
                    </Badge>
                  ) : (
                    <span className="text-muted-foreground text-xs">—</span>
                  )}
                </TableCell>
                <TableCell className="text-muted-foreground hidden max-w-[24ch] truncate text-sm md:table-cell">
                  {entry.description ?? "—"}
                </TableCell>
                <TableCell className="text-right font-mono text-xs">
                  {fmtSize(entry.size)}
                </TableCell>
                <TableCell className="text-muted-foreground hidden text-xs lg:table-cell">
                  {fmtDate(entry.mtime)}
                </TableCell>
                <TableCell>
                  {entry.inIndex ? (
                    <CheckIcon className="size-4 text-emerald-400" />
                  ) : (
                    <XIcon className="text-muted-foreground size-4" />
                  )}
                </TableCell>
                <TableCell className="text-right font-mono text-xs">
                  {entry.links.length}
                </TableCell>
              </TableRow>
              {expanded === entry.slug ? (
                <DetailRow actions={actions} entry={entry} />
              ) : null}
            </Fragment>
          ))}
        </TableBody>
      </Table>
    </div>
  );
};
