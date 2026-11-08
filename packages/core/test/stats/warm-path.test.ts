/** What a warm pass leans on: binaries lexed once at extract time, and shipped
 * lines redacted once, from facts that may predate the current rules. */
import { describe, expect, test } from "bun:test";
import { redactText } from "../../src/services/sessions/redact";
import {
  type AccumulatorOptions,
  defaultOptions,
  type FinishContext,
} from "../../src/services/stats/accumulate";
import { makeClusterTable } from "../../src/services/stats/detectors/clustering";
import { makeFailTable } from "../../src/services/stats/detectors/failures";
import { makeTimeTable } from "../../src/services/stats/detectors/timing";
import { makeWaitPanel } from "../../src/services/stats/detectors/waiting";
import { extractFacts } from "../../src/services/stats/extract";
import { clusterKeyOf } from "../../src/services/stats/normalize";
import { makeRedactor, type Redactor } from "../../src/services/stats/redact";
import {
  type BashFact,
  type DrillTable,
  isBashFact,
} from "../../src/services/stats/schema";
import { ranBins } from "../../src/services/stats/shell-lex";
import { bashFact, contextOf } from "./facts";
import { parsedOf } from "./fixture-session";

// Built at runtime so no scanner flags the fixture as a real key.
const SECRET = [
  "sk-ant-api03-",
  "AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234",
].join("");

describe("ran bins", () => {
  test("extract stores each binary the stored command runs, once", () => {
    const facts = extractFacts(
      parsedOf([
        {
          name: "Bash",
          input: {
            command: "FOO=1 bun run build && git status | head -5; bun test",
          },
          isError: false,
          output: "ok",
        },
      ])
    );
    const bash = facts.find(isBashFact);
    expect(bash?.ran).toEqual(["bun", "git", "head"]);
    expect(bash?.ran).toEqual(ranBins(bash?.cmd ?? ""));
  });
});

/** Every table that samples fact text, each with a drill on one key. */
const tablesOf = (redact: Redactor, drill: AccumulatorOptions["drill"]) => {
  const opts = { ...defaultOptions(redact), drill };
  return [
    makeTimeTable(opts),
    makeWaitPanel(opts),
    makeFailTable(opts),
    makeClusterTable(opts),
  ];
};

const shipped = (args: {
  readonly ctx: FinishContext;
  readonly drills: readonly { readonly key: string; table: DrillTable }[];
  readonly facts: readonly BashFact[];
}) => {
  const { ctx, drills, facts } = args;
  const out: unknown[] = [];
  for (const drill of [null, ...drills]) {
    for (const table of tablesOf(ctx.redact, drill)) {
      for (const fact of facts) {
        table.add(fact);
      }
      out.push(table.finish(ctx), table.drill(ctx));
    }
  }
  // Row keys are match keys, not shipped text, so they are left out.
  return JSON.stringify(out, (k, v) => (k === "key" ? undefined : v));
};

describe("shipped lines from stale facts", () => {
  test("a secret a shard still holds never ships", () => {
    expect(redactText(SECRET)).not.toContain(SECRET);
    const sig = `request failed with key ${SECRET}`;
    const facts = [0, 1, 2].map((seq) =>
      bashFact({
        seq,
        cmd: `curl -H "x-api-key: ${SECRET}" https://example.test/${seq}`,
        job: "wait/poll",
        err: true,
        failed: true,
        sig,
      })
    );
    const out = shipped({
      ctx: contextOf(),
      facts,
      drills: [
        { table: "time", key: "wait/poll" },
        { table: "wait", key: "all" },
        { table: "fail", key: "flag:Bash" },
        { table: "cluster", key: clusterKeyOf(sig) },
      ],
    });
    expect(out.includes("example.test")).toBe(true);
    expect(out.includes("REDACTED")).toBe(true);
    expect(out.includes(SECRET)).toBe(false);
  });
});

describe("shipped lines from stale facts, secret at the clip", () => {
  test("a secret that straddles the sample clip ships masked, not cut", () => {
    // The key starts before the clip and ends after it.
    const filler = "x ".repeat(70);
    const sig = `${filler}${SECRET}`;
    const facts = [0, 1, 2].map((seq) =>
      bashFact({
        seq,
        cmd: `${filler}${SECRET}`,
        job: "wait/poll",
        err: true,
        failed: true,
        sig,
      })
    );
    const out = shipped({
      ctx: contextOf(),
      facts,
      drills: [
        { table: "time", key: "wait/poll" },
        { table: "wait", key: "all" },
        { table: "fail", key: "flag:Bash" },
        { table: "cluster", key: clusterKeyOf(sig) },
      ],
    });
    expect(out.includes("REDACTED")).toBe(true);
    expect(out.includes(SECRET.slice(0, 16))).toBe(false);
  });
});

/** The real redactor, counting every call into it. */
const countingRedactor = () => {
  const base = makeRedactor({ enabled: true });
  let calls = 0;
  const redact: Redactor = {
    enabled: true,
    text: (s) => {
      calls += 1;
      return base.text(s);
    },
    line: (s, max) => {
      calls += 1;
      return base.line(s, max);
    },
    nullable: (s) => {
      calls += 1;
      return base.nullable(s);
    },
  };
  return { redact, calls: () => calls };
};

describe("redaction cost", () => {
  test("redactor calls follow what ships, not the fact count", () => {
    const FACTS = 5000;
    const { redact, calls } = countingRedactor();
    const facts = Array.from({ length: FACTS }, (_, seq) =>
      bashFact({
        seq,
        cmd: `bun run task-${seq}`,
        job: seq % 2 === 0 ? "wait/poll" : "build",
        err: true,
        failed: true,
        // One cluster: a row's label costs a call, so rows must stay few.
        sig: "task failed",
      })
    );
    shipped({ ctx: { ...contextOf(), redact }, facts, drills: [] });
    expect(calls()).toBeGreaterThan(0);
    expect(calls()).toBeLessThan(FACTS / 25);
  });
});
