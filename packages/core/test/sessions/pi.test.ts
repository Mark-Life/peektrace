/** Pi (pi.dev) parser tests.
 *
 * Feeds a hand-crafted, hermetic Pi transcript through `parsePiSession` and
 * asserts the normalized `ParsedSession`: meta (provider/sessionId/cwd/version/
 * models), the one-Turn-per-non-zero-usage-assistant rule with exact token
 * math (`contextTokens = input + cacheRead + cacheWrite`), that the all-zero
 * error line yields no Turn, tool-call ↔ tool-result pairing by `toolUseId`,
 * empty-thinking placeholder handling, `nativeContextWindow` via the
 * model-window heuristic, and the never-throw-on-garbage guarantee.
 */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { Schema } from "effect";
import {
  buildPiHeader,
  parsePiSession,
} from "../../src/services/sessions/parsers/pi";
import { ParsedSession } from "../../src/services/sessions/schema";

const FIXTURE = join(
  import.meta.dir,
  "../fixtures/sessions-pi/pi-fixture.jsonl"
);
const text = readFileSync(FIXTURE, "utf8");

const parse = () =>
  parsePiSession({
    text,
    path: FIXTURE,
    sessionId: "fallback-id",
    slug: "--Users-demo-proj--",
  });

describe("parsePiSession — meta", () => {
  test("normalizes provider and session metadata", () => {
    const p = parse();
    expect(p.provider).toBe("pi");
    // sessionId comes from the `session` line `.id`, not the fallback param.
    expect(p.sessionId).toBe("019f3788-3bb1-7302-a2bf-55b30354d752");
    expect(p.cwd).toBe("/Users/demo/proj");
    // `.version` is a number in the transcript -> stringified.
    expect(p.version).toBe("3");
    expect(p.startedAt).toBe("2026-07-06T13:05:02.897Z");
    expect(p.endedAt).toBe("2026-07-06T13:05:40.000Z");
    expect(p.compactionIndexes).toEqual([]);
    expect(p.subagents).toEqual([]);
    expect(p.gitBranch).toBeUndefined();
    expect(p.title).toBeUndefined();
  });

  test("collects distinct models from model_change + assistant lines", () => {
    const p = parse();
    expect([...p.models].sort()).toEqual([
      "claude-opus-4-8",
      "qwen/qwen3.6-27b",
    ]);
  });

  test("output validates against the ParsedSession schema", () => {
    expect(() =>
      Schema.decodeUnknownSync(ParsedSession)(parse())
    ).not.toThrow();
  });
});

describe("parsePiSession — turns and token math", () => {
  test("one turn per assistant line with non-zero usage", () => {
    const p = parse();
    expect(p.turns).toHaveLength(2);
    expect(p.turns.map((t) => t.requestId).sort()).toEqual([
      "aaaa1111",
      "bbbb2222",
    ]);
  });

  test("token math: contextTokens = input + cacheRead + cacheWrite", () => {
    const p = parse();
    const t = p.turns.find((x) => x.requestId === "aaaa1111");
    expect(t).toBeDefined();
    if (!t) {
      return;
    }
    expect(t.inputTokens).toBe(11);
    expect(t.cacheReadTokens).toBe(5750);
    expect(t.cacheCreationTokens).toBe(0);
    expect(t.outputTokens).toBe(35);
    expect(t.contextTokens).toBe(11 + 5750 + 0);
    // Verified invariant: contextTokens == totalTokens - output (5796 - 35).
    expect(t.contextTokens).toBe(5796 - 35);
    expect(t.model).toBe("claude-opus-4-8");
    expect(t.ts).toBe("2026-07-06T13:05:20.000Z");
  });

  test("cacheWrite feeds cacheCreationTokens on the second turn", () => {
    const p = parse();
    const t = p.turns.find((x) => x.requestId === "bbbb2222");
    expect(t?.cacheCreationTokens).toBe(100);
    expect(t?.contextTokens).toBe(20 + 0 + 100);
  });

  test("the all-zero-usage error line yields no turn", () => {
    const p = parse();
    expect(p.turns.some((t) => t.requestId === "cccc3333")).toBe(false);
  });
});

describe("parsePiSession — events", () => {
  test("tool-call and tool-result pair by toolUseId + toolName", () => {
    const p = parse();
    const call = p.events.find((e) => e.kind === "tool-call");
    const result = p.events.find((e) => e.kind === "tool-result");
    expect(call?.toolUseId).toBe("call_1");
    expect(call?.toolName).toBe("read");
    expect(result?.toolUseId).toBe("call_1");
    expect(result?.toolName).toBe("read");
    expect(call?.toolUseId).toBe(result?.toolUseId);
    // Tool-call body is the pretty-printed arguments object.
    expect(call?.body).toContain("README.md");
  });

  test("tool-result body joins content text blocks", () => {
    const p = parse();
    const result = p.events.find((e) => e.kind === "tool-result");
    expect(result?.body).toContain("A demo project readme body.");
    expect(result?.isError).toBe(false);
  });

  test("empty assistant thinking uses the placeholder preview + 0 tokens", () => {
    const p = parse();
    const thinking = p.events.filter((e) => e.kind === "assistant-thinking");
    // One non-empty (rich turn) + one empty (error line).
    expect(thinking).toHaveLength(2);
    const empty = thinking.find((e) => e.body === "");
    expect(empty).toBeDefined();
    expect(empty?.tokensEst).toBe(0);
    expect(empty?.preview).toBe("(content not stored in transcript)");
  });

  test("user prompts and assistant text are captured", () => {
    const p = parse();
    expect(
      p.events.filter((e) => e.kind === "user-prompt").map((e) => e.body)
    ).toEqual(["read the readme and summarize it", "thanks, now do it again"]);
    expect(p.events.some((e) => e.kind === "assistant-text")).toBe(true);
  });
});

describe("parsePiSession — context window", () => {
  test("nativeContextWindow inferred from the last model via windowForModel", () => {
    const p = parse();
    // Last model is claude-opus-4-8 -> 200_000 window heuristic.
    expect(p.nativeContextWindow).toBe(200_000);
  });

  test("unknown-only model leaves nativeContextWindow unset", () => {
    const line = JSON.stringify({
      type: "session",
      version: 1,
      id: "s1",
      timestamp: "2026-07-06T00:00:00.000Z",
      cwd: "/tmp",
    });
    const p = parsePiSession({
      text: line,
      path: "/tmp/x.jsonl",
      sessionId: "s1",
      slug: "",
    });
    expect(p.nativeContextWindow).toBeUndefined();
  });
});

describe("parsePiSession — defensive", () => {
  test("never throws on garbage, yields empty events/turns", () => {
    const garbage = [
      "not json at all",
      '{"type":"message","message":{',
      "",
      "   ",
      '{"type":"session"',
    ].join("\n");
    const p = parsePiSession({
      text: garbage,
      path: "/tmp/garbage.jsonl",
      sessionId: "g1",
      slug: "",
    });
    expect(p.provider).toBe("pi");
    expect(p.sessionId).toBe("g1");
    expect(p.events).toEqual([]);
    expect(p.turns).toEqual([]);
    expect(() => Schema.decodeUnknownSync(ParsedSession)(p)).not.toThrow();
  });

  test("empty transcript yields a valid empty ParsedSession", () => {
    const p = parsePiSession({
      text: "",
      path: "/tmp/empty.jsonl",
      sessionId: "e1",
      slug: "",
    });
    expect(p.events).toEqual([]);
    expect(p.turns).toEqual([]);
    expect(p.models).toEqual([]);
  });
});

describe("buildPiHeader", () => {
  test("stamps the pi agent, cwd, first model and counts", () => {
    const h = buildPiHeader({
      text,
      id: "019f3788-3bb1-7302-a2bf-55b30354d752",
      slug: "--Users-demo-proj--",
      path: FIXTURE,
      sizeBytes: text.length,
      mtimeMs: 0,
    });
    expect(h.agent).toBe("pi");
    expect(h.project).toBe("--Users-demo-proj--");
    expect(h.cwd).toBe("/Users/demo/proj");
    // First model seen is the initial model_change (qwen).
    expect(h.model).toBe("qwen/qwen3.6-27b");
    expect(h.messageCount).toBe(11);
    expect(h.startedAt).toBe("2026-07-06T13:05:02.897Z");
    expect(h.sizeBytes).toBeGreaterThan(0);
  });

  test("tolerates a fully garbage transcript", () => {
    const h = buildPiHeader({
      text: "garbage\n{broken\n\n!!!",
      id: "h",
      slug: "",
      path: "/tmp/x.jsonl",
      sizeBytes: 20,
      mtimeMs: 0,
    });
    expect(h.id).toBe("h");
    expect(h.model).toBeUndefined();
    expect(h.messageCount).toBe(3);
  });
});
