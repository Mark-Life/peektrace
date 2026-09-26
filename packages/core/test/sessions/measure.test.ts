/** Measured sizes: the growth in prompt size between two model calls goes to
 * the events added between them, and the budget uses it over chars/4.
 *
 * The Claude fixture mirrors a real screenshot session: a 25210 -> 26846 step
 * with 57 output tokens, where the image marker alone estimates ~11 tokens.
 */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { analyze } from "../../src/services/sessions/analyze";
import { measureTokens } from "../../src/services/sessions/measure";
import { parseClaudeSession } from "../../src/services/sessions/parse";
import { parseCodexSession } from "../../src/services/sessions/parsers/codex";
import type {
  ParsedSession,
  TimelineEvent,
} from "../../src/services/sessions/schema";
import { eventTokens, tokenLabel } from "../../src/services/sessions/tokens";

const fixture = (dir: string, name: string) =>
  join(import.meta.dir, "../fixtures", dir, name);

const claude = () => {
  const path = fixture("sessions-claude", "measured-image.jsonl");
  return parseClaudeSession({
    text: readFileSync(path, "utf8"),
    path,
    sessionId: "measured",
  });
};

const codex = () => {
  const path = fixture("sessions-codex", "rollout-budget.jsonl");
  return parseCodexSession({
    text: readFileSync(path, "utf8"),
    path,
    sessionId: "fallback-id",
    slug: "",
  });
};

const at = (p: ParsedSession, index: number) =>
  p.events.filter((e) => e.index === index);

describe("measured sizes on a Claude session", () => {
  test("a lone screenshot result gets the whole step", () => {
    const [shot] = at(claude(), 2);
    // 26846 - 25210 - 57
    expect(shot?.tokensMeasured).toBe(1579);
    expect(shot?.tokensEst).toBeLessThan(20);
  });

  test("a prompt with an image splits the step, image weighted", () => {
    const [text, image] = at(claude(), 4);
    // 28500 - 26846 - 40
    expect((text?.tokensMeasured ?? 0) + (image?.tokensMeasured ?? 0)).toBe(
      1614
    );
    expect(image?.tokensMeasured).toBeGreaterThan(1500);
    expect(text?.tokensMeasured).toBeLessThan(50);
  });

  test("a step with subagent lines keeps the estimate", () => {
    const [result] = at(claude(), 8);
    expect(result?.kind).toBe("tool-result");
    expect(result?.tokensMeasured).toBeUndefined();
  });

  test("steps across compaction or with a negative delta keep estimates", () => {
    const p = claude();
    expect(at(p, 10)[0]?.tokensMeasured).toBeUndefined();
    expect(at(p, 12)[0]?.tokensMeasured).toBeUndefined();
  });

  test("model output never gets a measured size", () => {
    const outputs = claude().events.filter((e) => e.requestId);
    expect(outputs.every((e) => e.tokensMeasured === undefined)).toBe(true);
  });

  test("the budget uses measured sizes, so less is unattributed", () => {
    const p = claude();
    const stripped = {
      ...p,
      events: p.events.map(({ tokensMeasured: _, ...e }) => e),
    };
    const unattributed = (s: ParsedSession) =>
      analyze(s).snapshots.find((x) => x.turnIndex === 2)?.slices
        .unattributed ?? 0;
    expect(unattributed(stripped)).toBeGreaterThan(3000);
    expect(unattributed(p)).toBe(0);
  });
});

describe("measured sizes on a Codex rollout", () => {
  test("tool output gets input growth minus the call's output", () => {
    const p = codex();
    // 13100 - 12000 - 220, and 13500 - 13100 - 140
    expect(at(p, 11)[0]?.tokensMeasured).toBe(880);
    expect(at(p, 15)[0]?.tokensMeasured).toBe(260);
  });

  test("a step holding an aborted call's output keeps estimates", () => {
    const p = codex();
    expect(at(p, 24)[0]?.tokensMeasured).toBeUndefined();
    expect(at(p, 29)[0]?.tokensMeasured).toBeUndefined();
  });
});

const ev = (e: Partial<TimelineEvent> & Pick<TimelineEvent, "index">) =>
  ({
    kind: "tool-result",
    title: "",
    preview: "",
    body: "x".repeat(400),
    tokensEst: 100,
    ...e,
  }) satisfies TimelineEvent;

const turn = (requestId: string, contextTokens: number, outputTokens = 0) => ({
  requestId,
  model: "m",
  contextTokens,
  inputTokens: contextTokens,
  cacheReadTokens: 0,
  cacheCreationTokens: 0,
  outputTokens,
  eventIndexes: [],
});

const session = (
  events: TimelineEvent[],
  turns: ReturnType<typeof turn>[]
): ParsedSession => ({
  provider: "pi",
  sessionId: "s",
  path: "p",
  models: [],
  events,
  turns,
  compactionIndexes: [],
  subagents: [],
});

describe("measureTokens rules", () => {
  const call = (index: number, requestId: string) =>
    ev({ index, kind: "assistant-text", requestId, tokensEst: 5 });

  test("splits by estimate and keeps the sum exact", () => {
    const p = measureTokens(
      session(
        [
          call(0, "a"),
          ev({ index: 1, tokensEst: 100 }),
          ev({ index: 2, tokensEst: 200 }),
          call(3, "b"),
        ],
        [turn("a", 1000, 10), turn("b", 1611)]
      )
    );
    const [one, two] = [at(p, 1)[0], at(p, 2)[0]];
    expect(one?.tokensMeasured).toBe(200);
    expect(two?.tokensMeasured).toBe(401);
  });

  test("a small delta over many equal events never goes negative", () => {
    const results = [1, 2, 3, 4, 5, 6].map((index) => ev({ index }));
    const p = measureTokens(
      session(
        [call(0, "a"), ...results, call(7, "b")],
        [turn("a", 100, 10), turn("b", 113)]
      )
    );
    const shares = results.map((r) => at(p, r.index)[0]?.tokensMeasured);
    expect(shares).toEqual([1, 1, 1, 0, 0, 0]);
  });

  test("a context edit taking effect inside the step skips it", () => {
    const edited = ev({
      index: 1,
      contextEdit: { status: "removed", atIndex: 2 },
    });
    const p = measureTokens(
      session(
        [call(0, "a"), edited, ev({ index: 2 }), call(3, "b")],
        [turn("a", 1000), turn("b", 1300)]
      )
    );
    expect(p.events.some((e) => e.tokensMeasured !== undefined)).toBe(false);
  });

  test("a system prompt change inside the step skips it", () => {
    const p = measureTokens(
      session(
        [
          call(0, "a"),
          ev({ index: 1, kind: "system-prompt" }),
          ev({ index: 2 }),
          call(3, "b"),
        ],
        [turn("a", 1000), turn("b", 1300)]
      )
    );
    expect(at(p, 2)[0]?.tokensMeasured).toBeUndefined();
  });
});

describe("token labels", () => {
  test("estimates carry ~, measured sizes do not", () => {
    const fmt = (n: number) => `${n}`;
    expect(tokenLabel({ tokensEst: 5 }, fmt)).toBe("~5");
    expect(tokenLabel({ tokensEst: 5, tokensMeasured: 1579 }, fmt)).toBe(
      "1579"
    );
    expect(eventTokens({ tokensEst: 5, tokensMeasured: 0 })).toEqual({
      tokens: 0,
      measured: true,
    });
  });
});
