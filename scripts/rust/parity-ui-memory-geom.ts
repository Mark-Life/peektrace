/** The memory charts as the inspector draws them: `LinkGraph`, `TypeDonut`
 * and `BudgetGauge` rendered to static markup, then read back into the
 * values `ui_helpers memoryGeom` prints, plus graph, count and budget cases
 * no corpus has. */
import type {
  GraphData,
  IndexBudget,
} from "../../packages/core/src/services/memory/types";
// The chart's own React, so hooks and elements share one instance.
import { createElement } from "../../packages/viz/node_modules/react";
import { renderToStaticMarkup } from "../../packages/viz/node_modules/react-dom/server";
import { BudgetGauge } from "../../packages/viz/src/components/budget-gauge";
import { LinkGraph } from "../../packages/viz/src/components/link-graph";
import { TypeDonut } from "../../packages/viz/src/components/type-donut";
import {
  MOCK_GRAPH,
  MOCK_INDEX_BUDGET,
  MOCK_TYPE_COUNTS,
} from "../../packages/viz/src/mock/memory";

const ENTITIES: Record<string, string> = {
  "&amp;": "&",
  "&lt;": "<",
  "&gt;": ">",
  "&quot;": '"',
  "&#x27;": "'",
};
const ENTITY = /&(amp|lt|gt|quot|#x27);/g;
const TAG = /<[^>]*>/g;
const UPPER = /<span class="[^"]*uppercase[^"]*">([^<]*)<\/span>/g;
const LINE =
  /<line class="([^"]*)"(?: stroke-dasharray="[^"]*")? stroke-width="1" x1="([^"]*)" x2="([^"]*)" y1="([^"]*)" y2="([^"]*)"/g;
const NODE =
  /<circle class="([^"]*)" cx="([^"]*)" cy="([^"]*)" r="([^"]*)"><\/circle><text[^>]* dy="([^"]*)"[^>]*>(.*?)<\/text>/g;
const SLICE =
  /<path data-ts-key="[^"]*:string:(\d+):([^"]*)" fill="var\(--chart-(\d)\)"[^>]* d="([^"]*)"/g;
const COMMAND = /[MALZ][^MALZ]*/g;
const BAR =
  /<span class="text-muted-foreground">([^<]*)<\/span><span class="font-mono([^"]*)">([^<]*)<\/span><\/div><div class="[^"]*"><div class="h-full rounded-full transition-all bg-([a-z]+)-(\d+)" style="width:([^%]*)%"/g;
const FOLD = /data-testid="below-fold">.*?<\/svg><span>(.*?)<\/span><\/div>/;
const SCALE6 = 1e6;

const unentity = (s: string) => s.replace(ENTITY, (m) => ENTITIES[m] ?? m);
const r6 = (x: number) => Math.round(x * SCALE6) / SCALE6;

const ringAnswer = (graph: typeof GraphData.Type) => {
  const svg = renderToStaticMarkup(createElement(LinkGraph, { graph }));
  if (graph.nodes.length === 0) {
    return { empty: true };
  }
  return {
    edges: [...svg.matchAll(LINE)].map((m) => [
      r6(Number(m[2])),
      r6(Number(m[4])),
      r6(Number(m[3])),
      r6(Number(m[5])),
      (m[1] ?? "").includes("muted-foreground"),
    ]),
    nodes: [...svg.matchAll(NODE)].map((m) => [
      unentity(m[6] ?? ""),
      r6(Number(m[2])),
      r6(Number(m[3])),
      r6(Number(m[4])),
      (m[1] ?? "").includes("emerald"),
      r6(Number(m[3]) + Number(m[5])),
    ]),
  };
};

/** M, the outer arc's end, L, the inner arc's end. */
const corners = (d: string) =>
  [...d.matchAll(COMMAND)].flatMap((m) => {
    const nums = (m[0] ?? "").slice(1).split(",").filter(Boolean).map(Number);
    return nums.length >= 2 ? [nums.slice(-2)] : [];
  });

const donutAnswer = (typeCounts: Readonly<Record<string, number>>) => {
  const svg = renderToStaticMarkup(createElement(TypeDonut, { typeCounts }));
  return [...svg.matchAll(SLICE)].map((m) => {
    const len = Number(m[1]);
    return [
      unentity(m[2] ?? "").slice(0, len),
      Number(m[3]),
      corners(m[4] ?? ""),
    ];
  });
};

const barsAnswer = (budget: typeof IndexBudget.Type) => {
  const html = renderToStaticMarkup(createElement(BudgetGauge, { budget }));
  const fold = html.match(FOLD)?.[1];
  return {
    absent: html.includes('data-testid="budget-absent"'),
    bars: [...html.matchAll(BAR)].map((m) => [
      m[1],
      unentity(m[3] ?? ""),
      Number(m[6]),
      (m[2] ?? "").includes("text-red-400"),
      `${m[4]}${m[5]}`,
    ]),
    belowFold:
      fold === undefined
        ? null
        : unentity(
            fold
              .replace(UPPER, (_: string, t: string) => t.toUpperCase())
              .replace(TAG, "")
          ),
  };
};

export interface GeomInput {
  readonly budgets: (typeof IndexBudget.Type)[];
  readonly counts: Record<string, number>[];
  readonly graphs: (typeof GraphData.Type)[];
}

export const geomAnswer = (g: GeomInput) => ({
  rings: g.graphs.map(ringAnswer),
  donuts: g.counts.map(donutAnswer),
  bars: g.budgets.map(barsAnswer),
});

/** Graph, count and budget cases no corpus has. */
export const syntheticGeom = (): GeomInput => {
  const node = (slug: string, bytes: number, inIndex = true) => ({
    slug,
    type: "user",
    bytes,
    inIndex,
    inDeg: 0,
    outDeg: 0,
  });
  const edge = (from: string, to: string, resolvedTo?: string) => ({
    from,
    to,
    kind: "wiki" as const,
    resolved: resolvedTo !== undefined,
    ...(resolvedTo === undefined ? {} : { resolvedTo }),
    line: 1,
  });
  const graph = (
    nodes: ReturnType<typeof node>[],
    edges = [] as ReturnType<typeof edge>[]
  ) => ({ nodes, edges, orphans: [] }) as unknown as typeof GraphData.Type;
  return {
    graphs: [
      graph([]),
      graph([node("solo", 10)]),
      graph([node("a", 0), node("b", 0, false)], [edge("a", "b", "b")]),
      graph(
        [node("a", 5), node("b", 50), node("c", 500)],
        [edge("a", "b"), edge("c", "c", "c"), edge("__index__", "a", "a")]
      ),
      graph(
        [node("dup", 1), node("x", 2), node("dup", 3)],
        [edge("x", "dup", "dup")]
      ),
      MOCK_GRAPH,
    ],
    counts: [
      { user: 1 },
      { user: 1, feedback: 2, project: 3, reference: 4, unknown: 5 },
      { user: 0, project: 2 },
      JSON.parse('{"b":1,"1":2,"a":4}') as Record<string, number>,
      { user: 1, project: 999 },
      { ...MOCK_TYPE_COUNTS },
    ],
    budgets: [
      MOCK_INDEX_BUDGET,
      {
        ...MOCK_INDEX_BUDGET,
        lines: 161,
        bytes: 20_481,
        belowFoldCount: 1,
        overBudget: false,
      },
      {
        ...MOCK_INDEX_BUDGET,
        lines: 0,
        bytes: 0,
        belowFoldCount: 0,
        overBudget: false,
      },
      { ...MOCK_INDEX_BUDGET, kind: "absent" },
      {
        ...MOCK_INDEX_BUDGET,
        lines: 1,
        bytes: 255,
        kind: "monolithic",
        belowFoldCount: 2,
      },
    ],
  };
};
