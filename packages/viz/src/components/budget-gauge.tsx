/** MEMORY.md index budget gauge — the headline forensic.
 *
 * The index is always loaded, but Claude only sees the first 200 lines / 25 KB;
 * anything past either cliff is below-fold and INVISIBLE TO CLAUDE. Two bars
 * (lines + bytes) show fill vs the cliff; a greyed "invisible" callout appears
 * whenever entries fall below the fold.
 */
import type { IndexBudget } from "@workspace/core/services/memory/types";
import { cn } from "@workspace/ui/lib/utils";
import { EyeOffIcon } from "lucide-react";

/** Full-scale percentage. */
const FULL_PCT = 100;
/** Fill ratio (%) above which the bar warns amber. */
const WARN_PCT = 80;

/** Pick the bar fill colour from its fill ratio + over-budget flag. */
const barColor = ({
  over,
  pct,
}: {
  readonly over: boolean;
  readonly pct: number;
}) => {
  if (over) {
    return "bg-red-500";
  }
  return pct > WARN_PCT ? "bg-amber-500" : "bg-emerald-500";
};

/** One labelled progress bar with an over-budget red state. */
const Bar = ({
  label,
  value,
  max,
  unit,
}: {
  readonly label: string;
  readonly value: number;
  readonly max: number;
  readonly unit: string;
}) => {
  const pct = Math.min(FULL_PCT, Math.round((value / max) * FULL_PCT));
  const over = value > max;
  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-baseline justify-between text-xs">
        <span className="text-muted-foreground">{label}</span>
        <span className={cn("font-mono", over && "font-semibold text-red-400")}>
          {value.toLocaleString()} / {max.toLocaleString()} {unit}
        </span>
      </div>
      <div className="h-2 w-full overflow-hidden rounded-full bg-muted">
        <div
          className={cn(
            "h-full rounded-full transition-all",
            barColor({ over, pct })
          )}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  );
};

/** Render the index budget for a vault. */
export const BudgetGauge = ({
  budget,
}: {
  readonly budget: typeof IndexBudget.Type;
}) => {
  if (budget.kind === "absent") {
    return (
      <p className="text-muted-foreground text-sm" data-testid="budget-absent">
        No MEMORY.md index in this vault.
      </p>
    );
  }
  return (
    <div className="flex flex-col gap-3" data-testid="budget-gauge">
      <Bar
        label="Index lines"
        max={budget.maxLines}
        unit="lines"
        value={budget.lines}
      />
      <Bar
        label="Index bytes"
        max={budget.maxBytes}
        unit="B"
        value={budget.bytes}
      />
      {budget.belowFoldCount > 0 ? (
        <div
          className="flex items-center gap-2 rounded-md border border-border bg-muted/50 px-3 py-2 text-muted-foreground text-xs"
          data-testid="below-fold"
        >
          <EyeOffIcon className="size-4 shrink-0" />
          <span>
            {budget.belowFoldCount} entr
            {budget.belowFoldCount === 1 ? "y is" : "ies are"} below the fold —{" "}
            <span className="font-semibold uppercase tracking-wide">
              invisible to Claude
            </span>
          </span>
        </div>
      ) : null}
    </div>
  );
};
