/** Error fingerprinting: one vocabulary, two entry points, six rules.
 *
 * Each rule below has a pair of inputs that land in one cluster with it and in
 * two without it — or the reverse, where a rule keeps apart two failures the
 * shipped mask merged. The numbers quoted are the ones a full corpus pass
 * produced; the fixture reproduces the shape, not the scale.
 */
import { describe, expect, test } from "bun:test";
import {
  type AccumulatorOptions,
  defaultOptions,
  type FinishContext,
  makeCorpusTotals,
} from "../../src/services/stats/accumulate";
import {
  clusterLabel,
  errorSignature,
  makeClusterTable,
  normalizeCommand,
} from "../../src/services/stats/detectors/clustering";
import { clusterKeyOf } from "../../src/services/stats/normalize";
import { makeRedactor } from "../../src/services/stats/redact";
import type { StatsFact, ToolFact } from "../../src/services/stats/schema";

const redact = makeRedactor({ enabled: true });

const sigOf = (tool: string, text: string) => errorSignature({ tool, text });

const keyOf = (tool: string, text: string) =>
  clusterKeyOf(errorSignature({ tool, text }) ?? "");

/** One errored tool call. Only the columns the cluster table reads. */
const factOf = (args: {
  readonly seq: number;
  readonly sid: string;
  readonly text: string;
  readonly tool: string;
}): ToolFact => ({
  row: "tool",
  sid: args.sid,
  agent: "claude",
  project: "peektrace",
  kind: "main",
  parent: null,
  run: null,
  seq: args.seq,
  toolUseId: null,
  tool: args.tool,
  ts: `2026-08-12T10:00:${String(args.seq).padStart(2, "0")}.000Z`,
  durSec: 1,
  err: true,
  failed: true,
  failReason: null,
  detector: null,
  benign: null,
  sig: errorSignature({ tool: args.tool, text: args.text }),
  arg: null,
  outChars: args.text.length,
  side: false,
});

const contextOf = (
  facts: readonly StatsFact[],
  opts: AccumulatorOptions
): FinishContext => {
  const totals = makeCorpusTotals();
  for (const fact of facts) {
    totals.add(fact);
  }
  return {
    corpus: totals.finish(),
    window: { days: 60, fromIso: "2026-06-13", toIso: "2026-08-12" },
    redact: opts.redact,
    limit: opts.limit,
    sampleCap: opts.sampleCap,
    caveat: () => undefined,
  };
};

/** Run the cluster table over `[tool, text, sid]` triples. */
const cluster = (
  rows: readonly (readonly [string, string, string])[],
  override: Partial<AccumulatorOptions> = {}
) => {
  const opts = { ...defaultOptions(redact), ...override };
  const facts = rows.map(([tool, text, sid], seq) =>
    factOf({ tool, text, sid, seq })
  );
  const table = makeClusterTable(opts);
  for (const fact of facts) {
    table.add(fact);
  }
  const ctx = contextOf(facts, opts);
  return { rows: table.finish(ctx), drill: () => table.drill(ctx) };
};

const SCHEMA_ONE =
  "Output does not match required schema: root: must have required property 'verdict'";
const SCHEMA_FOUR =
  "Output does not match required schema: root: must have required property 'angle', root: must have required property 'findings', root: must have required property 'method', root: must have required property 'nulls'";
const SCHEMA_MIXED =
  "Output does not match required schema: root: must have required property 'refuted', root: must NOT have additional properties";
const SCHEMA_PATH =
  "Output does not match required schema: /verdicts: must be array";

describe("fix 1: a shell error is keyed on its tail, exit code unmasked", () => {
  const traceback = (last: string) =>
    `Exit code 1\nrunning the probe\nTraceback (most recent call last):\n  File "/tmp/p.py", line 8, in <module>\n${last}`;

  test("two exception classes under one Traceback banner stay apart", () => {
    const a = keyOf("Bash", traceback("ValueError: EXA_API_KEY not found"));
    const b = keyOf("Bash", traceback("TypeError: not callable"));
    expect(a).not.toBe(b);
    expect(a).toBe("Exit code 1 ValueError: EXA_API_KEY not found");
  });

  test("the same exception under different banners is one cluster", () => {
    expect(keyOf("Bash", traceback("KeyError: 'gong'"))).toBe(
      keyOf("Bash", "Exit code 1\nquerying\nKeyError: 'calls'")
    );
  });

  test("the exit code survives, so two causes cannot share a row", () => {
    expect(keyOf("Bash", "Exit code 1\nno such thing")).not.toBe(
      keyOf("Bash", "Exit code 143\nno such thing")
    );
    expect(keyOf("Bash", "Exit code 143\nCommand timed out after 2m 0s")).toBe(
      "Exit code 143 Command timed out after <dur>"
    );
  });
});

describe("fix 2: a repeated clause collapses before the text is clipped", () => {
  test("one missing field and four land in one cluster", () => {
    const table = cluster([
      ["StructuredOutput", SCHEMA_ONE, "s1"],
      ["StructuredOutput", SCHEMA_FOUR, "s2"],
      ["StructuredOutput", SCHEMA_MIXED, "s3"],
      ["StructuredOutput", SCHEMA_PATH, "s4"],
    ]);
    expect(table.rows).toHaveLength(1);
    expect(table.rows[0]?.rows).toBe(4);
    expect(table.rows[0]?.sessions).toBe(4);
    expect(table.rows[0]?.key).toBe("Output does not match required schema:");
  });

  test("the collapse happens before the 200-char clip", () => {
    expect(sigOf("StructuredOutput", SCHEMA_FOUR)).toBe(
      "Output does not match required schema: root: must have required property 'angle'"
    );
  });
});

describe("fix 3: a tool_use_error is cut at its first sentence", () => {
  const replace = (body: string) =>
    `<tool_use_error>String to replace not found in file.\nString: ${body}</tool_use_error>`;

  test("two different payloads are one defect", () => {
    expect(keyOf("Edit", replace("  --dry-run   Price the request."))).toBe(
      keyOf("Edit", replace("export const x = 1"))
    );
    expect(keyOf("Edit", replace("anything"))).toBe(
      "String to replace not found in file."
    );
  });

  test("the advice trailing a blocked call is dropped", () => {
    const blocked = (n: number, advice: string) =>
      `<tool_use_error>Blocked: sleep ${n} followed by: echo waited. ${advice}</tool_use_error>`;
    expect(keyOf("Bash", blocked(90, "Use Monitor with an until-loop."))).toBe(
      keyOf("Bash", blocked(45, "Use run_in_background instead."))
    );
    expect(keyOf("Bash", blocked(90, "anything"))).toBe(
      "Blocked: sleep <n> followed by: echo waited."
    );
  });
});

describe("fix 4: durations, hosts, leading binaries and plurals are masked", () => {
  test("two timeout lengths are one timeout", () => {
    expect(keyOf("Bash", "Exit code 143\nCommand timed out after 2m 0s")).toBe(
      keyOf("Bash", "Exit code 143\nCommand timed out after 10m 0s")
    );
  });

  test("two hosts behind one resolver failure are one row", () => {
    expect(keyOf("WebFetch", "getaddrinfo ENOTFOUND search.marcia.dev")).toBe(
      "getaddrinfo ENOTFOUND <host>"
    );
    expect(keyOf("WebFetch", "getaddrinfo ENOTFOUND search.marcia.dev")).toBe(
      keyOf("WebFetch", "getaddrinfo ENOTFOUND search.marcia.cc")
    );
  });

  test("a version banner is not read as a host", () => {
    expect(
      keyOf("Bash", "Exit code 1\nBun v1.3.14 (macOS arm64)")
    ).not.toContain("<host>");
  });

  test("ls and cat reporting one missing path are one row", () => {
    expect(
      keyOf(
        "Bash",
        "Exit code 1\nls: node_modules/x: No such file or directory"
      )
    ).toBe(
      keyOf(
        "Bash",
        "Exit code 1\ncat: node_modules/y: No such file or directory"
      )
    );
    expect(
      keyOf(
        "Bash",
        "Exit code 1\nls: node_modules/x: No such file or directory"
      )
    ).toBe("Exit code 1 <bin>: <path>: No such file or directory");
  });

  test("one issue and three issues are one diagnostic", () => {
    expect(keyOf("Bash", "Exit code 1\nFound 1 issue")).toBe(
      keyOf("Bash", "Exit code 1\nFound 3 issues")
    );
  });
});

describe("fix 5: the fact keeps the name the cluster key masks", () => {
  test("the signature carries the missing property", () => {
    expect(sigOf("StructuredOutput", SCHEMA_ONE)).toContain("'verdict'");
  });

  test("the row prints the names it grouped over", () => {
    const table = cluster([
      ["StructuredOutput", SCHEMA_ONE, "s1"],
      ["StructuredOutput", SCHEMA_ONE, "s2"],
      ["StructuredOutput", SCHEMA_FOUR, "s3"],
    ]);
    expect(table.rows[0]?.label).toBe(
      "Output does not match required schema — verdict, angle"
    );
    expect(table.rows[0]?.samples[0]?.line).toContain(
      "must have required property '"
    );
  });

  test("a missing module name survives into the signature", () => {
    expect(sigOf("Bash", "Exit code 1\nerror: Cannot find module 'zod'")).toBe(
      "Exit code 1 error: Cannot find module 'zod'"
    );
    expect(keyOf("Bash", "Exit code 1\nerror: Cannot find module 'zod'")).toBe(
      "Exit code 1 error: Cannot find module <str>"
    );
  });
});

describe("fix 6: clusters rank by rows, then by sessions spanned", () => {
  test("a bigger cluster in fewer sessions still ranks first", () => {
    const many = Array.from(
      { length: 5 },
      (_, i) => ["Bash", "Exit code 1\nboom", `one-session-${i % 2}`] as const
    );
    const spread = Array.from(
      { length: 4 },
      (_, i) => ["Bash", "Exit code 1\nsplat", `spread-${i}`] as const
    );
    const table = cluster([...many, ...spread]);
    expect(table.rows.map((r) => [r.rows, r.sessions])).toEqual([
      [5, 2],
      [4, 4],
    ]);
  });

  test("equal rows break on sessions spanned", () => {
    const table = cluster([
      ["Bash", "Exit code 1\nboom", "a"],
      ["Bash", "Exit code 1\nboom", "a"],
      ["Bash", "Exit code 1\nsplat", "b"],
      ["Bash", "Exit code 1\nsplat", "c"],
    ]);
    expect(table.rows[0]?.key).toBe("Exit code 1 splat");
  });
});

describe("the table itself", () => {
  const corpus = [
    ["StructuredOutput", SCHEMA_ONE, "w1"],
    ["StructuredOutput", SCHEMA_FOUR, "w2"],
    ["StructuredOutput", SCHEMA_MIXED, "w3"],
    ["Bash", "Exit code 1\nls: a/b: No such file or directory", "b1"],
    ["Bash", "Exit code 1\ncat: c/d: No such file or directory", "b2"],
    [
      "Write",
      "<tool_use_error>File has not been read yet.</tool_use_error>",
      "b1",
    ],
  ] as const;

  test("the top rows reproduce", () => {
    const table = cluster(corpus);
    expect(table.rows.map((r) => [r.key, r.rows, r.sessions, r.tool])).toEqual([
      ["Output does not match required schema:", 3, 3, "StructuredOutput"],
      ["Exit code 1 <bin>: <path>: No such file or directory", 2, 2, "Bash"],
      ["File has not been read yet.", 1, 1, "Write"],
    ]);
  });

  test("two passes over the same facts print the same rows", () => {
    expect(JSON.stringify(cluster(corpus).rows)).toBe(
      JSON.stringify(cluster(corpus).rows)
    );
  });

  test("a drill returns the calls behind one key, in call order", () => {
    const table = cluster(corpus, {
      drill: {
        table: "cluster",
        key: "Output does not match required schema:",
      },
    });
    const hits = table.drill();
    expect(hits.map((h) => h.sid)).toEqual(["w1", "w2", "w3"]);
    expect(hits[0]?.line).toContain("'verdict'");
  });

  test("no drill target retains nothing", () => {
    expect(cluster(corpus).drill()).toEqual([]);
  });

  test("a label drops the mask noise a key ends on", () => {
    expect(clusterLabel("Output does not match required schema:")).toBe(
      "Output does not match required schema"
    );
  });
});

describe("the command side of the same vocabulary", () => {
  test("flags collapse but the task name does not", () => {
    expect(normalizeCommand("bun run typecheck --filter web").family).toBe(
      "bun run typecheck"
    );
    expect(normalizeCommand("bun run typecheck --filter api").family).toBe(
      "bun run typecheck"
    );
    expect(normalizeCommand("bun test").family).toBe("bun test");
  });

  test("echo names no work, so the family is what ran after it", () => {
    expect(
      normalizeCommand('echo "=== typecheck ==="; bun run typecheck').family
    ).toBe("bun run typecheck");
    expect(normalizeCommand("cd /tmp && echo hi && git status").family).toBe(
      "git status"
    );
  });

  test("every for-loop is one family", () => {
    expect(normalizeCommand("for f in *.ts; do wc -l $f; done").family).toBe(
      "for-loop"
    );
    expect(
      normalizeCommand("for P in $(lsof -ti:3000); do kill $P; done").family
    ).toBe("for-loop");
  });

  test("a heredoc body never names a family", () => {
    expect(
      normalizeCommand("python3 - <<'PY'\nimport os\nprint(os.getcwd())\nPY")
        .bin
    ).toBe("python3");
  });

  test("an unparseable command still yields its first word", () => {
    expect(normalizeCommand("   ").family).toBe("");
    expect(normalizeCommand("weirdbin").family).toBe("weirdbin");
  });
});
