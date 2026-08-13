/** A ranked list of rows, each with a proportional bar.
 *
 * The ranking is the whole opinion: rows arrive in the order their producer
 * ranked them and this draws them in that order, never re-sorting. The bar is
 * proportional to `max` — a share of the corpus, not a budget — so it carries no
 * colour banding: nothing here is "over" anything.
 *
 * Generic over its rows on purpose. The stats tables rank failures by sessions
 * spanned and time by seconds, which do not compare, so the caller formats its
 * own value and states its own unit in the header.
 */
import { cn } from "@workspace/ui/lib/utils";

/** Full-scale percentage. */
const FULL_PCT = 100;

/** Bar width below which a non-zero row would render as nothing at all. */
const MIN_VISIBLE_PCT = 0.5;

/** One ranked row. `value` drives the bar; `secondary` is a right-aligned cell. */
export interface RankRow {
  readonly key: string;
  readonly label: string;
  /** One line under the label: what to do about it, or how it was measured. */
  readonly note?: string;
  readonly secondary?: string;
  readonly value: number;
  /** Rendered instead of `value`, when the number has a unit or a format. */
  readonly valueText?: string;
}

const widthOf = (value: number, max: number) => {
  if (max <= 0) {
    return 0;
  }
  return Math.max(MIN_VISIBLE_PCT, (value / max) * FULL_PCT);
};

/** One row: label, bar, and the value it was ranked on. */
const Row = ({
  row,
  max,
  onSelect,
  selected,
}: {
  readonly max: number;
  readonly onSelect?: (key: string) => void;
  readonly row: RankRow;
  readonly selected: boolean;
}) => {
  const body = (
    <>
      <div className="flex items-baseline justify-between gap-3 text-xs">
        <span className="truncate font-medium">{row.label}</span>
        <span className="shrink-0 font-mono text-muted-foreground">
          {row.valueText ?? row.value.toLocaleString()}
          {row.secondary ? (
            <span className="ml-2 text-muted-foreground/70">
              {row.secondary}
            </span>
          ) : null}
        </span>
      </div>
      <div className="h-1.5 w-full overflow-hidden rounded-full bg-muted">
        <div
          className="h-full rounded-full bg-sky-500/70"
          style={{ width: `${widthOf(row.value, max)}%` }}
        />
      </div>
      {row.note ? (
        <p className="text-muted-foreground text-xs">{row.note}</p>
      ) : null}
    </>
  );
  if (!onSelect) {
    return (
      <div className="flex flex-col gap-1" data-testid="rank-row">
        {body}
      </div>
    );
  }
  return (
    <button
      className={cn(
        "flex w-full flex-col gap-1 rounded-md px-2 py-1 text-left hover:bg-muted/50",
        selected && "bg-muted/60"
      )}
      data-testid="rank-row"
      onClick={() => onSelect(row.key)}
      type="button"
    >
      {body}
    </button>
  );
};

/** Ranked rows with proportional bars, drawn in the order they arrive. */
export const RankBar = ({
  rows,
  max,
  onSelect,
  selectedKey,
}: {
  /** Scale of a full-width bar. Usually the top row's value. */
  readonly max: number;
  readonly onSelect?: (key: string) => void;
  readonly rows: readonly RankRow[];
  readonly selectedKey?: string | null;
}) => {
  if (rows.length === 0) {
    return (
      <p className="text-muted-foreground text-sm" data-testid="rank-empty">
        Nothing measured here.
      </p>
    );
  }
  return (
    <div className="flex flex-col gap-2" data-testid="rank-bar">
      {rows.map((row) => (
        <Row
          key={row.key}
          max={max}
          row={row}
          selected={selectedKey === row.key}
          {...(onSelect ? { onSelect } : {})}
        />
      ))}
    </div>
  );
};
