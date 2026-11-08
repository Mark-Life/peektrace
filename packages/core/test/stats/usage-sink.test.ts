/** The usage sink: day, month and block buckets over synthetic usage rows. */
import { describe, expect, test } from "bun:test";
import { turnCost } from "../../src/services/pricing";
import { makeCorpusTotals } from "../../src/services/stats/accumulate";
import { makeCorpusPanels } from "../../src/services/stats/detectors/corpus";
import {
  blocksOf,
  dayKeyOf,
  dedupKeyOf,
  makeUsageSink,
  type UsageSinkOptions,
} from "../../src/services/stats/detectors/usage";
import { DEFAULT_ROW_LIMIT, round } from "../../src/services/stats/rank";
import type { StatsFact, UsageFact } from "../../src/services/stats/schema";
import { contextOf, redact, sessionFact, toolFact, usageFact } from "./facts";

const ms = (iso: string) => Date.parse(iso);

const NOW = "2026-08-10T12:00:00.000Z";

const run = (
  facts: readonly StatsFact[],
  over: Partial<UsageSinkOptions> = {}
) => {
  const sink = makeUsageSink({
    by: "day",
    days: 30,
    nowMs: ms(NOW),
    tz: "UTC",
    ...over,
  });
  for (const fact of facts) {
    sink.add(fact);
  }
  return sink.finish();
};

/** Distinct Claude request ids, so dedup stays out of the way. */
let n = 0;
const at = (ts: string, over: Partial<UsageFact> = {}) => {
  n += 1;
  return usageFact({ ts, req: `req_${n}`, ...over });
};

const call = (iso: string) => ({
  ms: ms(iso),
  model: "claude-sonnet-4-5",
  tokens: {
    inputTokens: 1,
    outputTokens: 0,
    cacheReadTokens: 0,
    cacheCreationTokens: 0,
    usd: 0,
  },
});

describe("blocks", () => {
  test("a block starts at the UTC hour of its first call", () => {
    const [block] = blocksOf(
      [call("2026-08-01T10:47:00.000Z")],
      ms("2026-08-01T12:00:00.000Z")
    );
    expect(block?.startMs).toBe(ms("2026-08-01T10:00:00.000Z"));
    expect(block?.endMs).toBe(ms("2026-08-01T15:00:00.000Z"));
  });

  test("a call past start + 5h opens the next block", () => {
    const blocks = blocksOf(
      [call("2026-08-01T10:00:00.000Z"), call("2026-08-01T15:01:00.000Z")],
      ms(NOW)
    );
    expect(blocks.map((b) => new Date(b.startMs).toISOString())).toEqual([
      "2026-08-01T10:00:00.000Z",
      "2026-08-01T15:00:00.000Z",
    ]);
  });

  test("a gap over 5h opens the next block; calls sort first", () => {
    const blocks = blocksOf(
      [
        call("2026-08-01T21:30:00.000Z"),
        call("2026-08-01T10:10:00.000Z"),
        call("2026-08-01T11:00:00.000Z"),
      ],
      ms(NOW)
    );
    expect(blocks.map((b) => b.calls.length)).toEqual([2, 1]);
    expect(blocks[1]?.startMs).toBe(ms("2026-08-01T21:00:00.000Z"));
  });

  test("active while open and its last call is under 5h old", () => {
    const calls = [call("2026-08-01T10:47:00.000Z")];
    expect(blocksOf(calls, ms("2026-08-01T14:59:00.000Z"))[0]?.active).toBe(
      true
    );
    expect(blocksOf(calls, ms("2026-08-01T15:00:00.000Z"))[0]?.active).toBe(
      false
    );
  });

  test("block mode counts Claude only and says so", () => {
    const out = run(
      [
        at("2026-08-10T09:30:00.000Z"),
        at("2026-08-10T09:40:00.000Z", { agent: "codex", sid: "c1" }),
      ],
      { by: "block", days: 7 }
    );
    expect(out.rows).toHaveLength(1);
    expect(out.rows[0]?.turns).toBe(1);
    expect(out.rows[0]?.block).toEqual({
      startIso: "2026-08-10T09:00:00.000Z",
      endIso: "2026-08-10T14:00:00.000Z",
      lastIso: "2026-08-10T09:30:00.000Z",
      active: true,
    });
    expect(out.skipped.otherAgents).toBe(1);
    expect(out.caveats.some((c) => c.includes("Claude turns only"))).toBe(true);
  });

  test("a block that ended before the window is dropped whole", () => {
    const out = run(
      [at("2026-08-02T23:00:00.000Z"), at("2026-08-03T13:00:00.000Z")],
      { by: "block", days: 7 }
    );
    expect(out.rows.map((r) => r.key)).toEqual(["2026-08-03T13:00:00.000Z"]);
  });

  test("a block that overlaps the cutoff is kept whole", () => {
    const out = run(
      [at("2026-08-09T10:30:00.000Z"), at("2026-08-09T13:00:00.000Z")],
      { by: "block", days: 1 }
    );
    expect(out.rows.map((r) => r.key)).toEqual(["2026-08-09T10:00:00.000Z"]);
    expect(out.totals.turns).toBe(2);
  });
});

describe("day keys", () => {
  const late = "2026-08-02T03:30:00.000Z";
  const early = "2026-08-02T04:30:00.000Z";

  test("23:30 and 00:30 New York time are two days there, one in UTC", () => {
    const facts = [at(late), at(early)];
    expect(
      run(facts, { tz: "America/New_York" }).rows.map((r) => r.key)
    ).toEqual(["2026-08-01", "2026-08-02"]);
    expect(run(facts, { tz: "UTC" }).rows.map((r) => r.key)).toEqual([
      "2026-08-02",
    ]);
  });

  test("the +5:30 midnight falls on a 15-minute slot edge", () => {
    const key = dayKeyOf("Asia/Kolkata");
    expect(key(ms("2026-08-01T18:29:59.000Z"))).toBe("2026-08-01");
    expect(key(ms("2026-08-01T18:30:00.000Z"))).toBe("2026-08-02");
    expect(key(ms("2026-08-01T18:20:00.000Z"))).toBe("2026-08-01");
  });

  test("an unknown zone throws at build", () => {
    expect(() => dayKeyOf("Mars/Olympus")).toThrow(RangeError);
  });
});

describe("window trim", () => {
  test("day mode keeps whole days back to now - (days - 1)", () => {
    const out = run(
      [at("2026-08-08T23:50:00.000Z"), at("2026-08-09T00:10:00.000Z")],
      { days: 2 }
    );
    expect(out.rows.map((r) => r.key)).toEqual(["2026-08-09"]);
  });

  test("day mode counts calendar days across a DST change", () => {
    const out = run(
      [
        at("2026-03-08T12:00:00.000Z"),
        at("2026-03-08T04:30:00.000Z"),
        at("2026-03-15T03:00:00.000Z"),
      ],
      {
        days: 8,
        nowMs: ms("2026-03-15T04:30:00.000Z"),
        tz: "America/New_York",
      }
    );
    expect(out.rows.map((r) => r.key)).toEqual(["2026-03-08", "2026-03-14"]);
  });

  test("month mode keeps the first month whole", () => {
    const out = run(
      [
        at("2026-06-30T12:00:00.000Z"),
        at("2026-07-01T00:30:00.000Z"),
        at("2026-08-05T00:00:00.000Z"),
      ],
      { by: "month", days: 30 }
    );
    expect(out.rows.map((r) => r.key)).toEqual(["2026-07", "2026-08"]);
    expect(out.totals.turns).toBe(2);
  });
});

describe("dedup", () => {
  const ts = "2026-08-05T10:00:00.000Z";

  test("one Claude request id in two transcripts counts once", () => {
    const out = run([
      usageFact({ ts, sid: "a", req: "req_same" }),
      usageFact({ ts, sid: "b", req: "req_same" }),
    ]);
    expect(out.totals.turns).toBe(1);
    expect(out.skipped.duplicates).toBe(1);
    expect(out.caveats.some((c) => c.includes("count once"))).toBe(true);
    expect(out.caveats.some((c) => c.includes("Claude"))).toBe(false);
  });

  test("line-number ids and other agents' ids stay per transcript", () => {
    const out = run([
      usageFact({ ts, sid: "a", req: "line-3" }),
      usageFact({ ts, sid: "b", req: "line-3" }),
      usageFact({ ts, sid: "p1", req: "msg-1", agent: "pi" }),
      usageFact({ ts, sid: "p2", req: "msg-1", agent: "pi" }),
      usageFact({ ts, sid: "p2", req: "msg-1", agent: "pi" }),
    ]);
    expect(out.totals.turns).toBe(4);
    expect(dedupKeyOf({ agent: "claude", req: "req_x", sid: "a" })).toBe(
      "req_x"
    );
  });
});

describe("cost", () => {
  const ts = "2026-08-05T10:00:00.000Z";

  test("a mixed-model day sums each model at its own rates", () => {
    const opus = at(ts, { model: "claude-opus-4-1", inputTokens: 1_000_000 });
    const sonnet = at(ts, {
      model: "claude-sonnet-4-5",
      inputTokens: 0,
      outputTokens: 1_000_000,
    });
    const out = run([opus, sonnet]);
    const want = (turnCost(opus) ?? 0) + (turnCost(sonnet) ?? 0);
    expect(out.rows[0]?.usd).toBe(round(want));
    expect(out.rows[0]?.models).toEqual([
      "claude-opus-4-1",
      "claude-sonnet-4-5",
    ]);
    expect(out.totals.usd).toBe(round(want));
    expect(out.totals.totalTokens).toBe(2_000_050);
    expect(out.unpricedModels).toEqual([]);
  });

  test("a long prompt bills at its card's long tier", () => {
    const short = at(ts, { model: "claude-haiku-5-5", inputTokens: 99_000 });
    const long = at(ts, { model: "claude-haiku-5-5", inputTokens: 101_000 });
    expect(run([short]).totals.usd).toBe(round(0.0099 + 0.000_025));
    expect(run([long]).totals.usd).toBe(round(0.0505 + 0.000_125));
  });

  test("an unknown model adds no cost and is named", () => {
    const priced = at(ts);
    const out = run([
      priced,
      at(ts, { model: "made-up-model", agent: "codex" }),
    ]);
    expect(out.unpricedModels).toEqual(["made-up-model"]);
    expect(out.totals.usd).toBe(round(turnCost(priced) ?? 0));
    expect(
      out.caveats.some(
        (c) => c.includes("made-up-model") && c.includes("left out of the cost")
      )
    ).toBe(true);
  });
});

describe("skips", () => {
  test("zero-token and untimed calls are left out and counted", () => {
    const out = run([
      at("2026-08-05T10:00:00.000Z", { inputTokens: 0, outputTokens: 0 }),
      usageFact({ ts: null, req: "req_untimed" }),
      at("2026-08-05T10:00:00.000Z"),
      sessionFact({}),
      toolFact({}),
    ]);
    expect(out.totals.turns).toBe(1);
    expect(out.skipped.zeroTokens).toBe(1);
    expect(out.skipped.noTimestamp).toBe(1);
    expect(out.caveats.some((c) => c.includes("no timestamp"))).toBe(true);
  });
});

describe("the report detectors ignore usage rows", () => {
  const opts = { redact, limit: DEFAULT_ROW_LIMIT, sampleCap: 3, drill: null };
  const base: readonly StatsFact[] = [
    sessionFact({ run: "wf_1", kind: "workflow-agent" }),
    toolFact({ run: "wf_1", kind: "workflow-agent" }),
  ];
  const withUsage: readonly StatsFact[] = [
    ...base,
    usageFact({}),
    usageFact({ req: "req_2" }),
  ];

  const fold = (facts: readonly StatsFact[]) => {
    const totals = makeCorpusTotals();
    const panels = makeCorpusPanels(opts);
    for (const fact of facts) {
      totals.add(fact);
      panels.add(fact);
    }
    return { totals: totals.finish(), panels: panels.finish(contextOf()) };
  };

  test("tool counts and run calls are unchanged", () => {
    const got = fold(withUsage);
    expect(got).toEqual(fold(base));
    expect(got.totals.toolCalls).toBe(1);
  });
});
