import { describe, expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";
import type { FinishContext } from "../../src/services/stats/accumulate";
import { defaultOptions } from "../../src/services/stats/accumulate";
import {
  benignFilter,
  isPiped,
  makeFailTable,
  scanOutput,
} from "../../src/services/stats/detectors/failures";
import {
  DEFAULT_ROW_LIMIT,
  DEFAULT_SAMPLE_CAP,
} from "../../src/services/stats/rank";
import { makeRedactor } from "../../src/services/stats/redact";
import type { BashFact, CorpusSummary } from "../../src/services/stats/schema";

const redact = makeRedactor({ enabled: true });

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

const contextOf = (caveats: string[]): FinishContext => ({
  corpus: EMPTY_CORPUS,
  window: { days: 60, fromIso: "", toIso: "" },
  redact,
  limit: DEFAULT_ROW_LIMIT,
  sampleCap: DEFAULT_SAMPLE_CAP,
  caveat: (note) => {
    caveats.push(note);
  },
});

const bashFact = (over: Partial<BashFact>): BashFact => ({
  row: "bash",
  sid: "s1",
  agent: "claude",
  project: "proj",
  kind: "main",
  parent: null,
  run: null,
  seq: 0,
  toolUseId: null,
  tool: "Bash",
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
  cmd: "true",
  bin: "true",
  family: "true",
  job: "other",
  present: ["other"],
  wait: null,
  declaredDelaySec: 0,
  piped: false,
  bg: false,
  capped: false,
  ...over,
});

const shell = (text: string, isError = false) =>
  scanOutput({ tool: "Bash", text, isError });

describe("scanOutput", () => {
  test("a passing bun test run is not a failure", () => {
    expect(shell(" 24 pass\n 0 fail\n").reason).toBeNull();
    expect(shell(" 24 pass\n 0 fail\n").failed).toBe(false);
  });

  test("a non-zero failing count is a failure", () => {
    expect(shell(" 21 pass\n 3 fail\n").reason).toBe("test");
    expect(shell("Tests  2 failed | 8 passed").reason).toBe("test");
    expect(shell("FAIL  src/thing.test.ts > case").reason).toBe("test");
  });

  test("a tsc diagnostic fires, a bare word does not", () => {
    expect(shell("src/x.ts(12,3): error TS2339: nope").reason).toBe("tsc");
    expect(shell("error TS: something went wrong").reason).toBeNull();
  });

  test("a lint diagnostic needs a file:line:col or a summary", () => {
    expect(shell("src/x.ts:3:1 lint/style/useConst").reason).toBe("lint");
    expect(shell("Found 4 errors.").reason).toBe("lint");
    expect(shell("the workflow writes to lint/ and exits").reason).toBeNull();
  });

  test("a python traceback and a node stack frame fire", () => {
    expect(shell("Traceback (most recent call last):").reason).toBe(
      "traceback"
    );
    expect(shell("boom\n    at run (/a/b.ts:12:3)").reason).toBe("node");
  });

  test("the scanner only reads shell output", () => {
    const read = scanOutput({
      tool: "Read",
      text: "Traceback (most recent call last):",
      isError: false,
    });
    expect(read.reason).toBeNull();
    expect(read.failed).toBe(false);
  });

  test("the agent's own flag always fails the row", () => {
    expect(shell("all good", true).failed).toBe(true);
  });
});

describe("isPiped", () => {
  test("a pipe into a filter discards the exit code", () => {
    expect(isPiped("bun run typecheck | tail -30")).toBe(true);
    expect(isPiped("bun run typecheck 2>&1 | head")).toBe(true);
    expect(isPiped("bun run typecheck")).toBe(false);
  });
});

const filterOf = (cmd: string, output: string) =>
  benignFilter({ cmd, output, detector: null });

describe("benignFilter", () => {
  test("zsh no-match is the answer, not a failure", () => {
    expect(
      filterOf(
        "grep -rn x packages/**/__fixtures__",
        "Exit code 1\nzsh: no matches found: packages/**/__fixtures__"
      )
    ).toBe("glob-no-match");
  });

  test("an existence check reporting absence is the answer", () => {
    expect(
      filterOf("brew install x; which 7z", "Exit code 1\n7z not found")
    ).toBe("absence-probe");
  });

  test("a chain that printed its work before a failing tail", () => {
    const output = `Exit code 1\n${"listing output line\n".repeat(30)}`;
    expect(filterOf("ls -la; git status; false", output)).toBe("chained-tail");
  });

  test("a chain whose tail printed a real error is kept", () => {
    const output = `Exit code 1\n${"x".repeat(240)}\nTraceback (most recent call last):`;
    expect(filterOf("ls -la; python3 run.py", output)).toBeNull();
  });

  test("a denied call never ran", () => {
    expect(
      filterOf(
        "rm -rf /",
        "The user doesn't want to proceed with this tool use. The tool use was rejected."
      )
    ).toBe("user-rejected");
  });

  test("a detector's row is never suppressed", () => {
    const output = `Exit code 1\n${"printed work\n".repeat(30)}`;
    expect(filterOf("ls -la; echo ===; git status; false", output)).toBeNull();
    expect(
      benignFilter({ cmd: "ls -la; false", output, detector: "some-detector" })
    ).toBeNull();
  });

  test("a silent failure has no exit header, so it is never suppressed", () => {
    const output = `${"printed work\n".repeat(30)}`;
    expect(filterOf("bun run typecheck | tail -5; true", output)).toBeNull();
  });
});

describe("fail table", () => {
  const rowsFor = (facts: readonly BashFact[]) => {
    const table = makeFailTable(defaultOptions(redact));
    for (const f of facts) {
      table.add(f);
    }
    const caveats: string[] = [];
    return { rows: table.finish(contextOf(caveats)), caveats };
  };

  test("the zsh separator is a row of its own, with the fix attached", () => {
    const { rows } = rowsFor([
      bashFact({
        seq: 0,
        err: true,
        failed: true,
        cmd: "ls; echo ===; git status",
      }),
      bashFact({
        seq: 1,
        sid: "s2",
        err: true,
        failed: true,
        cmd: "echo === done",
      }),
    ]);
    expect(rows).toHaveLength(1);
    expect(rows[0]?.key).toBe("detect:zsh-equals-abort");
    expect(rows[0]?.detector).toBe("zsh-equals-abort");
    expect(rows[0]?.rows).toBe(2);
    expect(rows[0]?.sessions).toBe(2);
    expect(rows[0]?.note).toContain('echo "==="');
  });

  test("a silent failure is its own row and names the pipe", () => {
    const { rows } = rowsFor([
      bashFact({
        seq: 0,
        failed: true,
        failReason: "tsc",
        piped: true,
        cmd: "bun run typecheck | tail -30",
      }),
      bashFact({
        seq: 1,
        sid: "s2",
        failed: true,
        failReason: "tsc",
        piped: true,
        cmd: "bun run typecheck | head",
      }),
    ]);
    expect(rows[0]?.key).toBe("scan:tsc");
    expect(rows[0]?.label).toBe("error TS####");
    expect(rows[0]?.where).toBe("Bash exit 0");
    expect(rows[0]?.flagged).toBe(0);
    expect(rows[0]?.detector).toBe("piped-exit-code");
    expect(rows[0]?.note).toBe(
      "2 of 2 pipe into head/tail/grep, so the exit code is the pipe's"
    );
  });

  test("a suppressed row leaves the table and is counted in a caveat", () => {
    const { rows, caveats } = rowsFor([
      bashFact({ seq: 0, err: true, failed: true, benign: "glob-no-match" }),
      bashFact({ seq: 1, err: true, failed: true, cmd: "bun test" }),
    ]);
    expect(rows).toHaveLength(1);
    expect(rows[0]?.key).toBe("flag:Bash");
    expect(rows[0]?.rows).toBe(1);
    expect(caveats.some((c) => c.includes("1 of 2"))).toBe(true);
  });

  test("rows rank by sessions spanned, then rows", () => {
    const wide = Array.from({ length: 3 }, (_, i) =>
      bashFact({
        seq: i,
        sid: `wide${i}`,
        failed: true,
        failReason: "lint",
        cmd: "bun x ultracite check",
      })
    );
    const deep = Array.from({ length: 5 }, (_, i) =>
      bashFact({
        seq: i,
        sid: "deep",
        failed: true,
        failReason: "tsc",
        cmd: "bun run typecheck",
      })
    );
    const { rows } = rowsFor([...deep, ...wide]);
    expect(rows.map((r) => r.key)).toEqual(["scan:lint", "scan:tsc"]);
  });

  test("drill returns the calls behind one key, chronologically", () => {
    const table = makeFailTable({
      ...defaultOptions(redact),
      drill: { table: "fail", key: "scan:tsc" },
    });
    table.add(
      bashFact({
        seq: 1,
        ts: "2026-08-02T00:00:00.000Z",
        failed: true,
        failReason: "tsc",
        cmd: "b",
      })
    );
    table.add(
      bashFact({
        seq: 0,
        ts: "2026-08-01T00:00:00.000Z",
        failed: true,
        failReason: "tsc",
        cmd: "a",
      })
    );
    table.add(bashFact({ seq: 2, failed: true, failReason: "lint" }));
    const hits = table.drill(contextOf([]));
    expect(hits.map((h) => h.line)).toEqual(["a", "b"]);
  });
});

/**
 * The labelled oracle: 161 hand-labelled errored Bash rows, sampled every 6th by
 * (t, sid, cmd), kept outside the repo because it holds real commands from 25
 * private projects. `PEEKTRACE_BENIGN_SAMPLE` overrides the path.
 */
const ORACLE =
  process.env.PEEKTRACE_BENIGN_SAMPLE ??
  "/Users/andrey-m/Code/agent-analysis/peektrace-stats/benign-sample.jsonl";

interface LabelledRow {
  readonly cmd: string;
  readonly err_text: string;
  readonly label: "BENIGN" | "REAL" | "UNDECIDED";
  readonly ruleTag: string;
}

const ZSH_SEPARATOR = /(^|[\s;&|])echo\s+=/;

describe.skipIf(!existsSync(ORACLE))(
  "benign filters against the labelled sample",
  () => {
    const sample = readFileSync(ORACLE, "utf8")
      .trim()
      .split("\n")
      .map((l) => JSON.parse(l) as LabelledRow);
    const scored = sample.map((row) => ({
      row,
      id: benignFilter({
        cmd: row.cmd,
        output: row.err_text,
        detector: null,
      }),
    }));
    const suppressed = scored.filter((s) => s.id !== null);
    const count = (
      rows: readonly { readonly row: LabelledRow }[],
      label: LabelledRow["label"]
    ) => rows.filter((s) => s.row.label === label).length;

    test("the sample is the one the rates were measured on", () => {
      expect(sample).toHaveLength(161);
      expect(sample.filter((r) => r.label === "BENIGN")).toHaveLength(59);
      expect(sample.filter((r) => r.label === "REAL")).toHaveLength(100);
    });

    test("the shipped set is 78% precise at 64% recall", () => {
      const benign = count(suppressed, "BENIGN");
      const real = count(suppressed, "REAL");
      expect(suppressed).toHaveLength(50);
      expect(benign).toBe(38);
      expect(real).toBe(11);
      expect(Math.round((benign / (benign + real)) * 100)).toBe(78);
      expect(Math.round((benign / 59) * 100)).toBe(64);
    });

    test("each filter holds the precision it was scored at", () => {
      const per = (id: string) => {
        const hits = suppressed.filter((s) => s.id === id);
        const benign = count(hits, "BENIGN");
        const real = count(hits, "REAL");
        return { rows: hits.length, benign, real };
      };
      expect(per("glob-no-match")).toEqual({ rows: 9, benign: 9, real: 0 });
      expect(per("user-rejected")).toEqual({ rows: 3, benign: 3, real: 0 });
      expect(per("absence-probe")).toEqual({ rows: 10, benign: 9, real: 1 });
      expect(per("chained-tail")).toEqual({ rows: 28, benign: 17, real: 10 });
    });

    test("every sampled zsh-separator row is a real failure, and none is suppressed", () => {
      const detected = sample.filter((r) => ZSH_SEPARATOR.test(r.cmd));
      expect(detected).toHaveLength(30);
      expect(detected.every((r) => r.label === "REAL")).toBe(true);
      expect(suppressed.filter((s) => ZSH_SEPARATOR.test(s.row.cmd))).toEqual(
        []
      );
    });
  }
);
