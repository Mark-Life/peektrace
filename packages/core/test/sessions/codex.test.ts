/** Unit tests for the Codex CLI rollout parser.
 *
 * Feeds a hermetic hand-crafted rollout fixture (session_meta + turn_context +
 * response_items + two info-bearing token_count lines) through the parser and
 * asserts meta extraction, exact per-turn token math, the authoritative context
 * window, encrypted-reasoning handling, tool-call/result pairing, and the
 * never-throw-on-garbage guarantee.
 */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  buildCodexHeader,
  codexParser,
  parseCodexSession,
} from "../../src/services/sessions/parsers/codex";

const FIXTURE = join(
  import.meta.dir,
  "../fixtures/sessions-codex/rollout-fixture.jsonl"
);
const TEXT = readFileSync(FIXTURE, "utf8");
const SESSION_ID = "019f42f4-342d-7b30-9551-a78e5a2cbe5e";

const parse = () =>
  parseCodexSession({
    text: TEXT,
    path: FIXTURE,
    sessionId: "fallback-id",
    slug: "",
  });

describe("parseCodexSession (meta)", () => {
  test("extracts provider + session metadata from session_meta", () => {
    const p = parse();
    expect(p.provider).toBe("codex");
    expect(p.sessionId).toBe(SESSION_ID);
    expect(p.cwd).toBe("/Users/andrey-m/Code/personal/peephole");
    expect(p.gitBranch).toBe("feature/parsers");
    expect(p.version).toBe("0.134.0");
    expect(p.models).toEqual(["gpt-5.5"]);
    expect(p.title).toBeUndefined();
    expect(p.compactionIndexes).toEqual([]);
    expect(p.subagents).toEqual([]);
    expect(p.startedAt).toBe("2026-07-08T10:00:00.000Z");
    expect(p.endedAt).toBe("2026-07-08T10:00:12.000Z");
  });

  test("nativeContextWindow comes straight from model_context_window", () => {
    expect(parse().nativeContextWindow).toBe(258_400);
  });
});

describe("parseCodexSession (turns)", () => {
  test("one turn per info-bearing token_count; skips the leading info:null", () => {
    const p = parse();
    expect(p.turns).toHaveLength(2);
    expect(p.turns.map((t) => t.requestId)).toEqual([
      "turn-aaa#1",
      "turn-aaa#2",
    ]);
  });

  test("turn[0] uses last_token_usage (delta), not the cumulative total", () => {
    const t = parse().turns[0];
    // last_token_usage: input 13917, cached 11648, output 201.
    expect(t.contextTokens).toBe(13_917);
    expect(t.cacheReadTokens).toBe(11_648);
    expect(t.inputTokens).toBe(13_917 - 11_648);
    expect(t.cacheCreationTokens).toBe(0);
    expect(t.outputTokens).toBe(201);
    expect(t.model).toBe("gpt-5.5");
    expect(t.ts).toBe("2026-07-08T10:00:06.000Z");
    // Invariant: contextTokens == input + cacheRead + cacheCreation.
    expect(t.inputTokens + t.cacheReadTokens + t.cacheCreationTokens).toBe(
      t.contextTokens
    );
    // response_item events produced before this token_count.
    expect(t.eventIndexes).toEqual([0, 1, 2, 3, 4]);
  });

  test("turn[1] carries its own delta and the events since the prior turn", () => {
    const t = parse().turns[1];
    expect(t.contextTokens).toBe(15_200);
    expect(t.cacheReadTokens).toBe(13_000);
    expect(t.inputTokens).toBe(2200);
    expect(t.outputTokens).toBe(300);
    expect(t.eventIndexes).toEqual([5, 6, 7]);
  });
});

describe("parseCodexSession (events)", () => {
  test("timeline is built from response_item only", () => {
    const kinds = parse().events.map((e) => e.kind);
    expect(kinds).toEqual([
      "user-prompt",
      "system",
      "assistant-thinking",
      "assistant-text",
      "tool-call",
      "tool-result",
      "system",
      "assistant-text",
      "tool-call",
      "tool-result",
    ]);
  });

  test("injected <environment_context> user message becomes a system event", () => {
    const sys = parse().events[1];
    expect(sys.kind).toBe("system");
    expect(sys.attachmentType).toBe("environment_context");
  });

  test("assistant-thinking has empty body + encrypted placeholder + 0 tokens", () => {
    const thinking = parse().events.find(
      (e) => e.kind === "assistant-thinking"
    );
    expect(thinking?.body).toBe("");
    expect(thinking?.tokensEst).toBe(0);
    expect(thinking?.preview).toBe("(reasoning encrypted — not stored)");
  });

  test("tool-call / tool-result pair by toolUseId (call_id)", () => {
    const p = parse();
    const call = p.events.find(
      (e) => e.kind === "tool-call" && e.toolName === "exec_command"
    );
    const result = p.events.find(
      (e) => e.kind === "tool-result" && e.toolUseId === "call_1"
    );
    expect(call?.toolUseId).toBe("call_1");
    expect(result?.toolUseId).toBe(call?.toolUseId);
    // "Process exited with code 2" trips the isError heuristic.
    expect(result?.isError).toBe(true);
  });

  test("custom_tool_call (apply_patch) pairs with its output; no false error", () => {
    const p = parse();
    const call = p.events.find(
      (e) => e.kind === "tool-call" && e.toolName === "apply_patch"
    );
    const result = p.events.find(
      (e) => e.kind === "tool-result" && e.toolUseId === "call_2"
    );
    expect(call?.toolUseId).toBe("call_2");
    expect(result?.body).toContain("Success");
    expect(result?.isError).toBe(false);
  });
});

describe("buildCodexHeader", () => {
  test("derives project from cwd basename and stamps agent codex", () => {
    const h = buildCodexHeader({
      text: TEXT,
      id: SESSION_ID,
      slug: "",
      path: FIXTURE,
      sizeBytes: TEXT.length,
      mtimeMs: 0,
    });
    expect(h.agent).toBe("codex");
    expect(h.project).toBe("peephole");
    expect(h.cwd).toBe("/Users/andrey-m/Code/personal/peephole");
    expect(h.gitBranch).toBe("feature/parsers");
    expect(h.model).toBe("gpt-5.5");
    expect(h.messageCount).toBe(15);
    expect(h.startedAt).toBe("2026-07-08T10:00:00.000Z");
    expect(h.sizeBytes).toBeGreaterThan(0);
  });
});

describe("codexParser (defensive)", () => {
  test("codexParser exposes the codex agent id", () => {
    expect(codexParser.agent).toBe("codex");
  });

  test("never throws on a garbage / empty transcript", () => {
    const broken = [
      "not json at all",
      '{"type":"response_item","payload":{',
      "",
      "   ",
      '{"type":"event_msg","payload":{"type":"token_count"',
    ].join("\n");
    const p = parseCodexSession({
      text: broken,
      path: "/tmp/broken.jsonl",
      sessionId: "broken",
      slug: "",
    });
    expect(p.provider).toBe("codex");
    expect(p.sessionId).toBe("broken");
    expect(p.events).toEqual([]);
    expect(p.turns).toEqual([]);
    expect(p.nativeContextWindow).toBeUndefined();

    const empty = parseCodexSession({
      text: "",
      path: "/tmp/empty.jsonl",
      sessionId: "empty",
      slug: "",
    });
    expect(empty.events).toEqual([]);
    expect(empty.turns).toEqual([]);
  });
});
