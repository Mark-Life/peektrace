/** Per-session cost and the generation-keyed price table. Synthetic fixtures only. */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import {
  estimateCost,
  priceFor,
  pricingEcho,
  turnCost,
} from "../../src/services/pricing";
import { analyze } from "../../src/services/sessions/analyze";
import {
  costSummary,
  fmtUsd,
  turnsCost,
} from "../../src/services/sessions/cost";
import { buildHeader } from "../../src/services/sessions/header";
import { parseClaudeSession } from "../../src/services/sessions/parse";
import {
  buildOpencodeHeader,
  parseOpencodeSession as parseOpencode,
} from "../../src/services/sessions/parsers/opencode";
import { parsePiSession } from "../../src/services/sessions/parsers/pi";
import { buildPiHeader } from "../../src/services/sessions/parsers/pi-header";

const line = (o: Record<string, unknown>) => JSON.stringify(o);

const usage = (input: number, output = 0) => ({
  input_tokens: input,
  output_tokens: output,
  cache_read_input_tokens: 0,
  cache_creation_input_tokens: 0,
});

const claudeText = (u: Record<string, unknown>, model: string) =>
  [
    line({
      type: "user",
      timestamp: "2026-01-01T00:00:00Z",
      message: { content: "hi" },
    }),
    line({
      type: "assistant",
      timestamp: "2026-01-01T00:00:01Z",
      requestId: "r1",
      message: { model, usage: u, content: [{ type: "text", text: "ok" }] },
    }),
  ].join("\n");

const claudeSession = (u: Record<string, unknown>, model: string) =>
  parseClaudeSession({
    text: claudeText(u, model),
    path: "/synthetic/s.jsonl",
    sessionId: "s",
  });

const turn = (
  model: string,
  tokens: Partial<Parameters<typeof turnCost>[0]>
) => ({
  model,
  inputTokens: 0,
  outputTokens: 0,
  cacheReadTokens: 0,
  cacheCreationTokens: 0,
  ...tokens,
});

/** OpenCode session whose own cost is `cost`, one gpt-5.5 call with reasoning. */
const opencodeText = (cost: number, messageCost = 0) =>
  [
    line({
      kind: "session",
      id: "ses_x",
      directory: "/synthetic/proj",
      model: { id: "gpt-5.5", providerID: "openai" },
      cost,
    }),
    line({
      kind: "message",
      role: "assistant",
      id: "m1",
      modelID: "gpt-5.5",
      cost: messageCost,
      tokens: {
        input: 1000,
        output: 100,
        reasoning: 900,
        cache: { read: 0, write: 0 },
      },
    }),
  ].join("\n");

/** gpt-5.5: 1000 input at $5 plus (100 output + 900 reasoning) at $30, per million. */
const OPENCODE_EST = (1000 * 5 + 1000 * 30) / 1e6;

const HEADER_ARGS = {
  id: "s",
  slug: "demo",
  path: "/synthetic/s.jsonl",
  sizeBytes: 1,
  mtimeMs: 0,
};

describe("price table keyed by generation", () => {
  test("opus 4.8 is $5/$25, not the old opus-family $15/$75", () => {
    const { rates, priced } = priceFor("claude-opus-4-8");
    expect(priced).toBe(true);
    expect(rates.input).toBe(5);
    expect(rates.output).toBe(25);
  });

  test("dated opus 4.1 and opus 4 ids keep $15", () => {
    expect(priceFor("claude-opus-4-1-20250805").rates.input).toBe(15);
    expect(priceFor("claude-opus-4-20250514").rates.input).toBe(15);
  });

  test("a bare family alias gets the family's newest card", () => {
    expect(priceFor("claude-opus").key).toBe("opus-5-5");
    expect(priceFor("anthropic/claude-sonnet-latest").key).toBe("sonnet-5-5");
  });

  test("an id naming a version the table lacks stays unpriced", () => {
    for (const id of [
      "claude-opus-4-9",
      "claude-opus-4-1-thinking",
      "claude-sonnet-4-6-fast",
      "claude-3-opus-20240229",
      "claude-3-5-haiku-20241022",
      "claude-3-7-sonnet-latest",
    ]) {
      expect(priceFor(id).priced).toBe(false);
    }
  });

  test("sonnet 5.5 is $2/$10", () => {
    expect(priceFor("claude-sonnet-5-5").rates.input).toBe(2);
    expect(priceFor("claude-sonnet-5.5").rates.input).toBe(2);
  });

  test("the most specific codex key wins", () => {
    expect(priceFor("gpt-5.1-codex").rates.input).toBe(1.25);
    expect(priceFor("gpt-5.1-codex-mini").rates.input).toBe(0.25);
    expect(priceFor("gpt-5.1-codex-max").key).toBe("gpt-5.1-codex-max");
  });

  test("provider prefixes and Bedrock versions still match", () => {
    expect(priceFor("anthropic/claude-sonnet-4-6").priced).toBe(true);
    expect(priceFor("us.anthropic.claude-opus-4-1-20250805-v1:0").key).toBe(
      "opus-4-1"
    );
    expect(priceFor("openai/gpt-5.5").key).toBe("gpt-5.5");
    expect(priceFor("claude-opus-4-8[1m]").key).toBe("opus-4-8");
  });

  test("fast mode and codex-spark have no verified rate", () => {
    expect(priceFor("gpt-5.6-terra-fast").priced).toBe(false);
    expect(priceFor("gpt-5.3-codex-spark").priced).toBe(false);
  });

  test("the echo lists only the cards used, with the check date", () => {
    const echo = pricingEcho(["claude-opus-5", "example-model-1"]);
    expect(echo).toBe("opus-5 5/25/6.25/0.5 (checked 2026-11-08)");
  });
});

describe("turn cost", () => {
  test("1h writes bill at the 1h rate, the rest at the 5m rate", () => {
    const t = turn("claude-sonnet-4-6", {
      cacheCreationTokens: 2_000_000,
      cacheCreation1hTokens: 1_000_000,
      cacheCreation5mTokens: 1_000_000,
    });
    expect(estimateCost(t, priceFor(t.model).rates)).toBeCloseTo(9.75, 6);
    expect(turnCost(t)).toBeCloseTo(9.75, 6);
  });

  test("without the split every write bills at the 5m rate", () => {
    const t = turn("claude-sonnet-4-6", { cacheCreationTokens: 1_000_000 });
    expect(turnCost(t)).toBeCloseTo(3.75, 6);
  });

  test("haiku 5.5 bills the whole request at the higher tier above 100k", () => {
    expect(
      turnCost(turn("claude-haiku-5-5", { inputTokens: 99_000 }))
    ).toBeCloseTo(0.0099, 8);
    expect(
      turnCost(turn("claude-haiku-5-5", { inputTokens: 101_000 }))
    ).toBeCloseTo(0.0505, 8);
  });

  test("gpt-5.5 bills the long-context tier above 272k", () => {
    expect(turnCost(turn("gpt-5.5", { inputTokens: 200_000 }))).toBeCloseTo(
      1,
      8
    );
    expect(turnCost(turn("gpt-5.5", { inputTokens: 300_000 }))).toBeCloseTo(
      3,
      8
    );
  });

  test("an unknown model has no cost; a zero-usage turn costs 0", () => {
    expect(
      turnCost(turn("example-model-1", { inputTokens: 10 }))
    ).toBeUndefined();
    expect(turnCost(turn("<synthetic>", {}))).toBe(0);
  });
});

describe("AnalyzedSession cost", () => {
  test("analyze prices each turn and totals the session", () => {
    const a = analyze(claudeSession(usage(1_000_000), "claude-opus-4-8"));
    expect(a.turns[0]?.costUsd).toBeCloseTo(5, 6);
    expect(a.cost?.totalUsd).toBeCloseTo(5, 6);
    expect(a.cost?.source).toBe("estimate");
    expect(a.cost?.unpricedModels).toBeUndefined();
  });

  test("the total includes subagent cost, also shown apart", () => {
    const p = claudeSession(usage(1_000_000), "claude-opus-4-8");
    const a = analyze({
      ...p,
      subagents: [
        {
          id: "agent-a",
          path: "/synthetic/agent-a.jsonl",
          turns: 1,
          peakContextTokens: 10,
          costUsd: 0.5,
        },
      ],
    });
    expect(a.cost?.totalUsd).toBeCloseTo(5.5, 6);
    expect(a.cost?.subagentsUsd).toBeCloseTo(0.5, 6);
  });

  test("an unreadable subagent is counted, not silently dropped", () => {
    const p = claudeSession(usage(1_000_000), "claude-opus-4-8");
    const a = analyze({
      ...p,
      subagents: [
        {
          id: "agent-a",
          path: "/synthetic/agent-a.jsonl",
          turns: 0,
          peakContextTokens: 0,
          unreadable: true,
        },
      ],
    });
    expect(a.cost?.totalUsd).toBeCloseTo(5, 6);
    expect(a.cost?.unreadableSubagents).toBe(1);
  });

  test("inline sidechain turns are priced into the subagent share", () => {
    const sidechain = line({
      type: "assistant",
      requestId: "r9",
      isSidechain: true,
      message: { model: "claude-sonnet-4-5", usage: usage(1_000_000) },
    });
    const p = parseClaudeSession({
      text: [claudeText(usage(1_000_000), "claude-sonnet-4-5"), sidechain].join(
        "\n"
      ),
      path: "/synthetic/s.jsonl",
      sessionId: "s",
    });
    expect(p.turns).toHaveLength(1);
    const a = analyze(p);
    expect(a.cost?.totalUsd).toBeCloseTo(6, 6);
    expect(a.cost?.subagentsUsd).toBeCloseTo(3, 6);
  });

  test("a subagent's cost fields come from its turns", () => {
    expect(
      turnsCost([
        turn("claude-opus-4-8", { inputTokens: 1_000_000 }),
        turn("example-model-1", { inputTokens: 5 }),
      ])
    ).toEqual({ costUsd: 5, unpricedModels: ["example-model-1"] });
  });

  test("an unknown model leaves the total absent and names the model", () => {
    const a = analyze(claudeSession(usage(1000), "example-model-1"));
    expect(a.cost?.totalUsd).toBeUndefined();
    expect(a.cost?.unpricedModels).toEqual(["example-model-1"]);
  });

  test("a <synthetic> zero-usage turn does not mark the session unpriced", () => {
    const p = claudeSession(usage(1000), "claude-sonnet-4-6");
    const synthetic = {
      ...p.turns[0],
      requestId: "r2",
      model: "<synthetic>",
      contextTokens: 0,
      inputTokens: 0,
      outputTokens: 0,
      cacheReadTokens: 0,
      cacheCreationTokens: 0,
      eventIndexes: [],
    };
    const a = analyze({ ...p, turns: [...p.turns, synthetic] });
    expect(a.cost?.unpricedModels).toBeUndefined();
    expect(a.cost?.totalUsd).toBeCloseTo(0.003, 8);
  });

  test("OpenCode's own session cost is authoritative", () => {
    const fixture = join(
      import.meta.dir,
      "../fixtures/sessions-opencode/session-fixture.jsonl"
    );
    const p = parseOpencode({
      text: readFileSync(fixture, "utf8"),
      path: `${fixture}#ses_test123`,
      sessionId: "fallback-id",
      slug: "/Users/demo/proj",
    });
    const a = analyze(p);
    expect(a.cost?.source).toBe("agent");
    expect(a.cost?.totalUsd).toBe(0.01);
  });

  test("OpenCode with no recorded cost bills reasoning at the output rate", () => {
    const a = analyze(
      parseOpencode({
        text: opencodeText(0),
        path: "/synthetic/oc.jsonl#ses_x",
        sessionId: "ses_x",
      })
    );
    expect(a.cost?.source).toBe("estimate");
    expect(a.turns[0]?.costUsd).toBeCloseTo(OPENCODE_EST, 8);
    expect(a.cost?.totalUsd).toBeCloseTo(OPENCODE_EST, 8);
  });
});

describe("OpenCode message costs", () => {
  const parse = (text: string) =>
    analyze(
      parseOpencode({ text, path: "/synthetic/oc.jsonl#ses_x", sessionId: "x" })
    );

  test("every billed message's own cost is the agent's figure", () => {
    const a = parse(opencodeText(0, 0.5));
    expect(a.cost).toEqual({ source: "agent", totalUsd: 0.5 });
  });

  test("a message without its own cost keeps the session an estimate", () => {
    const priced = opencodeText(0, 0.5);
    const unpriced = line({
      kind: "message",
      role: "assistant",
      id: "m2",
      modelID: "gpt-5.5",
      cost: 0,
      tokens: { input: 1000, output: 100, reasoning: 900, cache: {} },
    });
    const a = parse([priced, unpriced].join("\n"));
    expect(a.cost?.source).toBe("estimate");
    expect(a.cost?.totalUsd).toBeCloseTo(0.5 + OPENCODE_EST, 8);
  });

  test("the list header uses the same figure", () => {
    const h = buildOpencodeHeader({
      ...HEADER_ARGS,
      path: "/synthetic/oc.jsonl#ses_x",
      text: opencodeText(0, 0.5),
    });
    expect(h.costUsd).toBe(0.5);
  });
});

describe("header cost", () => {
  test("Claude: one request streamed over two lines counts once; inline sidechains count", () => {
    const assistant = (output: number, text: string) =>
      line({
        type: "assistant",
        requestId: "r1",
        message: {
          model: "claude-opus-4-8",
          usage: usage(1_000_000, output),
          content: [{ type: "text", text }],
        },
      });
    const sidechain = line({
      type: "assistant",
      requestId: "r9",
      isSidechain: true,
      message: { model: "claude-opus-4-8", usage: usage(1_000_000) },
    });
    const h = buildHeader({
      ...HEADER_ARGS,
      text: [assistant(10, "a"), assistant(1_000_000, "b"), sidechain].join(
        "\n"
      ),
    });
    expect(h.costUsd).toBeCloseTo(5 + 25 + 5, 6);
    expect(h.unpricedModels).toBeUndefined();
  });

  test("Claude: an unpriced model sets the flag", () => {
    const h = buildHeader({
      ...HEADER_ARGS,
      text: claudeText(usage(1000), "example-model-1"),
    });
    expect(h.costUsd).toBeUndefined();
    expect(h.unpricedModels).toEqual(["example-model-1"]);
  });

  test("Pi: a repeated message id counts once", () => {
    const message = line({
      type: "message",
      id: "m1",
      message: {
        role: "assistant",
        model: "claude-sonnet-4-6",
        usage: { input: 1_000_000, output: 0, cacheRead: 0, cacheWrite: 0 },
        content: [],
      },
    });
    const h = buildPiHeader({
      ...HEADER_ARGS,
      text: [message, message].join("\n"),
    });
    expect(h.costUsd).toBeCloseTo(3, 6);
  });

  test("OpenCode: session cost wins, else messages priced like analyze", () => {
    const args = { ...HEADER_ARGS, path: "/synthetic/oc.jsonl#ses_x" };
    expect(
      buildOpencodeHeader({ ...args, text: opencodeText(0.5) }).costUsd
    ).toBe(0.5);
    expect(
      buildOpencodeHeader({ ...args, text: opencodeText(0) }).costUsd
    ).toBeCloseTo(OPENCODE_EST, 8);
  });

  test("Claude and Pi headers match analyze on the same transcript", () => {
    const split = {
      ...usage(2000, 300),
      cache_read_input_tokens: 50_000,
      cache_creation_input_tokens: 4000,
      cache_creation: {
        ephemeral_1h_input_tokens: 3000,
        ephemeral_5m_input_tokens: 1000,
      },
    };
    const claude = [
      claudeText(split, "claude-opus-4-8"),
      line({
        type: "assistant",
        requestId: "r1",
        message: {
          model: "claude-opus-4-8",
          usage: { ...split, output_tokens: 900 },
        },
      }),
      line({
        type: "assistant",
        requestId: "r2",
        isSidechain: true,
        message: { model: "claude-haiku-4-5", usage: usage(7000, 50) },
      }),
      line({
        type: "assistant",
        requestId: "r3",
        message: { model: "<synthetic>", usage: usage(0) },
      }),
    ].join("\n");
    const a = analyze(
      parseClaudeSession({
        text: claude,
        path: "/synthetic/s.jsonl",
        sessionId: "s",
      })
    );
    expect(buildHeader({ ...HEADER_ARGS, text: claude }).costUsd).toBeCloseTo(
      a.cost?.totalUsd ?? Number.NaN,
      10
    );

    const piMessage = (id: string, input: number) =>
      line({
        type: "message",
        id,
        timestamp: "2026-01-01T00:00:01Z",
        message: {
          role: "assistant",
          model: "claude-sonnet-4-6",
          usage: { input, output: 20, cacheRead: 900, cacheWrite: 100 },
          content: [{ type: "text", text: "ok" }],
        },
      });
    const pi = [
      piMessage("m1", 500),
      piMessage("m1", 9999),
      piMessage("m2", 0),
    ].join("\n");
    const piSession = analyze(
      parsePiSession({ text: pi, path: "/synthetic/p.jsonl", sessionId: "p" })
    );
    expect(buildPiHeader({ ...HEADER_ARGS, text: pi }).costUsd).toBeCloseTo(
      piSession.cost?.totalUsd ?? Number.NaN,
      10
    );
  });
});

describe("cost formatting", () => {
  test("fmtUsd rounds to cents and marks sub-cent spend", () => {
    expect(fmtUsd(0)).toBe("~$0.00");
    expect(fmtUsd(0.001)).toBe("<$0.01");
    expect(fmtUsd(1.234)).toBe("~$1.23");
  });

  test("costSummary lists only the qualifiers that are set", () => {
    expect(costSummary({ source: "estimate", totalUsd: 1 }, "X").notes).toEqual(
      []
    );
    expect(
      costSummary(
        {
          source: "agent",
          totalUsd: 1,
          subagentsUsd: 0.25,
          unpricedModels: ["mystery-a", "mystery-b"],
          unreadableSubagents: 2,
        },
        "OpenCode"
      ).notes
    ).toEqual([
      "reported by OpenCode",
      "subagents ~$0.25",
      "unpriced: mystery-a, mystery-b",
      "2 subagent(s) unreadable",
    ]);
  });

  test("costSummary dashes an unpriced total and names an agent's own figure", () => {
    expect(
      costSummary({ source: "estimate", unpricedModels: ["mystery-a"] }, "X")
    ).toEqual({ total: "—", notes: ["unpriced: mystery-a"] });
    expect(costSummary({ source: "agent", totalUsd: 0.5 }, "OpenCode")).toEqual(
      { total: "~$0.50", notes: ["reported by OpenCode"] }
    );
  });
});
