/** Stats header: the rescan button while a pass is in flight. */
import { expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import { ReportHeader } from "../src/routes/stats/report-header";
import { report } from "./stats-report-fixture";

const RESCAN_BUSY =
  /data-testid="stats-rescan"[^>]*\sdisabled=""|\sdisabled=""[^>]*data-testid="stats-rescan"/;

test("a rescan in flight disables the button and says so", () => {
  const header = (rescanning: boolean) =>
    renderToStaticMarkup(
      <ReportHeader
        agent={null}
        days={30}
        onAgent={() => undefined}
        onDays={() => undefined}
        onRescan={() => undefined}
        prompt={null}
        report={report({ fails: [], time: [] })}
        rescanning={rescanning}
      />
    );
  expect(header(false)).toContain("Rescan</button>");
  expect(header(false)).not.toMatch(RESCAN_BUSY);
  expect(header(true)).toContain("Rescanning…");
  expect(header(true)).toMatch(RESCAN_BUSY);
});
