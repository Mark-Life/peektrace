"use client";

/** Memory-type distribution donut (TanStack Charts, polar entry).
 *
 * One slice per frontmatter `type` (user / feedback / project / reference /
 * untyped), sized by file count. A compact legend doubles as the count readout
 * so the donut stays useful even at small sizes.
 *
 * `pie` allocates the angular intervals eagerly and `radialArc` renders them.
 * Colours stay on the shared `--chart-N` tokens, so light/dark comes from
 * `@workspace/ui`'s globals rather than from the chart library.
 */

import { defineChart } from "@tanstack/charts";
import { pie, polar, radialArc } from "@tanstack/charts/polar";
import { tooltip } from "@tanstack/charts/tooltip";
import { Chart } from "@tanstack/react-charts/tooltip";
import { useMemo } from "react";

/** Stable colour per memory type; core emits `unknown` for untyped entries. */
const TYPE_COLORS: Record<string, string> = {
  user: "var(--chart-1)",
  feedback: "var(--chart-2)",
  project: "var(--chart-3)",
  reference: "var(--chart-4)",
  unknown: "var(--chart-5)",
};

/** Rendered size of the donut square, in pixels. */
const SIZE = 140;
/** Inner radius as a fraction of the outer, giving the 40/70 ring. */
const INNER_RATIO = 0.57;
/** Radians of empty space between slices. The `--chart-N` tokens are five
 * lightness steps of one hue, so adjacent slices need a seam to read apart. */
const GAP_ANGLE = 0.03;

/** One slice: a memory type, its file count and the token it paints with. */
interface Slice {
  readonly count: number;
  readonly fill: string;
  readonly type: string;
}

/** Render the type-count donut. `typeCounts` is a `{ type: count }` record. */
export const TypeDonut = ({
  typeCounts,
}: {
  readonly typeCounts: Readonly<Record<string, number>>;
}) => {
  const data = useMemo<readonly Slice[]>(
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
              radialArc(pie(data, { value: "count", gapAngle: GAP_ANGLE }), {
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
        tooltip: {
          use: tooltip,
          format: (point) => `${point.datum.type}: ${point.datum.count}`,
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
    <div className="flex items-center gap-4" data-testid="type-donut">
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
