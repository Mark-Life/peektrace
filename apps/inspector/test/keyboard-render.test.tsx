/** What keyboard users land on, as rendered markup: one Tab stop in the
 * session rail, the help sheet listing every shortcut, the skip links. */

import { describe, expect, test } from "bun:test";
import type { SessionHeader } from "@workspace/core/services/sessions/schema";
import { TooltipProvider } from "@workspace/ui/components/tooltip";
import { renderToStaticMarkup } from "react-dom/server";
import { ShortcutsList } from "../src/components/shortcuts-dialog";
import { SkipLinks } from "../src/components/skip-links";
import { SHORTCUTS } from "../src/lib/shortcuts";
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

const LIST_ITEM = /<li[\s>]/g;
const SKIP = /data-testid="skip-link"/g;
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

  test("a stop outside the drawn window is still drawn, so Tab reaches it", () => {
    const many = Array.from({ length: 500 }, (_, i) =>
      header(
        `m${i}`,
        new Date(Date.parse("2026-01-01T00:00:00Z") + i * 60_000).toISOString()
      )
    );
    const html = renderToStaticMarkup(
      <TooltipProvider>
        <SessionList headers={many} onOpen={() => undefined} selectedId="m3" />
      </TooltipProvider>
    );
    expect((html.match(ROW) ?? []).length).toBeLessThan(many.length);
    const stops = html.match(TAB_STOP) ?? [];
    expect(stops).toHaveLength(1);
    expect(stops[0]).toContain('data-session-id="m3"');
  });

  test("rows sit in list items and search names its shortcut", () => {
    const html = renderList(null);
    expect(html).toContain('<ul aria-label="Sessions"');
    expect(html.match(LIST_ITEM)).toHaveLength(3);
    expect(html).toContain('aria-keyshortcuts="/"');
  });
});

describe("shortcuts sheet", () => {
  test("lists every entry of the table with its keys", () => {
    const html = renderToStaticMarkup(<ShortcutsList mac={false} />);
    for (const s of SHORTCUTS) {
      expect(html).toContain(`data-shortcut="${s.id}"`);
      expect(html).toContain(s.label);
    }
    expect(html).toContain("Ctrl+B");
    expect(renderToStaticMarkup(<ShortcutsList mac />)).toContain("⌘B");
  });
});

describe("skip links", () => {
  test("skip to content always, skip to session only with one open", () => {
    const closed = renderToStaticMarkup(<SkipLinks sessionOpen={false} />);
    expect(closed.match(SKIP)).toHaveLength(1);
    expect(closed).toContain("Skip to content");
    const open = renderToStaticMarkup(<SkipLinks sessionOpen />);
    expect(open.match(SKIP)).toHaveLength(2);
    expect(open).toContain("Skip to session");
  });

  test("drop below the window buttons under an inset titlebar", () => {
    expect(renderToStaticMarkup(<SkipLinks sessionOpen={false} />)).toContain(
      "focus:top-2"
    );
    const inset = renderToStaticMarkup(<SkipLinks inset sessionOpen={false} />);
    expect(inset).toContain("focus:top-10");
    expect(inset).not.toContain("focus:top-2");
  });
});
