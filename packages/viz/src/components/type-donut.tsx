"use client";

/** Memory-type distribution donut (recharts via the shared chart component).
 *
 * One slice per frontmatter `type` (user / feedback / project / reference /
 * untyped), sized by file count. A compact legend doubles as the count readout
 * so the donut stays useful even at small sizes.
 */
import {
  ChartContainer,
  ChartTooltip,
  ChartTooltipContent,
} from "@workspace/ui/components/chart";
import { Cell, Pie, PieChart } from "recharts";

/** Stable colour per memory type; core emits `unknown` for untyped entries. */
const TYPE_COLORS: Record<string, string> = {
  user: "var(--chart-1)",
  feedback: "var(--chart-2)",
  project: "var(--chart-3)",
  reference: "var(--chart-4)",
  unknown: "var(--chart-5)",
};

/** Render the type-count donut. `typeCounts` is a `{ type: count }` record. */
export const TypeDonut = ({
  typeCounts,
}: {
  readonly typeCounts: Readonly<Record<string, number>>;
}) => {
  const data = Object.entries(typeCounts)
    .filter(([, n]) => n > 0)
    .map(([type, count]) => ({
      type,
      count,
      fill: TYPE_COLORS[type] ?? "var(--chart-5)",
    }));
  if (data.length === 0) {
    return (
      <p className="text-muted-foreground text-sm" data-testid="donut-empty">
        No typed memories yet.
      </p>
    );
  }
  const config = Object.fromEntries(
    data.map((d) => [d.type, { label: d.type, color: d.fill }])
  );
  return (
    <div className="flex items-center gap-4" data-testid="type-donut">
      <ChartContainer
        className="aspect-square h-[140px] w-[140px]"
        config={config}
      >
        <PieChart>
          <ChartTooltip content={<ChartTooltipContent nameKey="type" />} />
          <Pie data={data} dataKey="count" innerRadius={40} nameKey="type">
            {data.map((d) => (
              <Cell fill={d.fill} key={d.type} />
            ))}
          </Pie>
        </PieChart>
      </ChartContainer>
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
