/** The four corpus panels.
 *
 * Each panel is judged on the property that makes it actionable, not on a
 * number. Context concentration must name the head of the distribution, because
 * a capped table hides it. Fan-out must charge a run only for the agents it
 * spawned, and must say which session kind it filtered to. A web repeat must
 * distinguish a re-fetch inside one run, which a workflow author can cache, from
 * the same url reached by two unrelated runs. Cost must never present a guess as
 * a measurement: an unpriced model id is labelled and named, never absorbed.
 */

import { describe, expect, test } from "bun:test";
import type { FinishContext } from "../../src/services/stats/accumulate";
import {
  DEFAULT_PRICING,
  estimateCost,
  makeCorpusPanels,
  priceFor,
} from "../../src/services/stats/detectors/corpus";
import { DEFAULT_ROW_LIMIT } from "../../src/services/stats/rank";
import { makeRedactor } from "../../src/services/stats/redact";
import type {
  CorpusSummary,
  SessionFact,
  StatsFact,
  ToolFact,
} from "../../src/services/stats/schema";

const redact = makeRedactor({ enabled: true });

const opts = { redact, limit: DEFAULT_ROW_LIMIT, sampleCap: 3, drill: null };

const EMPTY_CORPUS: CorpusSummary = {
  transcripts: 0,
  sessions: 0,
  projects: 0,
  toolCalls: 0,
  bashCalls: 0,
  bashSec: 0,
  toolSec: 0,
  humanWaitSec: 0,
  kinds: {},
  agents: {},
};

/** One transcript summary, minted the way `extract.ts` mints it. */
const session = (
  fact: Partial<SessionFact> & { sid: string }
): SessionFact => ({
  row: "session",
  agent: "claude",
  project: "p1",
  kind: "main",
  parent: null,
  run: null,
  startedAt: "2026-08-01T00:00:00.000Z",
  endedAt: "2026-08-01T01:00:00.000Z",
  title: null,
  models: ["claude-opus-5"],
  turns: 1,
  userMessages: 1,
  toolCalls: 0,
  inputTokens: 0,
  outputTokens: 0,
  cacheReadTokens: 0,
  cacheCreationTokens: 0,
  maxContextTokens: 0,
  firstTurnCacheCreation: 0,
  ...fact,
});

/** One tool call, minted the way `extract.ts` mints it. */
const call = (fact: Partial<ToolFact> & { sid: string }): ToolFact => ({
  row: "tool",
  agent: "claude",
  project: "p1",
  kind: "main",
  parent: null,
  run: null,
  seq: 0,
  toolUseId: null,
  tool: "WebFetch",
  ts: "2026-08-01T00:00:00.000Z",
  durSec: 1,
  err: false,
  failed: false,
  failReason: null,
  detector: null,
  benign: null,
  sig: null,
  arg: null,
  outChars: 0,
  side: false,
  ...fact,
});

/** Fold facts through the panels, as the service does. */
const run = (facts: readonly StatsFact[]) => {
  const panels = makeCorpusPanels(opts);
  for (const fact of facts) {
    panels.add(fact);
  }
  const caveats: string[] = [];
  const ctx: FinishContext = {
    corpus: EMPTY_CORPUS,
    window: { days: 60, fromIso: "", toIso: "" },
    redact,
    limit: DEFAULT_ROW_LIMIT,
    sampleCap: 3,
    caveat: (note) => {
      caveats.push(note);
    },
  };
  return { caveats, panels: panels.finish(ctx) };
};

/** The one caveat line holding `needle`. */
const caveatWith = (caveats: readonly string[], needle: string) =>
  caveats.find((c) => c.includes(needle)) ?? "";

describe("context concentration", () => {
  test("ranks sessions by peak context, not by the tokens they read", () => {
    const { panels } = run([
      session({ sid: "a", maxContextTokens: 100, cacheReadTokens: 9000 }),
      session({ sid: "b", maxContextTokens: 900, cacheReadTokens: 10 }),
    ]);
    expect(panels.context.map((r) => r.sid)).toEqual(["b", "a"]);
  });

  test("names the head of the distribution, which a capped table hides", () => {
    // One session in 100 holds 90% of the cache reads: the concentration the
    // panel exists to surface, and the row a top-20 table would still show.
    const facts = Array.from({ length: 100 }, (_, i) =>
      session({
        sid: `s${String(i).padStart(3, "0")}`,
        maxContextTokens: i,
        cacheReadTokens: i === 0 ? 900_000 : 1010,
      })
    );
    const { caveats } = run(facts);
    const note = caveatWith(caveats, "top 1%");
    expect(note).toContain("(1 of 100)");
    expect(note).toContain("90.0%");
  });

  test("carries the session id and project, so a row routes back", () => {
    const { panels } = run([
      session({ sid: "a", project: "proj", maxContextTokens: 5 }),
    ]);
    expect(panels.context[0]?.key).toBe("a");
    expect(panels.context[0]?.project).toBe("proj");
  });

  test("two transcripts sharing an id merge instead of colliding", () => {
    const { panels } = run([
      session({ sid: "a", maxContextTokens: 10, cacheReadTokens: 5 }),
      session({ sid: "a", maxContextTokens: 30, cacheReadTokens: 7 }),
    ]);
    expect(panels.context).toHaveLength(1);
    expect(panels.context[0]?.maxContextTokens).toBe(30);
    expect(panels.context[0]?.cacheReadTokens).toBe(12);
  });
});

describe("fan-out prefix cost", () => {
  const FANOUT: readonly StatsFact[] = [
    session({
      sid: "a1",
      kind: "workflow-agent",
      run: "wf_1",
      firstTurnCacheCreation: 30_000,
      cacheCreationTokens: 40_000,
    }),
    session({
      sid: "a2",
      kind: "workflow-agent",
      run: "wf_1",
      firstTurnCacheCreation: 30_000,
      cacheCreationTokens: 40_000,
    }),
    session({ sid: "m1", kind: "main", firstTurnCacheCreation: 99_000 }),
    call({ sid: "a1", run: "wf_1", tool: "Read", durSec: 2 }),
    call({ sid: "a2", run: "wf_1", tool: "Read", durSec: 3 }),
  ];

  test("sums the prefix every spawned agent re-pays, per run", () => {
    const { panels } = run(FANOUT);
    expect(panels.fanout).toHaveLength(1);
    expect(panels.fanout[0]).toMatchObject({
      key: "wf_1",
      agents: 2,
      prefixTokens: 60_000,
      toolCalls: 2,
      totalSec: 5,
    });
  });

  test("states the kind it filtered to, because most transcripts are agents", () => {
    const { caveats } = run(FANOUT);
    const note = caveatWith(caveats, "Fan-out");
    expect(note).toContain("75.0%");
    expect(note).toContain("2 workflow-agent transcripts across 1 runs");
  });

  test("a main session with a first turn is not a fan-out", () => {
    const { panels } = run([
      session({ sid: "m1", kind: "main", firstTurnCacheCreation: 99_000 }),
    ]);
    expect(panels.fanout).toEqual([]);
  });
});

describe("duplicate web fetches", () => {
  const url = "https://example.com/docs";

  test("a repeat inside one run is marked, so a workflow author can cache it", () => {
    const { panels } = run([
      call({ sid: "a1", run: "wf_1", arg: url, seq: 0 }),
      call({ sid: "a2", run: "wf_1", arg: url, seq: 1 }),
    ]);
    expect(panels.web).toHaveLength(1);
    expect(panels.web[0]).toMatchObject({
      calls: 2,
      sessions: 2,
      sameRun: true,
    });
  });

  test("the same url reached by two runs is a repeat, but not an in-run one", () => {
    const { panels } = run([
      call({ sid: "a1", run: "wf_1", arg: url, seq: 0 }),
      call({ sid: "b1", run: "wf_2", arg: url, seq: 1 }),
    ]);
    expect(panels.web[0]?.sameRun).toBe(false);
  });

  test("a url fetched once is not a row", () => {
    const { panels } = run([call({ sid: "a1", arg: url })]);
    expect(panels.web).toEqual([]);
  });

  test("counts the in-run repeats, not the calls that made them", () => {
    const { caveats } = run([
      call({ sid: "a1", run: "wf_1", arg: url, seq: 0 }),
      call({ sid: "a1", run: "wf_1", arg: url, seq: 1 }),
      call({ sid: "a1", run: "wf_1", arg: url, seq: 2 }),
      call({
        sid: "a1",
        run: "wf_1",
        arg: "https://example.com/other",
        seq: 3,
      }),
    ]);
    expect(caveatWith(caveats, "Web:")).toContain("2 of 4 web calls");
  });

  test("a non-web tool with a path argument is never a web repeat", () => {
    const { panels } = run([
      call({ sid: "a1", tool: "Read", arg: "/tmp/x.ts", seq: 0 }),
      call({ sid: "a1", tool: "Read", arg: "/tmp/x.ts", seq: 1 }),
    ]);
    expect(panels.web).toEqual([]);
  });
});

describe("pricing", () => {
  test("a model id resolves through its family", () => {
    expect(priceFor("claude-opus-4-8").rates).toBe(DEFAULT_PRICING.opus);
    expect(priceFor("claude-haiku-4-5-20251001").priced).toBe(true);
  });

  test("an unknown id is priced at sonnet rates and says so", () => {
    const match = priceFor("<synthetic>");
    expect(match.priced).toBe(false);
    expect(match.rates).toBe(DEFAULT_PRICING.sonnet);
  });

  test("cache reads and cache writes bill apart from input", () => {
    const rates = DEFAULT_PRICING.sonnet ?? priceFor("sonnet").rates;
    const usd = estimateCost(
      {
        inputTokens: 1e6,
        outputTokens: 0,
        cacheReadTokens: 1e6,
        cacheCreationTokens: 1e6,
      },
      rates
    );
    expect(usd).toBeCloseTo(
      rates.input + rates.cacheRead + rates.cacheWrite,
      6
    );
  });
});

describe("cost", () => {
  const COST: readonly StatsFact[] = [
    session({
      sid: "m1",
      kind: "main",
      project: "alpha",
      models: ["claude-opus-5"],
      outputTokens: 1e6,
    }),
    session({
      sid: "a1",
      kind: "workflow-agent",
      project: "beta",
      models: ["claude-haiku-4-5"],
      outputTokens: 1e6,
    }),
  ];

  test("rolls up by kind, by model and by project, each key namespaced", () => {
    const { panels } = run(COST);
    const keys = panels.cost.map((r) => r.key);
    expect(keys).toContain("kind:main");
    expect(keys).toContain("kind:workflow-agent");
    expect(keys).toContain("model:claude-opus-5");
    expect(keys).toContain("project:alpha");
  });

  test("every label marks the dollar figure as an estimate", () => {
    const { panels } = run(COST);
    for (const row of panels.cost) {
      expect(row.label).toContain("est)");
    }
  });

  test("the report echoes the rate table that produced the estimate", () => {
    const { caveats } = run(COST);
    const note = caveatWith(caveats, "ESTIMATE");
    expect(note).toContain("opus 15/75/18.75/1.5");
    expect(note).toContain("sonnet 3/15/3.75/0.3");
  });

  test("an unpriced model is labelled and named, never absorbed silently", () => {
    const { caveats, panels } = run([
      session({ sid: "x", models: ["gpt-5-codex"], outputTokens: 1000 }),
    ]);
    const row = panels.cost.find((r) => r.key === "model:gpt-5-codex");
    expect(row?.label).toContain("(unpriced)");
    expect(caveatWith(caveats, "no rate card")).toContain("gpt-5-codex");
  });

  test("a two-model session splits evenly and still totals its own tokens", () => {
    const { caveats, panels } = run([
      session({
        sid: "x",
        models: ["claude-opus-5", "claude-haiku-4-5"],
        outputTokens: 1000,
      }),
    ]);
    const models = panels.cost.filter((r) => r.key.startsWith("model:"));
    expect(models.map((r) => r.outputTokens)).toEqual([500, 500]);
    expect(caveatWith(caveats, "splits each session")).toContain(
      "1 of 1 sessions"
    );
  });
});

describe("determinism", () => {
  test("two passes over the same facts produce identical output", () => {
    const facts: readonly StatsFact[] = [
      session({ sid: "b", maxContextTokens: 10, cacheReadTokens: 10 }),
      session({
        sid: "a",
        kind: "workflow-agent",
        run: "wf_1",
        maxContextTokens: 10,
        cacheReadTokens: 10,
        cacheCreationTokens: 20,
        firstTurnCacheCreation: 5,
      }),
      call({ sid: "a", run: "wf_1", arg: "https://example.com", seq: 0 }),
      call({ sid: "b", arg: "https://example.com", seq: 1 }),
    ];
    const first = run(facts);
    const second = run(facts);
    expect(JSON.stringify(second.panels)).toBe(JSON.stringify(first.panels));
    expect(second.caveats).toEqual(first.caveats);
  });

  test("ties break on the key, so equal metrics still order the same way", () => {
    const { panels } = run([
      session({ sid: "z", maxContextTokens: 1 }),
      session({ sid: "a", maxContextTokens: 1 }),
      session({ sid: "m", maxContextTokens: 1 }),
    ]);
    expect(panels.context.map((r) => r.sid)).toEqual(["a", "m", "z"]);
  });
});
