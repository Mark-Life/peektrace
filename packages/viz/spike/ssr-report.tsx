/** SPIKE scratch — SSR comparison harness. Not part of the package build.
 *
 * Server-renders the hand-rolled, Recharts and TanStack Charts components with
 * `react-dom/server` and reports what actually reaches the HTML. Run with:
 *   bun run packages/viz/spike/ssr-report.tsx
 */
import type { ReactElement } from "react";
import { renderToString } from "react-dom/server";
import { GrowthTimeline } from "../src/components/growth-timeline";
import { GrowthTimelineTanstack } from "../src/components/growth-timeline-tanstack";
import { LinkGraph } from "../src/components/link-graph";
import { LinkGraphTanstack } from "../src/components/link-graph-tanstack";
import { TypeDonut } from "../src/components/type-donut";
import { TypeDonutTanstack } from "../src/components/type-donut-tanstack";
import { MOCK_GRAPH, MOCK_TYPE_COUNTS } from "../src/mock/memory";
import { MOCK_SESSION } from "../src/mock/session";

const report = (name: string, html: string) => {
  const count = (re: RegExp) => html.match(re)?.length ?? 0;
  console.log(
    [
      `--- ${name}`,
      `  bytes           ${html.length}`,
      `  <svg>           ${count(/<svg/g)}`,
      `  <path>          ${count(/<path/g)}`,
      `  <circle>        ${count(/<circle/g)}`,
      `  <text>          ${count(/<text/g)}`,
      `  <line>          ${count(/<line/g)}`,
      `  <rect>          ${count(/<rect/g)}`,
      `  role="img"      ${count(/role="img"/g)}`,
      `  aria-label      ${count(/aria-label=/g)}`,
      `  <title>/<desc>  ${count(/<(title|desc)/g)}`,
      `  tabindex        ${count(/tabindex=/g)}`,
      `  roledescription ${count(/aria-roledescription=/g)}`,
    ].join("\n")
  );
};

const run = (name: string, node: ReactElement) => {
  try {
    report(name, renderToString(node));
  } catch (error) {
    console.log(`--- ${name}\n  THREW: ${(error as Error).message}`);
  }
};

run("growth-timeline (hand-rolled SVG)", <GrowthTimeline a={MOCK_SESSION} />);
run(
  "growth-timeline (TanStack Charts)",
  <GrowthTimelineTanstack a={MOCK_SESSION} />
);
run("link-graph (hand-rolled SVG)", <LinkGraph graph={MOCK_GRAPH} />);
run("link-graph (TanStack Charts)", <LinkGraphTanstack graph={MOCK_GRAPH} />);
run("type-donut (Recharts)", <TypeDonut typeCounts={MOCK_TYPE_COUNTS} />);
run(
  "type-donut (TanStack Charts)",
  <TypeDonutTanstack typeCounts={MOCK_TYPE_COUNTS} />
);
