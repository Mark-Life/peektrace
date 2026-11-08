/** Session export renderers over a synthetic parsed session. */
import { describe, expect, test } from "bun:test";
import {
  collapseHomePaths,
  fenceFor,
  renderExport,
  renderHtml,
  renderJson,
  renderMarkdown,
} from "../../src/services/sessions/export/index";
import { eventBadgeLabel } from "../../src/services/sessions/labels";
import type {
  ParsedSession,
  TimelineEvent,
} from "../../src/services/sessions/schema";

const TRANSCRIPT_PATH = "/home/demo/.claude/projects/x/secret-transcript.jsonl";
const SUBAGENT_PATH = "/home/demo/.claude/projects/x/subagents/agent-a.jsonl";
const XSS = `<script>alert(1)</script> "onerror=alert(2) <img src=x onerror=alert(3)>`;

const ev = (
  index: number,
  kind: TimelineEvent["kind"],
  body: string,
  extra: Partial<TimelineEvent> = {}
): TimelineEvent => ({
  index,
  kind,
  title: kind,
  preview: body.slice(0, 20),
  body,
  tokensEst: 10,
  ...extra,
});

const session: ParsedSession = {
  provider: "claude",
  sessionId: "sess-1",
  path: TRANSCRIPT_PATH,
  cwd: "/home/demo/work",
  gitBranch: "main",
  title: "Demo <session>",
  models: ["claude-opus-4-1"],
  startedAt: "2026-01-01T00:00:00.000Z",
  endedAt: "2026-01-01T00:05:00.000Z",
  events: [
    ev(0, "user-prompt", `hello ${XSS}`, { promptOrigin: "human" }),
    ev(1, "assistant-thinking", "", { preview: "" }),
    ev(2, "assistant-text", "sure, here is code:\n```ts\nconst a = 1\n```"),
    ev(3, "tool-call", '{"file_path":"/home/demo/work/a.ts"}', {
      toolName: "Read",
      toolUseId: "t1",
      title: "Read /home/demo/work/a.ts",
    }),
    ev(4, "tool-result", "line 1\n````\nnested fence\n````", {
      toolUseId: "t1",
    }),
    ev(5, "compaction", "summary of earlier work"),
    ev(6, "user-prompt", "look [image: image/png]", { promptOrigin: "human" }),
  ],
  turns: [],
  compactionIndexes: [5],
  subagents: [
    {
      id: "agent-a",
      agentType: "explore",
      description: "scan /home/demo/work",
      path: SUBAGENT_PATH,
      turns: 2,
      peakContextTokens: 1234,
    },
  ],
};

const EVENT_HANDLER_ATTR = /<[^>]+\son\w+=/;
const EXTERNAL_ATTR = /<[^>]*\s(?:src|href)=/;
const DARK_ERR_VAR = /prefers-color-scheme:dark[^}]*--err:/;
const BARE_FENCE = /^```$/m;
const URL_SCHEME = /https?:/;

const opts = { exportedAt: "2026-02-02T00:00:00.000Z", redacted: true };

describe("collapseHomePaths", () => {
  test("collapses home prefixes on both OSes", () => {
    expect(collapseHomePaths("/home/x/a.ts")).toBe("~/a.ts");
    expect(collapseHomePaths("/Users/x")).toBe("~");
    expect(collapseHomePaths("C:\\Users\\x\\a.ts")).toBe("~\\a.ts");
    expect(collapseHomePaths("C:\\\\Users\\\\x\\\\a")).toBe("~\\\\a");
  });
  test("collapses URI, WSL and slug forms", () => {
    expect(collapseHomePaths("open file:///home/alice/notes.txt")).toBe(
      "open file:///~/notes.txt"
    );
    expect(collapseHomePaths("vscode://file/home/alice/x")).toBe(
      "vscode://file/~/x"
    );
    expect(collapseHomePaths("file:///Users/alice/x")).toBe("file:///~/x");
    expect(collapseHomePaths("/mnt/c/Users/bob/x")).toBe("~/x");
    expect(collapseHomePaths("/c/Users/bob/x")).toBe("~/x");
    expect(
      collapseHomePaths(
        "/home/alice/.claude/projects/-home-alice-Code-proj/memory"
      )
    ).toBe("~/.claude/projects/-~-Code-proj/memory");
    expect(collapseHomePaths("C--Users-bob-proj")).toBe("-~-proj");
  });
  test("keeps surrounding shell and bracket text", () => {
    expect(collapseHomePaths("echo $\u007bHOME:-/home/alice}")).toBe(
      "echo $\u007bHOME:-~}"
    );
    expect(collapseHomePaths("see [/home/alice] ok")).toBe("see [~] ok");
    expect(collapseHomePaths("cd /home/alice&&ls")).toBe("cd ~&&ls");
    expect(collapseHomePaths("in /home/alice.")).toBe("in ~.");
  });
  test("handles spaced names and Windows letter case", () => {
    expect(collapseHomePaths("/Users/John Smith/proj")).toBe("~/proj");
    expect(collapseHomePaths("C:\\Users\\John Smith\\proj")).toBe("~\\proj");
    expect(collapseHomePaths("c:\\users\\bob\\x")).toBe("~\\x");
  });
  test("collapses mid-string, leaves lookalikes alone", () => {
    expect(collapseHomePaths('{"p":"/home/x/a"} and /Users/y/b')).toBe(
      '{"p":"~/a"} and ~/b'
    );
    expect(collapseHomePaths("/homework/a /opt/home/x/a")).toBe(
      "/homework/a /opt/home/x/a"
    );
  });
});

describe("collapseHomePaths: other layouts and names", () => {
  test("/var/home and WSL UNC paths", () => {
    expect(collapseHomePaths("/var/home/alice/x")).toBe("~/x");
    expect(collapseHomePaths("\\\\wsl.localhost\\Ubuntu\\home\\alice\\x")).toBe(
      "~\\x"
    );
    expect(collapseHomePaths("\\\\wsl$\\Ubuntu\\home\\alice\\x")).toBe("~\\x");
  });
  test("non-ASCII user names collapse in full", () => {
    expect(collapseHomePaths("/Users/\u00D3lafur/x")).toBe("~/x");
    expect(collapseHomePaths("C:\\Users\\\u00C1d\u00E1m\\x")).toBe("~\\x");
    expect(collapseHomePaths("/home/\u0438\u0432\u0430\u043D/x")).toBe("~/x");
    expect(collapseHomePaths("/Users/Jos\u00E9/x")).toBe("~/x");
    expect(collapseHomePaths("C:\\Users\\Zo\u00EB\\proj")).toBe("~\\proj");
    expect(collapseHomePaths("-Users-Jos\u00E9-proj")).toBe("-~-proj");
  });
  test("names with three or more words", () => {
    expect(collapseHomePaths("/Users/John Smith Jr/x")).toBe("~/x");
  });
});

describe("fenceFor", () => {
  test("is longer than any backtick run in the body", () => {
    expect(fenceFor("plain")).toBe("```");
    expect(fenceFor("a ``` b")).toBe("````");
    expect(fenceFor("a ````` b ``")).toBe("``````");
  });
});

describe("markdown", () => {
  const md = renderMarkdown(session, opts);
  test("headings count the exported events, empty ones are skipped", () => {
    const order = [...md.matchAll(/^### (\d+)\./gm)].map((m) => m[1]);
    expect(order).toEqual(["1", "2", "3", "4", "5", "6"]);
    expect(md.startsWith("# Demo <session>")).toBe(true);
  });
  test("a body holding a fence gets a longer fence", () => {
    expect(md).toContain("`````\nline 1\n````\nnested fence\n````\n`````");
  });
  test("labels match the shared badge labels", () => {
    for (const e of session.events.filter((x) => x.kind === "tool-call")) {
      expect(md).toContain(`. ${eventBadgeLabel(e)}`);
    }
  });
  test("no transcript paths, home paths collapsed when redacted", () => {
    expect(md).not.toContain("secret-transcript");
    expect(md).not.toContain("agent-a.jsonl");
    expect(md).not.toContain("/home/demo");
    expect(md).toContain("Directory: ~/work");
    expect(md).toContain("`agent-a`: explore, scan ~/work, 2 turns");
  });
  test("image markers stay as written by the parser", () => {
    expect(md).toContain("[image: image/png]");
  });
});

describe("json", () => {
  const out = JSON.parse(renderJson(session, opts));
  test("has schema, version and meta", () => {
    expect(out.schema).toBe("peektrace.session-export");
    expect(out.version).toBe(1);
    expect(out.redacted).toBe(true);
    expect(out.exportedAt).toBe(opts.exportedAt);
    expect(out.session.provider).toBe("claude");
    expect(out.session.models).toEqual(["claude-opus-4-1"]);
  });
  test("keeps event kinds in order, skips empty events", () => {
    expect(out.session.events.map((e: { kind: string }) => e.kind)).toEqual([
      "user-prompt",
      "assistant-text",
      "tool-call",
      "tool-result",
      "compaction",
      "user-prompt",
    ]);
  });
  test("exports no paths, turns or images", () => {
    const raw = renderJson(session, opts);
    expect(raw).not.toContain("secret-transcript");
    expect(raw).not.toContain("agent-a.jsonl");
    expect(out.session.path).toBeUndefined();
    expect(out.session.subagents[0].path).toBeUndefined();
    expect(out.session.turns).toBeUndefined();
    expect(Object.keys(out.session.subagents[0]).sort()).toEqual(
      ["agentType", "description", "id", "peakContextTokens", "turns"].sort()
    );
  });
});

describe("markdown fences and notes", () => {
  const withEvents = (events: TimelineEvent[]) =>
    renderMarkdown({ ...session, events }, opts);
  test("an unclosed fence in a chat body does not swallow later turns", () => {
    const md = withEvents([
      ev(0, "user-prompt", "hi"),
      ev(1, "assistant-text", "```ts\nconst x = 1"),
      ev(2, "user-prompt", "second prompt"),
    ]);
    const [head, tail] = md.split("### 3. User");
    expect((head?.match(/^```/gm) ?? []).length % 2).toBe(0);
    expect(tail).toContain("second prompt");
  });
  test("an inline code span starting a line is not a fence", () => {
    const md = withEvents([
      ev(
        0,
        "assistant-text",
        "```inline``` is how you write a code span.\nDone."
      ),
      ev(1, "user-prompt", "SECOND visible?"),
    ]);
    expect(md).not.toMatch(BARE_FENCE);
  });
  test("an unclosed HTML comment does not swallow later turns", () => {
    const md = withEvents([
      ev(0, "assistant-text", "Add this:\n\n<!-- TODO"),
      ev(1, "user-prompt", "THIRD"),
    ]);
    expect(md).toContain("<!-- TODO\n-->\n\n### 2. User");
    const closed = withEvents([ev(0, "assistant-text", "<!-- a -->\nx")]);
    expect(closed).not.toContain("\n-->\n\n");
  });
  test("headings number events by position, not source line", () => {
    const md = withEvents([
      ev(5, "user-prompt", "a"),
      ev(5, "assistant-text", "b"),
      ev(9, "assistant-text", "c"),
    ]);
    expect(md).toContain("### 1. User");
    expect(md).toContain("### 2. Assistant");
    expect(md).toContain("### 3. Assistant");
  });
  test("balanced fences are left alone", () => {
    const body = "a\n```ts\nx\n```\nb";
    expect(withEvents([ev(0, "assistant-text", body)])).toContain(`${body}\n`);
  });
  test("context edit notes show on chat events", () => {
    const md = withEvents([
      ev(0, "assistant-text", "x", {
        contextEdit: { status: "removed", atIndex: 9 },
      }),
    ]);
    expect(md).toContain("Dropped");
  });
});

describe("html", () => {
  const html = renderHtml(session, opts);
  test("escapes content, no script or event handler attributes", () => {
    expect(html).toContain("&lt;script&gt;alert(1)&lt;/script&gt;");
    expect(html).toContain("&quot;onerror=alert(2)");
    expect(html).not.toContain("<script");
    expect(html).not.toMatch(EVENT_HANDLER_ATTR);
    expect(html).not.toContain("<img");
  });
  test("chat turns are headings", () => {
    expect(html).toContain('<h3 class="who">User');
    expect(html).toContain('<h3 class="who">Assistant');
  });
  test("escapes the title", () => {
    expect(html).toContain("<title>Demo &lt;session&gt;</title>");
  });
  test("loads nothing external and sets CSP", () => {
    expect(html).toContain("<!doctype html>");
    expect(html).toContain("Content-Security-Policy");
    expect(html).toContain('name="referrer" content="no-referrer"');
    expect(html).not.toMatch(EXTERNAL_ATTR);
    expect(html).not.toMatch(URL_SCHEME);
  });
  test("no transcript paths", () => {
    expect(html).not.toContain("secret-transcript");
    expect(html).not.toContain("agent-a.jsonl");
    expect(html).not.toContain("/home/demo");
  });
});

describe("html dark mode", () => {
  test("error summary color is a variable with a dark value", () => {
    const html = renderHtml(session, opts);
    expect(html).toContain(".err summary{color:var(--err)}");
    expect(html).toMatch(DARK_ERR_VAR);
  });
});

describe("redacted flag", () => {
  test("not redacted keeps home paths in every format", () => {
    const raw = { ...opts, redacted: false };
    for (const format of ["md", "json", "html"] as const) {
      const out = renderExport(format, session, raw);
      expect(out).toContain("/home/demo/work");
      expect(out).not.toContain("secret-transcript");
    }
  });
});
