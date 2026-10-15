/** Sort controls above the transcript table, one per sortable column.
 *
 * Rows are disclosure widgets, not table cells, so the header is a strip of
 * toggle buttons rather than `<th>`s; each button names its column and the
 * direction it is sorted in, and lines up with the row field it sorts.
 */
import { cn } from "@workspace/ui/lib/utils";
import { ArrowDownIcon, ArrowUpDownIcon, ArrowUpIcon } from "lucide-react";
import type { HistorySort, HistorySortKey } from "../../lib/history-sort";

const COLUMNS: readonly {
  readonly key: HistorySortKey;
  readonly label: string;
  readonly className?: string;
}[] = [
  { key: "order", label: "Turn" },
  { key: "type", label: "Type" },
  { key: "tokens", label: "Tokens", className: "ml-auto" },
];

const DIR_WORD = { asc: "ascending", desc: "descending" } as const;

const SortIcon = ({ dir }: { readonly dir: HistorySort["dir"] | null }) => {
  if (dir === "asc") {
    return <ArrowUpIcon className="size-3" />;
  }
  if (dir === "desc") {
    return <ArrowDownIcon className="size-3" />;
  }
  return <ArrowUpDownIcon className="size-3 opacity-40" />;
};

export const SortHeader = ({
  onSort,
  sort,
}: {
  readonly onSort: (key: HistorySortKey) => void;
  readonly sort: HistorySort;
}) => (
  <div
    className="flex items-center gap-2 border-border border-b bg-muted/30 px-2 py-1 text-muted-foreground text-xs"
    data-testid="history-sort-header"
  >
    {/* Lines the first label up with the row chevron's column. */}
    <span aria-hidden="true" className="size-3.5 shrink-0" />
    {COLUMNS.map(({ key, label, className }) => {
      const active = sort.key === key;
      return (
        <button
          aria-label={
            active
              ? `Sort by ${label.toLowerCase()}, ${DIR_WORD[sort.dir]}`
              : `Sort by ${label.toLowerCase()}`
          }
          aria-pressed={active}
          className={cn(
            "inline-flex items-center gap-1 rounded-sm px-1 hover:text-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring",
            active && "font-medium text-foreground",
            className
          )}
          data-dir={active ? sort.dir : undefined}
          data-testid={`history-sort-${key}`}
          key={key}
          onClick={() => onSort(key)}
          type="button"
        >
          {label}
          <SortIcon dir={active ? sort.dir : null} />
        </button>
      );
    })}
  </div>
);
