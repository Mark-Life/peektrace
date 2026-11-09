/** Session rail over a large listing: rows stay compact and readable. */

import { describe, expect, test } from "bun:test";
import type { SessionHeader } from "@workspace/core/services/sessions/schema";
import { renderToStaticMarkup } from "react-dom/server";
import { SessionList } from "../src/routes/sessions/session-list";

const COUNT = 500;
const FLEX_WRAP = /\bflex-wrap\b/;

const header = (i: number): SessionHeader => {
  const cwd = `/home/dev/code/app-${i % 4}`;
  const start = Date.parse("2026-10-01T08:00:00.000Z") + i * 3_600_000;
  // session 0 started first but is still being written to
  const updated =
    i === 0 ? Date.parse("2026-11-07T22:00:00.000Z") : start + 60_000;
  return {
    id: `id-${String(i).padStart(4, "0")}`,
    path: `${cwd}/id-${i}.jsonl`,
    agent: "claude" as SessionHeader["agent"],
    cwd,
    project: cwd.replaceAll("/", "-"),
    gitBranch: i % 2 === 0 ? "HEAD" : "main",
    model: "claude-opus-4",
    title: `Synthetic task ${i}`,
    startedAt: new Date(start).toISOString(),
    updatedAt: new Date(updated).toISOString(),
    messageCount: 4,
    sizeBytes: 1024,
  };
};

const headers = Array.from({ length: COUNT }, (_, i) => header(i));
const html = renderToStaticMarkup(
  <SessionList headers={headers} selectedId={null} />
);
const rowTags = html.match(/<a[^>]*data-testid="session-row"[^>]*>/g) ?? [];
const rowCount = rowTags.length;

describe("SessionList over 500 sessions", () => {
  test("mounts only a window of rows, not all of them", () => {
    expect(rowCount).toBeLessThan(100);
  });

  test("labels the project by the last folder of cwd, not the encoded slug", () => {
    expect(html.includes("-home-dev-code-app-1")).toBe(false);
    expect(html.includes("app-1")).toBe(true);
  });

  test("hides a detached HEAD branch", () => {
    expect(html.includes(">HEAD<")).toBe(false);
  });

  test("puts the most recently updated session first", () => {
    const first = html.indexOf("Synthetic task 0<");
    const newestStart = html.indexOf(`Synthetic task ${COUNT - 1}<`);
    expect(first).toBeGreaterThan(-1);
    expect(first).toBeLessThan(
      newestStart === -1 ? Number.POSITIVE_INFINITY : newestStart
    );
  });

  test("mounts no tooltip per row", () => {
    expect(html.includes('data-slot="tooltip-trigger"')).toBe(false);
  });

  test("gives every row the same fixed height", () => {
    expect(rowCount).toBeGreaterThan(0);
    expect(rowTags.every((tag) => tag.includes("h-[3.75rem]"))).toBe(true);
    // a leftover `flex-wrap` would push the second line into a new column
    expect(rowTags.some((tag) => FLEX_WRAP.test(tag))).toBe(false);
  });

  test("keeps the model in a row tooltip", () => {
    expect(rowCount).toBeGreaterThan(0);
    expect(html.includes(' · claude-opus-4"')).toBe(true);
  });

  test("shows the session count over the full list", () => {
    expect(html.includes(`${COUNT} sessions`)).toBe(true);
  });
});

describe("SessionList live marker", () => {
  test("marks only a session written to in the last few minutes", () => {
    const now = Date.now();
    const live = {
      ...header(1),
      id: "live",
      updatedAt: new Date(now - 30_000).toISOString(),
    };
    const idle = {
      ...header(2),
      id: "idle",
      updatedAt: new Date(now - 3_600_000).toISOString(),
    };
    const markup = renderToStaticMarkup(
      <SessionList headers={[idle, live]} selectedId={null} />
    );
    expect(markup.split('data-testid="session-live"').length - 1).toBe(1);
    expect(
      markup.includes('title="Live: wrote a line in the last 5 min"')
    ).toBe(true);
    expect(markup.indexOf("session-live")).toBeLessThan(
      markup.indexOf("Synthetic task 1<")
    );
  });
});
