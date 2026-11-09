/** Stats first load: report-shaped skeleton, a status note and a counter. */
import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { StatsPending } from "../src/routes/stats/stats-pending";

const EMPTY_STATUS = /<p role="status"><\/p>/;
const HIDDEN_COUNTER = /<span aria-hidden="true"[^>]*>0 s<\/span>/;
const COUNTER_15 = /data-testid="stats-elapsed">15 s<\/span>/;
const SKELETON = /data-slot="skeleton"/g;

test("pending stats mount an empty status region and hide the ticking counter", () => {
  const html = renderToStaticMarkup(<StatsPending since={Date.now()} />);
  expect(html).toContain('data-testid="stats-pending"');
  // The note is added after mount, so the live region announces it.
  expect(html).toMatch(EMPTY_STATUS);
  expect(html).toMatch(HIDDEN_COUNTER);
  expect(html).not.toContain("progressbar");
  expect(html.match(SKELETON)?.length).toBeGreaterThan(5);
});

test("the counter counts from the pass start, not from mount", () => {
  const html = renderToStaticMarkup(
    <StatsPending since={Date.now() - 15_000} />
  );
  expect(html).toMatch(COUNTER_15);
});
