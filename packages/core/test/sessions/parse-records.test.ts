import { describe, expect, test } from "bun:test";
import { parseClaudeSession } from "../../src/services/sessions/parse";

const parse = (rows: readonly object[]) =>
  parseClaudeSession({
    path: "/demo/s.jsonl",
    sessionId: "s",
    text: rows.map((r) => JSON.stringify(r)).join("\n"),
  });

describe("parse summary and system rows", () => {
  test("summary line becomes a rolling-summary event", () => {
    const { events } = parse([
      { type: "summary", summary: "Fixed the build\nmore", timestamp: "t1" },
    ]);
    const [e] = events.filter((x) => x.kind === "summary");
    expect(e?.title).toBe("Rolling summary");
    expect(e?.preview).toBe("Fixed the build");
    expect(e?.body).toBe("Fixed the build\nmore");
    expect(e?.tokensEst).toBeGreaterThan(0);
  });

  test("generic system row keeps subtype in the title", () => {
    const { events } = parse([
      { type: "system", subtype: "demo_notice", content: "heads up" },
    ]);
    const [e] = events.filter((x) => x.kind === "system");
    expect(e?.title).toBe("system: demo_notice");
    expect(e?.body).toBe("heads up");
    expect(e?.tokensEst).toBe(0);
  });
});

describe("advisor results", () => {
  test("advisor result, plain and redacted", () => {
    const { events } = parse([
      {
        type: "assistant",
        requestId: "r1",
        message: {
          id: "m1",
          content: [
            {
              type: "advisor_tool_result",
              tool_use_id: "u1",
              content: { type: "advisor_result", text: "try X" },
            },
            {
              type: "advisor_tool_result",
              tool_use_id: "u2",
              content: { type: "advisor_redacted_result" },
            },
          ],
        },
      },
    ]);
    const results = events.filter((x) => x.title === "advisor result");
    expect(results.map((r) => [r.toolUseId, r.body, r.tokensEst > 0])).toEqual([
      ["u1", "try X", true],
      ["u2", "(advisor response encrypted, not stored in transcript)", false],
    ]);
  });
});

const fetchResult = (content: object) =>
  parse([
    {
      type: "assistant",
      requestId: "r2",
      message: {
        id: "m2",
        content: [
          { type: "web_fetch_tool_result", tool_use_id: "f1", content },
        ],
      },
    },
  ]).events.find((x) => x.toolUseId === "f1" && x.title.endsWith("result"));

describe("web_fetch results", () => {
  test("text source is appended under the url and title", () => {
    const e = fetchResult({
      type: "web_fetch_result",
      url: "https://example.test/doc",
      content: { title: "Doc", source: { type: "text", data: "body" } },
    });
    expect(e?.body).toBe("https://example.test/doc — Doc\n\nbody");
  });

  test("no text source keeps only the head", () => {
    const e = fetchResult({
      type: "web_fetch_result",
      url: "https://example.test/doc",
      content: { title: "Doc", source: { type: "base64", data: "AAAA" } },
    });
    expect(e?.body).toBe("https://example.test/doc — Doc");
  });
});
