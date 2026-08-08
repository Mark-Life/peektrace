"use client";

/** SPIKE — TanStack Charts port of `type-donut.tsx`.
 *
 * `type-donut.tsx` is the only component in the repo that actually renders with
 * Recharts, so this is the port the adopt/wait decision turns on. It replaces
 * the shadcn `ChartContainer` + `PieChart`/`Pie`/`Cell` tree with the opt-in
 * polar entry: `pie` allocates the angular intervals eagerly, `radialArc`
 * renders them.
 *
 * The colours stay on the shared `--chart-N` tokens, so light/dark still comes
 * from `@workspace/ui`'s globals rather than from the chart library.
 *
 * Nothing in the product imports this. `type-donut.tsx` is still the shipping
 * component.
 */

import { defineChart } from "@tanstack/charts";
import { pie, polar, radialArc } from "@tanstack/charts/polar";
import { Chart } from "@tanstack/react-charts";
import { useMemo } from "react";

/** Stable colour per memory type; core emits `unknown` for untyped entries. */
const TYPE_COLORS: Record<string, string> = {
  user: "var(--chart-1)",
  feedback: "var(--chart-2)",
  project: "var(--chart-3)",
  reference: "var(--chart-4)",
  unknown: "var(--chart-5)",
};

/** Rendered size, matching the original's 140px square. */
const SIZE = 140;
/** Inner radius as a fraction of the outer, giving the original's 40/70 ring. */
const INNER_RATIO = 0.57;

/** Render the type-count donut. `typeCounts` is a `{ type: count }` record. */
export const TypeDonutTanstack = ({
  typeCounts,
}: {
  readonly typeCounts: Readonly<Record<string, number>>;
}) => {
  const data = useMemo(
    () =>
      Object.entries(typeCounts)
        .filter(([, n]) => n > 0)
        .map(([type, count]) => ({
          type,
          count,
          fill: TYPE_COLORS[type] ?? "var(--chart-5)",
        })),
    [typeCounts]
  );

  const definition = useMemo(
    () =>
      defineChart({
        marks: [
          polar({
            radiusRatio: 1,
            marks: [
              radialArc(pie(data, { value: "count" }), {
                innerRadius: ({ radius }) => radius * INNER_RATIO,
                color: "type",
                key: "type",
              }),
            ],
          }),
        ],
        color: {
          domain: data.map((d) => d.type),
          range: data.map((d) => d.fill),
        },
      }),
    [data]
  );

  if (data.length === 0) {
    return (
      <p className="text-muted-foreground text-sm" data-testid="donut-empty">
        No typed memories yet.
      </p>
    );
  }

  return (
    <div className="flex items-center gap-4" data-testid="type-donut-tanstack">
      <Chart
        ariaLabel="Memory type distribution"
        definition={definition}
        height={SIZE}
        initialWidth={SIZE}
        width={SIZE}
      />
      <ul className="flex flex-col gap-1 text-xs">
        {data.map((d) => (
          <li className="flex items-center gap-2" key={d.type}>
            <span
              className="inline-block size-2.5 rounded-sm"
              style={{ background: d.fill }}
            />
            <span className="capitalize">{d.type}</span>
            <span className="font-mono text-muted-foreground">{d.count}</span>
          </li>
        ))}
      </ul>
    </div>
  );
};
