/** Codex model calls pair with their token_count so the context budget builds.
 *
 * The fixture follows the current rollout shape: token_count lands after the
 * call's tool output, a token_usage_record precedes it, one token_count is
 * re-emitted with an unchanged total, and one turn is aborted mid-call.
 */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { analyze } from "../../src/services/sessions/analyze";
import { parseCodexSession } from "../../src/services/sessions/parsers/codex";

const FIXTURE = join(
  import.meta.dir,
  "../fixtures/sessions-codex/rollout-budget.jsonl"
);

const FORK_FIXTURE = join(
  import.meta.dir,
  "../fixtures/sessions-codex/rollout-fork.jsonl"
);

const parseFile = (path: string) =>
  parseCodexSession({
    text: readFileSync(path, "utf8"),
    path,
    sessionId: "fallback-id",
    slug: "",
  });

const parse = () => parseFile(FIXTURE);

const sum = (slices: Record<string, number>) =>
  Object.values(slices).reduce((a, b) => a + b, 0);

describe("codex model calls", () => {
  test("one turn per token_count; the re-emitted total is skipped", () => {
    expect(parse().turns.map((t) => t.requestId)).toEqual([
      "turn-a#1",
      "turn-a#2",
      "turn-a#3",
      "turn-c#1",
      "turn-c#2",
    ]);
  });

  test("context is input_tokens; cached input is a subset of it", () => {
    const t = parse().turns[0];
    expect(t?.contextTokens).toBe(12_000);
    expect(t?.cacheReadTokens).toBe(3000);
    expect(t?.inputTokens).toBe(9000);
    // output_tokens already includes reasoning_output_tokens.
    expect(t?.outputTokens).toBe(220);
  });

  test("a call's output events carry its request id; inputs do not", () => {
    const p = parse();
    const tagged = p.events.filter((e) => e.requestId === "turn-a#1");
    expect(tagged.map((e) => e.kind)).toEqual([
      "assistant-thinking",
      "assistant-text",
      "tool-call",
    ]);
    const inputs = p.events.filter(
      (e) =>
        e.kind === "user-prompt" ||
        e.kind === "tool-result" ||
        e.kind === "attachment"
    );
    expect(inputs.every((e) => e.requestId === undefined)).toBe(true);
    for (const t of p.turns) {
      expect(t.eventIndexes.map((i) => p.events[i]?.requestId)).toEqual(
        t.eventIndexes.map(() => t.requestId)
      );
    }
  });

  test("output of an aborted call is not paired with a later call", () => {
    const aborted = parse().events.find(
      (e) => e.kind === "assistant-thinking" && e.body === "**Planning alias**"
    );
    expect(aborted).toBeDefined();
    expect(aborted?.requestId).toBeUndefined();
  });
});

describe("codex context budget", () => {
  test("every call gets a snapshot whose slices sum to its context", () => {
    const a = analyze(parse());
    expect(a.snapshots.map((s) => s.ctx)).toEqual([
      12_000, 13_100, 13_500, 13_900, 14_000,
    ]);
    for (const s of a.snapshots) {
      expect(sum(s.slices)).toBeCloseTo(s.ctx, 6);
    }
  });

  test("peak budget uses the model_context_window and is not empty", () => {
    const a = analyze(parse());
    expect(a.contextWindow).toBe(258_400);
    expect(a.contextWindowInferred).toBe(false);
    expect(a.peakContextTokens).toBe(14_000);
    expect(a.budget.length).toBeGreaterThan(0);
    const tokens = Object.fromEntries(a.budget.map((b) => [b.key, b.tokens]));
    expect(tokens.tool_results).toBeGreaterThan(0);
    expect(tokens.thinking).toBeGreaterThan(0);
  });

  test("system floor is the first call's context minus earlier inputs", () => {
    const p = parse();
    const a = analyze(p);
    const firstCall = p.events.findIndex((e) => e.requestId === "turn-a#1");
    const before = p.events
      .slice(0, firstCall)
      .filter((e) => e.kind !== "system-prompt")
      .reduce((n, e) => n + e.tokensEst, 0);
    expect(a.systemOverheadTokens).toBe(12_000 - before);
  });

  test("a turn with no events of its own still yields a peak budget", () => {
    const p = parse();
    // Drop the tags of the peak call so it has no events of its own.
    const events = p.events.map(({ requestId, ...rest }) =>
      requestId === "turn-c#2" ? rest : { ...rest, requestId }
    );
    const a = analyze({ ...p, events });
    expect(a.snapshots).toHaveLength(4);
    expect(a.budget.length).toBeGreaterThan(0);
  });
});

describe("forked codex rollout", () => {
  test("the parent's replayed token_count is not a turn", () => {
    const p = parseFile(FORK_FIXTURE);
    expect(p.turns.map((t) => t.requestId)).toEqual([
      "child-turn#1",
      "child-turn#2",
    ]);
    expect(p.turns[0]?.contextTokens).toBe(10_000);
    expect(p.turns.reduce((n, t) => n + t.outputTokens, 0)).toBe(160);
  });

  test("the first turn is the child's first call and has a snapshot", () => {
    const p = parseFile(FORK_FIXTURE);
    const first = p.turns[0];
    expect(first?.eventIndexes.map((i) => p.events[i]?.kind)).toEqual([
      "assistant-thinking",
      "tool-call",
    ]);
    const a = analyze(p);
    expect(a.snapshots.map((s) => s.turnIndex)).toEqual([0, 1]);
    expect(a.systemOverheadTokens).toBeGreaterThan(0);
  });
});
