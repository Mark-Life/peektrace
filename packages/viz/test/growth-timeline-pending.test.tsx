/** The growth chart's loading box draws the chart's frame and legend, so the
 *  page below it keeps its place when the chart lands. Synthetic fixtures only. */
import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import {
  GrowthTimeline,
  GrowthTimelinePending,
} from "../src/components/growth-timeline";
import { MOCK_SESSION } from "../src/mock/session";

const LEGEND = /<div class="flex flex-wrap gap-3 text-xs">.*?<\/div>/;
const SVG = /<svg[\s\S]*<\/svg>/;
const PLOT_BOX = /<div class="w-full" style="[^"]*"><\/div>/;
const BUSY = / aria-busy="true"/;
const TEST_ID = / data-testid="[^"]*"/;

/** Markup with the plot itself (the SVG or its placeholder box) cut out. */
const frameOf = (html: string) =>
  html
    .replace(SVG, "<plot/>")
    .replace(PLOT_BOX, "<plot/>")
    .replace(BUSY, "")
    .replace(TEST_ID, "");

test("pending box has the chart's frame and legend", () => {
  expect(MOCK_SESSION.snapshots.length).toBeGreaterThan(0);
  const chart = renderToStaticMarkup(<GrowthTimeline a={MOCK_SESSION} />);
  const pending = renderToStaticMarkup(
    <GrowthTimelinePending a={MOCK_SESSION} />
  );
  expect(pending.match(LEGEND)?.[0]).toBe(chart.match(LEGEND)?.[0]);
  expect(pending).toContain("aspect-ratio:1000 / 440");
  expect(frameOf(pending)).toBe(frameOf(chart));
});

test("a session without turns shows the chart's empty line while pending", () => {
  const empty = { ...MOCK_SESSION, snapshots: [], turnCount: 0 };
  expect(renderToStaticMarkup(<GrowthTimelinePending a={empty} />)).toBe(
    renderToStaticMarkup(<GrowthTimeline a={empty} />)
  );
});
