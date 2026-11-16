/** SPIKE scratch — SSR comparison harness. Not part of the package build.
 *
 * Server-renders the shipping components and the two unported TanStack Charts
 * candidates with `react-dom/server`, and reports what actually reaches the
 * HTML. Run with:
 *   bun run packages/viz/spike/ssr-report.tsx
 */
import type { ReactElement } from "react";
import { renderToString } from "react-dom/server";

import { GrowthTimeline } from "../src/components/growth-timeline";
import { LinkGraph } from "../src/components/link-graph";
import { TypeDonut } from "../src/components/type-donut";
import { MOCK_GRAPH, MOCK_TYPE_COUNTS } from "../src/mock/memory";
import { MOCK_SESSION } from "../src/mock/session";
import { GrowthTimelineTanstack } from "./growth-timeline-tanstack";
import { LinkGraphTanstack } from "./link-graph-tanstack";

const report = (name: string, html: string) => {
  const count = (re: RegExp) => html.match(re)?.length ?? 0;
  console.log(
    [
      `--- ${name}`,
      `  bytes           ${html.length}`,
      `  <svg>           ${count(/<svg/gu)}`,
      `  <path>          ${count(/<path/gu)}`,
      `  <circle>        ${count(/<circle/gu)}`,
      `  <text>          ${count(/<text/gu)}`,
      `  <line>          ${count(/<line/gu)}`,
      `  <rect>          ${count(/<rect/gu)}`,
      `  role="img"      ${count(/role="img"/gu)}`,
      `  aria-label      ${count(/aria-label=/gu)}`,
      `  <title>/<desc>  ${count(/<(?<tag>title|desc)/gu)}`,
      `  tabindex        ${count(/tabindex=/gu)}`,
      `  roledescription ${count(/aria-roledescription=/gu)}`,
    ].join("\n")
  );
};

const run = (name: string, node: ReactElement) => {
  try {
    report(name, renderToString(node));
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    console.log(`--- ${name}\n  THREW: ${message}`);
  }
};

run("growth-timeline (hand-rolled SVG)", <GrowthTimeline a={MOCK_SESSION} />);

run(
  "growth-timeline (TanStack Charts)",
  <GrowthTimelineTanstack a={MOCK_SESSION} />
);

run("link-graph (hand-rolled SVG)", <LinkGraph graph={MOCK_GRAPH} />);

run("link-graph (TanStack Charts)", <LinkGraphTanstack graph={MOCK_GRAPH} />);

run(
  "type-donut (TanStack Charts)",
  <TypeDonut typeCounts={MOCK_TYPE_COUNTS} />
);
