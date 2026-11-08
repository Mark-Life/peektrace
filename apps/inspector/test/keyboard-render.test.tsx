/** What keyboard users land on, as rendered markup: one Tab stop in the
 * session rail, the help sheet listing every shortcut, the skip links. */

import { describe, expect, test } from "bun:test";
import type { SessionHeader } from "@workspace/core/services/sessions/schema";
import { TooltipProvider } from "@workspace/ui/components/tooltip";
import { renderToStaticMarkup } from "react-dom/server";
import { SessionList } from "../src/routes/sessions/session-list";

const header = (id: string, startedAt: string): SessionHeader => ({
  agent: "claude",
  id,
  messageCount: 3,
  path: `/fixtures/${id}.jsonl`,
  project: "demo",
  sizeBytes: 1024,
  startedAt,
  title: `Session ${id}`,
});

const HEADERS = [
  header("s1", "2026-01-01T10:00:00Z"),
  header("s2", "2026-01-02T10:00:00Z"),
  header("s3", "2026-01-03T10:00:00Z"),
];

const TAB_STOP = /<button[^>]*tabindex="0"[^>]*>/g;
const ROW = /data-testid="session-row"/g;

const renderList = (selectedId: string | null) =>
  renderToStaticMarkup(
    <TooltipProvider>
      <SessionList
        headers={HEADERS}
        onOpen={() => undefined}
        selectedId={selectedId}
      />
    </TooltipProvider>
  );

describe("session list", () => {
  test("the open session is the one Tab stop", () => {
    const html = renderList("s2");
    expect(html.match(ROW)).toHaveLength(3);
    const stops = html.match(TAB_STOP) ?? [];
    expect(stops).toHaveLength(1);
    expect(stops[0]).toContain('data-session-id="s2"');
  });

  test("with nothing open, the top row is the stop", () => {
    const stops = renderList(null).match(TAB_STOP) ?? [];
    expect(stops).toHaveLength(1);
    // Newest first.
    expect(stops[0]).toContain('data-session-id="s3"');
  });

  test("rows sit in list items and search names its shortcut", () => {
    const html = renderList(null);
    expect(html).toContain('<ul aria-label="Sessions"');
    expect(html.match(/<li>/g)).toHaveLength(3);
    expect(html).toContain('aria-keyshortcuts="/"');
  });
});
