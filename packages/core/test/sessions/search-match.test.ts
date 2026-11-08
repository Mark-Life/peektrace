import { describe, expect, test } from "bun:test";
import {
  buildSnippet,
  matchTranscript,
  queryPatterns,
} from "../../src/services/sessions/search-match";

const lines = (...rows: readonly unknown[]) =>
  rows.map((r) => JSON.stringify(r)).join("\n");

const match = (text: string, q: string, redact = false) =>
  matchTranscript({ text, q, redact, maxHits: 3 });

describe("matchTranscript", () => {
  test("ignores key names and malformed lines", () => {
    const text = `${lines({ needle: "x" })}\nneedle not json`;
    expect(match(text, "needle").matchCount).toBe(0);
  });

  test("matches text that JSON escapes", () => {
    const text = lines({ text: "line one\nline two" });
    expect(match(text, "one\nline").matchCount).toBe(1);
  });

  test("ignores base64 data and signatures", () => {
    const text = lines({ data: "QUJDREVGRw==", signature: "QUJDREVGRw==" });
    expect(match(text, "QUJDREVG").matchCount).toBe(0);
  });

  test("matches non-ASCII text case-insensitively", () => {
    const text = lines({ text: "Привет, МИР" });
    const r = match(text, "привет, мир");
    expect(r.matchCount).toBe(1);
    expect(r.hits[0]?.snippet).toBe("Привет, МИР");
  });

  test("numbers lines from 1 and counts blank lines", () => {
    const text = `\n${lines({ text: "hit" })}`;
    expect(match(text, "hit").hits[0]?.line).toBe(2);
  });
});

const snippetOf = (text: string, q: string, redact = false) => {
  const { content } = queryPatterns(q);
  return buildSnippet({ text, pattern: content, redact });
};

describe("metadata is not searched", () => {
  test("Codex session_meta git info and turn_context settings", () => {
    const text = lines(
      {
        type: "session_meta",
        payload: {
          originator: "codex_cli_rs",
          git: {
            branch: "feature/parsers",
            repository_url: "git@x:acme/r.git",
          },
        },
      },
      { type: "turn_context", payload: { effort: "medium" } }
    );
    expect(match(text, "feature/parsers").matchCount).toBe(0);
    expect(match(text, "acme/r").matchCount).toBe(0);
    expect(match(text, "medium").matchCount).toBe(0);
  });

  test("Claude usage, stop reason and permission mode", () => {
    const text = lines(
      {
        type: "assistant",
        message: {
          role: "assistant",
          stop_reason: "end_turn",
          usage: { service_tier: "standard" },
          content: [{ type: "text", text: "hi" }],
        },
      },
      { type: "user", permissionMode: "default", message: { content: "yo" } }
    );
    for (const q of ["standard", "end_turn", "default"]) {
      expect(match(text, q).matchCount).toBe(0);
    }
    expect(match(text, "hi").matchCount).toBe(1);
  });

  test("tool input paths still match", () => {
    const text = lines({
      type: "assistant",
      message: {
        content: [
          { type: "tool_use", input: { file_path: "/srv/demo/src/app.ts" } },
        ],
      },
    });
    expect(match(text, "src/app.ts").matchCount).toBe(1);
  });
});

describe("blank query", () => {
  test("matches nothing", () => {
    const text = lines({ message: { content: "abc" } });
    for (const q of ["", "   ", "\n\t"]) {
      expect(match(text, q)).toEqual({ hits: [], matchCount: 0 });
    }
  });
});

describe("buildSnippet", () => {
  const long = `${"a ".repeat(100)}NEEDLE${" b".repeat(100)}`;

  test("cuts around the match with ellipses and a correct range", () => {
    const s = snippetOf(long, "needle");
    expect(s.snippet.startsWith("…")).toBe(true);
    expect(s.snippet.endsWith("…")).toBe(true);
    expect(s.snippet.slice(s.matchStart, s.matchEnd)).toBe("NEEDLE");
    expect(s.snippet.length).toBeLessThan(180);
  });

  test("collapses whitespace", () => {
    expect(snippetOf("a\n\n  b   NEEDLE", "needle").snippet).toBe("a b NEEDLE");
  });

  test("keeps the range when q has repeated or odd whitespace", () => {
    const text = `foo  bar ${"x".repeat(400)}`;
    for (const q of ["foo  bar", "foo\tbar", "foo bar"]) {
      const s = snippetOf(text, q, true);
      expect(s.snippet.slice(s.matchStart, s.matchEnd)).toBe("foo bar");
    }
  });

  test("stays on the match when lowercasing changes string length", () => {
    const text = `${"İ".repeat(400)} needle here ${"z".repeat(300)}`;
    const r = match(lines({ message: { content: text } }), "needle");
    const hit = r.hits[0];
    expect(hit?.snippet).toContain("needle");
    expect(hit?.snippet.slice(hit.matchStart, hit.matchEnd)).toBe("needle");
  });

  test("strips terminal escape sequences and control characters", () => {
    const text = "ok \u001b[31mERROR\u001b[0m \u001b]0;pwned\u0007 done\u0000";
    const s = snippetOf(text, "error");
    expect(s.snippet).toBe("ok ERROR done");
    expect(s.snippet).toContain("ERROR");
    expect(s.snippet).not.toContain("pwned");
    expect(s.snippet.slice(s.matchStart, s.matchEnd)).toBe("ERROR");
  });

  test("drops the range when redaction removes the query", () => {
    const key = "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234";
    const s = snippetOf(`token ${key} end`, key, true);
    expect(s.matchStart).toBeUndefined();
    expect(s.snippet).not.toContain(key);
  });
});

describe("queryPatterns", () => {
  test("the raw pattern accepts the JSON-escaped form", () => {
    const { raw } = queryPatterns('a"b c');
    expect(raw.test('{"t":"a\\"b c"}')).toBe(true);
    expect(raw.test('{"t":"a\\"b\\nc"}')).toBe(true);
    expect(raw.test("nothing")).toBe(false);
  });
});

describe("redaction sees the whole line", () => {
  const body = Array.from({ length: 20 }, (_, i) =>
    `Zm9vYmFyYmF6${i}`.repeat(4)
  ).join("\n");
  const pem = `-----BEGIN PRIVATE KEY-----\n${body}\n-----END PRIVATE KEY-----`;

  test("a long PEM block is hidden when the query hits its middle", () => {
    const text = lines({ message: { content: `${pem} use it for walrus` } });
    for (const q of ["walrus", "BEGIN PRIVATE", "Zm9vYmFyYmF6"]) {
      const snippet = match(text, q, true).hits[0]?.snippet ?? "";
      expect(snippet).not.toContain("Zm9vYmFyYmF6");
      expect(snippet).not.toContain("BEGIN PRIVATE KEY");
    }
  });

  test("a long JWT is hidden when the query follows it", () => {
    const seg = (n: string) => `eyJ${n}`.padEnd(160, "A");
    const jwt = `${seg("h")}.${seg("p")}.${seg("s")}`;
    const text = lines({ message: { content: `token ${jwt} walrus` } });
    const snippet = match(text, "walrus", true).hits[0]?.snippet ?? "";
    expect(snippet).not.toContain("AAAAAAAAAA");
    expect(snippet).toContain("walrus");
  });
});

describe("metadata of other agents is not searched", () => {
  const cases: readonly (readonly [string, unknown, string])[] = [
    [
      "codex phase",
      {
        type: "response_item",
        payload: {
          phase: "commentary",
          role: "assistant",
          content: [{ type: "output_text", text: "hello" }],
        },
      },
      "commentary",
    ],
    [
      "pi provider",
      {
        type: "model_change",
        id: "m1",
        provider: "anthropic",
        modelId: "claude-x",
        parentId: null,
      },
      "anthropic",
    ],
    [
      "pi model id",
      {
        type: "model_change",
        id: "m1",
        provider: "anthropic",
        modelId: "claude-x",
      },
      "claude-x",
    ],
    [
      "pi stop reason",
      {
        type: "message",
        message: { role: "assistant", content: "ok", stopReason: "stop" },
        parentId: "pp1",
      },
      "stop",
    ],
    [
      "pi parent id",
      {
        type: "message",
        message: { role: "user", content: "ok" },
        parentId: "pp1",
      },
      "pp1",
    ],
    [
      "opencode directory",
      { kind: "session", directory: "/work/demo", projectId: "prj9" },
      "demo",
    ],
    ["opencode project id", { kind: "session", projectId: "prj9" }, "prj9"],
    ["opencode kind", { kind: "message", role: "user" }, "message"],
  ];
  for (const [name, row, q] of cases) {
    test(name, () => {
      expect(match(lines(row), q).matchCount).toBe(0);
    });
  }

  test("Codex response_item and event_msg for one message dedupe", () => {
    const text = lines(
      {
        type: "response_item",
        payload: {
          phase: "commentary",
          role: "assistant",
          content: [
            { type: "output_text", text: "Let me run the walrus test." },
          ],
        },
      },
      {
        type: "event_msg",
        payload: {
          type: "agent_message",
          message: "Let me run the walrus test.",
        },
      }
    );
    const r = match(text, "walrus");
    expect(r.hits.map((h) => h.snippet)).toEqual([
      "Let me run the walrus test.",
    ]);
    expect(r.matchCount).toBe(2);
  });
});

describe("escaped whitespace beyond newline and tab", () => {
  test("form feed and vertical tab separators match a spaced query", () => {
    const text = lines({ text: "page one\fpage two\u000bend" });
    expect(match(text, "one page").matchCount).toBe(1);
    expect(match(text, "two end").matchCount).toBe(1);
    expect(
      match(lines({ text: "tab\tsep\fformfeed walrus3" }), "sep formfeed")
        .hits[0]?.snippet
    ).toBe("tab sep formfeed walrus3");
  });
});

describe("snippet safety", () => {
  test("bidi controls become spaces", () => {
    const s = snippetOf("evil \u202e txt.exe \u2066 gnu \u200f", "gnu");
    expect(s.snippet).toBe("evil txt.exe gnu");
  });

  test("cuts never leave a lone surrogate", () => {
    const text = `emoji ${"\u{1F600}".repeat(100)} walrus5 ${"\u{1F600}".repeat(100)}`;
    for (let shift = 0; shift < 2; shift += 1) {
      const t = `${"x".repeat(shift)}${text}`;
      const { snippet } = snippetOf(t, "walrus5");
      expect(snippet.isWellFormed()).toBe(true);
    }
  });
});
