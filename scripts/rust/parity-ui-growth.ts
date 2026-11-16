/** The growth chart as the inspector draws it: `GrowthTimeline` rendered to
 * static SVG markup, then read back into the values `ui_helpers growth`
 * prints from `chart::geom` (viewBox units, `toFixed(1)` text). */
import type { AnalyzedSession } from "../../packages/core/src/services/sessions/schema";
// The chart's own React, so hooks and elements share one instance.
import { createElement } from "../../packages/viz/node_modules/react";
import { renderToStaticMarkup } from "../../packages/viz/node_modules/react-dom/server";
import { GrowthTimeline } from "../../packages/viz/src/components/growth-timeline";

const AREA_KEYS = 10;
const ENTITIES: Record<string, string> = {
  "&amp;": "&",
  "&lt;": "<",
  "&gt;": ">",
  "&quot;": '"',
  "&#x27;": "'",
};
const PATH = /<path d="([^"]*)"/g;
const PEAK = /<path class="fill-foreground" d="([^"]*)"/;
const CIRCLE = /<circle cx="([^"]*)" cy="([^"]*)" fill="[^"]*" r="([^"]*)"/g;
const TEXT = /<text[^>]*>(.*?)<\/text>/g;
const LINE_Y =
  /<line stroke="rgba\(255,255,255,0\.07\)" x1="[^"]*" x2="[^"]*" y1="([^"]*)"/g;
const CLIFF =
  /<line stroke="rgba\(210,153,34,0\.9\)" stroke-dasharray="3 2" x1="([^"]*)"/g;
const DZ =
  /stroke="rgba\(248,81,73,0\.7\)" stroke-dasharray="4 3" x1="[^"]*" x2="[^"]*" y1="([^"]*)"/;
const ARIA = /aria-label="([^"]*)"/;
const COMMENT = /<!-- -->/g;
const ENTITY = /&(amp|lt|gt|quot|#x27);/g;

const unentity = (s: string) =>
  s.replace(COMMENT, "").replace(ENTITY, (m) => ENTITIES[m] ?? m);

/** `null` when the chart draws no plot. */
export const growthAnswer = (a: AnalyzedSession) => {
  if (a.snapshots.length === 0 || !(a.contextWindow > 0)) {
    return null;
  }
  const svg = renderToStaticMarkup(createElement(GrowthTimeline, { a }));
  const paths = [...svg.matchAll(PATH)].map((m) => m[1] ?? "");
  const dots = [...svg.matchAll(CIRCLE)]
    .slice(0, a.snapshots.length)
    .map((m) => [m[1], m[2], m[3]]);
  return {
    areas: paths.slice(0, AREA_KEYS),
    silhouette: paths.at(-1) ?? "",
    dots,
    grid: [...svg.matchAll(LINE_Y)].map((m) => m[1]),
    cliffs: [...svg.matchAll(CLIFF)].map((m) => m[1]),
    dz: svg.match(DZ)?.[1] ?? "",
    texts: [...svg.matchAll(TEXT)].map((m) => unentity(m[1] ?? "")),
    peak: svg.match(PEAK)?.[1] ?? "",
    aria: unentity(svg.match(ARIA)?.[1] ?? ""),
  };
};
