/** Stats header: the rescan button, and the corpus and redaction lines. */
import { expect, test } from "bun:test";

import type { StatsReport } from "@workspace/view-core/api-types.gen";
import { renderToStaticMarkup } from "react-dom/server";

import { ReportHeader } from "../src/routes/stats/report-header";
import { report } from "./stats-report-fixture";

const RESCAN_BUSY =
  /data-testid="stats-rescan"[^>]*\sdisabled=""|\sdisabled=""[^>]*data-testid="stats-rescan"/u;

const header = (
  rescanning: boolean,
  data: StatsReport = report({ fails: [], time: [] })
) =>
  renderToStaticMarkup(
    <ReportHeader
      agent={null}
      days={30}
      onAgent={() => {}}
      onDays={() => {}}
      onRescan={() => {}}
      prompt={null}
      report={data}
      rescanning={rescanning}
    />
  );

test("a rescan in flight disables the button and says so", () => {
  expect(header(false)).toContain("Rescan</button>");
  expect(header(false)).not.toMatch(RESCAN_BUSY);
  expect(header(true)).toContain("Rescanning…");
  expect(header(true)).toMatch(RESCAN_BUSY);
});

test("the header counts main sessions apart from subagent runs", () => {
  const base = report({ fails: [], time: [] });

  const html = header(false, {
    ...base,
    corpus: {
      ...base.corpus,
      kinds: { main: 1302, subagent: 204, "workflow-agent": 4000 },
      sessions: 1302,
      transcripts: 5506,
    },
  });

  expect(html).toContain("1,302 sessions + 4,204");
  expect(html).toContain("subagent runs");
  expect(html).not.toContain("transcripts");
  expect(html).toContain("Commands and paths are");
});
