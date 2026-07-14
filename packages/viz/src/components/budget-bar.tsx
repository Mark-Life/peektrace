/** Budget-at-peak stacked bar + table.
 *
 * Partitions the peak-turn context into every `BudgetKey` slice — including the
 * recovered `thinking` band and the honest `unattributed` residual — as a single
 * stacked bar plus a per-category table (tokens, % window, % context). Slice
 * order/colors come from core `CAT_META`; hatched slices (system+tools,
 * unattributed) are inferred rather than measured.
 */
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@workspace/ui/components/table";
import { fmt, fmtK, PERCENT } from "../lib/session-format";

/** Hatched (inferred) slices get a diagonal overlay rather than a flat fill. */
const HATCHED = new Set(["system_tools", "unattributed"]);

/** Hide the slice label when its share is below this percent (too narrow). */
const LABEL_MIN_PCT = 6;

/** CSS background for a slice (hatch overlay for inferred categories). */
const sliceBg = (key: string, color: string): string =>
  HATCHED.has(key)
    ? `repeating-linear-gradient(45deg, rgba(0,0,0,0.35) 0 2px, transparent 2px 6px), ${color}`
    : color;

/** Stacked bar + category table for the peak-turn budget. */
export const BudgetBar = ({ a }: { readonly a: AnalyzedSession }) => {
  const total = a.budget.reduce((s, b) => s + b.tokens, 0) || 1;
  const free = Math.max(0, a.contextWindow - a.peakContextTokens);
  return (
    <section
      className="flex flex-col gap-3 rounded-lg border border-border p-4"
      data-testid="budget-bar"
    >
      <div>
        <h2 className="font-semibold text-base">Context budget at peak</h2>
        <p className="text-muted-foreground text-sm">
          Where the {fmt(a.peakContextTokens)}-token peak went. Thinking is
          recovered from <code>output_tokens</code>; hatched slices
          (system+tools, overhead) are inferred.
        </p>
      </div>

      <div className="flex h-7 w-full overflow-hidden rounded-md border border-border">
        {a.budget.map((s) => {
          const pct = (PERCENT * s.tokens) / total;
          return (
            <div
              className="flex items-center justify-center"
              key={s.key}
              style={{
                flex: s.tokens,
                background: sliceBg(s.key, s.color),
              }}
              title={`${s.label}: ${fmt(s.tokens)} (${pct.toFixed(1)}%)`}
            >
              {pct > LABEL_MIN_PCT ? (
                <span className="px-1 font-medium text-[10px] text-white/90">
                  {s.short}
                </span>
              ) : null}
            </div>
          );
        })}
      </div>
      <div className="flex justify-between text-muted-foreground text-xs">
        <span>0</span>
        <span>
          window {fmtK(a.contextWindow)} · free {fmt(free)}
        </span>
      </div>

      <Table data-testid="budget-table">
        <TableHeader>
          <TableRow>
            <TableHead>Category</TableHead>
            <TableHead className="text-right">Tokens</TableHead>
            <TableHead className="text-right">% window</TableHead>
            <TableHead className="text-right">% context</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {a.budget.map((s) => {
            const pctWin = (PERCENT * s.tokens) / a.contextWindow;
            const pctCtx = a.peakContextTokens
              ? (PERCENT * s.tokens) / a.peakContextTokens
              : 0;
            return (
              <TableRow data-testid={`budget-row-${s.key}`} key={s.key}>
                <TableCell>
                  <span className="inline-flex items-center gap-2">
                    <span
                      className="inline-block size-3 rounded-sm"
                      style={{ background: s.color }}
                    />
                    {s.label}
                    {s.estimated ? (
                      <span className="text-[10px] text-muted-foreground">
                        est
                      </span>
                    ) : null}
                  </span>
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {fmt(s.tokens)}
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {pctWin.toFixed(pctWin < 1 ? 2 : 1)}%
                </TableCell>
                <TableCell className="text-right tabular-nums">
                  {pctCtx.toFixed(1)}%
                </TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
    </section>
  );
};
