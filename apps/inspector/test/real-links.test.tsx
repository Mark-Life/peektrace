/** Session links render as real `<a href>` so Cmd-click, middle-click and
 * "Copy link" work, and each page names its tab. Synthetic fixtures only. */

import { expect, test } from "bun:test";

import { TooltipProvider } from "@workspace/ui/components/tooltip";
import type {
  SampleRef,
  SessionHeader,
} from "@workspace/view-core/api-types.gen";
import { analyzedTitle } from "@workspace/viz/components/verdict-header";
import { renderToStaticMarkup } from "react-dom/server";

import {
  pageTitle,
  setTitleBase,
  setTitleOverride,
} from "../src/lib/document-title";
import { markedSessionHash, sessionHash } from "../src/lib/routes";
import { SessionList } from "../src/routes/sessions/session-list";
import { SessionLinks } from "../src/routes/stats/session-links";

const header: SessionHeader = {
  agent: "claude",
  id: "sess-aaaa-1111",
  messageCount: 3,
  path: "/fixture/sess-aaaa-1111.jsonl",
  project: "demo-project",
  sizeBytes: 1024,
  title: "Synthetic session",
};

const sample: SampleRef = {
  agent: "claude",
  kind: "main",
  line: "Bash: echo hi",
  parent: null,
  project: "demo-project",
  seq: 1,
  sid: "sess-bbbb-2222",
  toolUseId: "toolu_fixture_1",
  ts: null,
};

const escapeAttr = (s: string) => s.replaceAll("&", "&amp;");

test("a session row is an <a href> to its session", () => {
  const html = renderToStaticMarkup(
    <TooltipProvider>
      <SessionList headers={[header]} selectedId={null} />
    </TooltipProvider>
  );

  expect(html).toContain('href="#/sessions?id=sess-aaaa-1111"');
});

test("a stats session chip is an <a href> built by markedSessionHash", () => {
  const html = renderToStaticMarkup(
    <SessionLinks detector="retry" samples={[sample]} />
  );

  const href = markedSessionHash({
    call: sample.toolUseId,
    id: sample.sid,
    mark: "retry",
    parent: null,
  });

  expect(html).toContain(`href="${escapeAttr(href)}"`);
});

test("sessionHash names the parent only for a nested transcript", () => {
  expect(sessionHash({ id: "sess-aaaa-1111", parent: null })).toBe(
    "#/sessions?id=sess-aaaa-1111"
  );
  expect(sessionHash({ id: "agent-cccc", parent: "sess-aaaa-1111" })).toBe(
    "#/sessions?id=agent-cccc&parent=sess-aaaa-1111"
  );
});

test("a session row marks the open session as the current page", () => {
  const html = renderToStaticMarkup(
    <TooltipProvider>
      <SessionList headers={[header]} selectedId={header.id} />
    </TooltipProvider>
  );

  expect(html).toContain('aria-current="page"');
});

test("pageTitle suffixes the app name", () => {
  expect(pageTitle("Stats")).toBe("Stats · Peektrace");
});

test("the route label titles the tab until a page overrides it, in any order", () => {
  const doc = { title: "" };
  const prev = Object.getOwnPropertyDescriptor(globalThis, "document");
  Object.defineProperty(globalThis, "document", {
    configurable: true,
    value: doc,
  });

  try {
    setTitleBase("Stats");
    expect(doc.title).toBe("Stats · Peektrace");
    // A session is open but its detail has not mounted yet.
    setTitleBase("Sessions");
    expect(doc.title).toBe("Sessions · Peektrace");
    setTitleOverride("Synthetic session");
    expect(doc.title).toBe("Synthetic session · Peektrace");
    // The shell re-applying its label must not clobber the override.
    setTitleBase("Sessions");
    expect(doc.title).toBe("Synthetic session · Peektrace");
    setTitleOverride(null);
    expect(doc.title).toBe("Sessions · Peektrace");
  } finally {
    setTitleOverride(null);
    setTitleBase(null);

    if (prev) {
      Object.defineProperty(globalThis, "document", prev);
    } else {
      Reflect.deleteProperty(globalThis, "document");
    }
  }
});

test("analyzedTitle uses the title, else names the harness", () => {
  expect(
    analyzedTitle({ provider: "claude-code", title: "Synthetic session" })
  ).toBe("Synthetic session");
  expect(analyzedTitle({ provider: "claude-code", title: "" })).toBe(
    "Claude Code session"
  );
  expect(analyzedTitle({ provider: "other-agent", title: "" })).toBe(
    "other-agent session"
  );
});
