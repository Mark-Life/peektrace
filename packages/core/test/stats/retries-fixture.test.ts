/** One synthetic Claude transcript, end to end: parse, extract, fold into the
 * cause and fail tables, and check what reaches the JSON. */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { parseClaudeSession } from "../../src/services/sessions/parse";
import { defaultOptions } from "../../src/services/stats/accumulate";
import { makeFailTable } from "../../src/services/stats/detectors/failures";
import { makeCauseTable } from "../../src/services/stats/detectors/retries";
import { extractFacts } from "../../src/services/stats/extract";
import { makeMarkerCollector } from "../../src/services/stats/markers";
import { makeRedactor } from "../../src/services/stats/redact";
import { isCallFact } from "../../src/services/stats/schema";
import { contextOf } from "./facts";
import { refOf } from "./fixture-session";

const SECRET = "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234";
const PATH = new URL(
  "../fixtures/sessions-claude/retries.jsonl",
  import.meta.url
).pathname;

const factsOf = (enabled = true) =>
  extractFacts({
    ref: refOf("retries"),
    parsed: parseClaudeSession({
      text: readFileSync(PATH, "utf8"),
      path: PATH,
      sessionId: "retries",
    }),
    project: "proj",
    redact: makeRedactor({ enabled }),
  });

const tablesOf = (enabled = true) => {
  const redact = makeRedactor({ enabled });
  const opts = { ...defaultOptions(redact) };
  const causes = makeCauseTable(opts);
  const fails = makeFailTable(opts);
  for (const fact of factsOf(enabled)) {
    causes.add(fact);
    fails.add(fact);
  }
  const ctx = { ...contextOf(), redact };
  return { causes: causes.finish(ctx).rows, fails: fails.finish(ctx) };
};

describe("the retries fixture", () => {
  const facts = factsOf();
  const calls = facts.filter(isCallFact);

  test("every call carries its request id and context", () => {
    expect(calls).toHaveLength(22);
    expect(calls[0]?.req).toBe("req_0");
    expect(calls[0]?.ctxTok).toBe(21_000);
    // The sidechain call's turn is not registered in a main transcript.
    expect(calls[4]?.side).toBe(true);
    expect(calls[4]?.ctxTok).toBeNull();
    expect(facts[0]?.row === "session" && facts[0].cwd).toBe("/work/proj");
  });

  test("each cause lands on its call, and the grep false positive does not", () => {
    expect(calls.map((c) => c.cause)).toEqual([
      "zsh-glob-flag",
      null,
      "zsh-glob-path",
      null,
      "zsh-glob-flag",
      null,
      null,
      "pkill-self-kill",
      null,
      "sleep-blocked",
      null,
      "command-not-found",
      null,
      "read-before-write",
      null,
      null,
      "structured-output-schema",
      null,
      "output-too-large",
      null,
      "zsh-glob-flag",
      null,
    ]);
    expect(calls[0]?.err).toBe(false);
    expect(calls[0]?.failed).toBe(true);
  });

  test("pairs: siblings and a sidechain never pair; the rest do", () => {
    const { causes } = tablesOf();
    const pairs = Object.fromEntries(causes.map((r) => [r.cause, r.pairs]));
    expect(pairs).toEqual({
      "zsh-glob-flag": 2,
      "zsh-glob-path": 0,
      "pkill-self-kill": 1,
      "sleep-blocked": 1,
      "command-not-found": 1,
      "read-before-write": 1,
      "structured-output-schema": 1,
      "output-too-large": 1,
    });
    const glob = causes.find((r) => r.cause === "zsh-glob-flag");
    expect(glob?.hits).toBe(3);
    expect(glob?.flagged).toBe(2);
    expect(glob?.hitsBy).toEqual({ main: 2, nested: 1 });
    expect(glob?.diffs).toEqual([
      { from: "--include=*.env", to: "--include='*.env'", pairs: 1 },
      { from: "--include=*.ts", to: "--include='*.ts'", pairs: 1 },
    ]);
    expect(glob?.retryCtxTokens).toBe(22_000 + 10_000);
    expect(causes[0]?.cause).toBe("zsh-glob-flag");
  });

  test("a call a cause claims is charged to the cause table, never the fail table", () => {
    const { fails } = tablesOf();
    expect(fails.map((r) => r.key)).toEqual([]);
  });

  test("no secret from a failing command reaches the rows", () => {
    const json = JSON.stringify(tablesOf());
    expect(json).not.toContain(SECRET);
    expect(JSON.stringify(facts)).not.toContain(SECRET);
  });

  test("an unredacted pass finds the same causes", () => {
    const raw = tablesOf(false).causes.map((r) => [r.key, r.hits, r.pairs]);
    const red = tablesOf().causes.map((r) => [r.key, r.hits, r.pairs]);
    expect(raw).toEqual(red);
  });

  test("markers: a cause on the failed call, a retry mark on the retry", () => {
    const collector = makeMarkerCollector({
      redact: makeRedactor({ enabled: true }),
      sid: "retries",
    });
    for (const fact of facts) {
      collector.add(fact);
    }
    const marks = collector.finish();
    const at = (seq: number) =>
      marks.filter((m) => m.seq === seq).map((m) => m.detector);
    expect(at(0)).toEqual(["zsh-glob-flag"]);
    expect(at(1)).toEqual(["retry"]);
    expect(marks.find((m) => m.seq === 1)?.label).toBe(
      "retry of the Bash call 10s earlier: --include=*.ts → --include='*.ts'"
    );
    expect(at(19)).toEqual(["retry"]);
  });
});
