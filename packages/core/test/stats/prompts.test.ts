/** The investigation prompts every surface copies.
 *
 * Pinned: the goal leads, the data block stays short, each row names the calls
 * behind it by transcript path and tool-use id, the commands are real CLI
 * flags in the report's own window, and a rule that is already in memory but
 * still fires asks why instead of suggesting the same line again.
 */
import { describe, expect, test } from "bun:test";
import type { FindingFormat } from "../../src/services/stats/findings";
import { statsPrompt } from "../../src/services/stats/prompts";
import type {
  CauseRow,
  FailRow,
  RuleCheck,
  SampleRef,
  StatsReport,
  TimeRow,
} from "../../src/services/stats/schema";

const FORMAT: FindingFormat = {
  count: (value) => value.toLocaleString("en-US"),
  hms: (sec) => `${sec}s`,
  pct: (share) => `${share * 100}%`,
  tokens: (value) => `${value / 1000}K`,
};

/** A conciseness bound: the whole-report prompt fits on one screen. */
const REPORT_MAX_LINES = 30;

const SUB = "agent-a8cc4430796a7efce";
const PARENT = "92ea6c7d-b251-4d46-af59-c5772244e477";
const PATH = `/home/me/.claude/projects/-home-me-work/${PARENT}/subagents/${SUB}.jsonl`;

const sample = (over: Partial<SampleRef>): SampleRef => ({
  sid: SUB,
  agent: "claude",
  project: "-home-me-work",
  kind: "subagent",
  parent: PARENT,
  seq: 18,
  toolUseId: "toolu_01",
  ts: "2026-10-15T07:46:55.123Z",
  line: 'grep -rn "x" --include=*.ts .',
  ...over,
});

const FIRING: RuleCheck = {
  matches: [{ path: "~/.claude/CLAUDE.md", line: 101, excerpt: "Quote globs" }],
  sinceIso: "2026-10-01T00:00:00.000Z",
  hitsSince: { main: 3, nested: 40 },
  callsSince: { main: 1000, nested: 2000 },
  per1kSince: { main: 3, nested: 20 },
  advice: "if it keeps firing: setopt nonomatch in ~/.zshenv",
};

const causeRow = (over: Partial<CauseRow>): CauseRow => ({
  key: "cause:zsh-glob-flag",
  cause: "zsh-glob-flag",
  kind: "rule",
  label: "unquoted glob in a flag value",
  fix: "quote the glob",
  failing: true,
  hits: 1492,
  flagged: 35,
  hitsBy: { main: 125, nested: 1367 },
  pairs: 678,
  pairsBy: { main: 41, nested: 637 },
  retrySessions: 552,
  sessions: 913,
  projects: 4,
  retrySec: 2698,
  secMeasured: 678,
  retryCtxTokens: 32_000,
  tokensMeasured: 678,
  per1k: { main: 25, nested: 44 },
  details: [{ value: "*.ts", hits: 1196 }],
  diffs: [{ from: "--include=*.ts", to: "--include='*.ts'", pairs: 145 }],
  rule: null,
  samples: [],
  pairSamples: [],
  ...over,
});

const failRow = (over: Partial<FailRow>): FailRow => ({
  key: "scan:tsc",
  label: "error TS####",
  where: "Bash exit 0",
  rows: 206,
  sessions: 137,
  projects: 2,
  flagged: 0,
  note: "186 of 206 pipe into head/tail/grep",
  detector: "piped-exit-code",
  samples: [],
  ...over,
});

const timeRow = (over: Partial<TimeRow>): TimeRow => ({
  key: "wait/poll",
  label: "waiting",
  runs: 643,
  coPresentRuns: 0,
  totalSec: 13_196,
  share: 0.5,
  medianSec: 4,
  p95Sec: 87,
  cappedRuns: 0,
  cappedSec: 0,
  bgRuns: 0,
  bgSec: 0,
  samples: [],
  ...over,
});

const report = (over: Partial<StatsReport>): StatsReport =>
  ({
    window: {
      days: 30,
      fromIso: "2026-09-21T17:48:33.960Z",
      toIso: "2026-10-21T17:48:33.960Z",
    },
    corpus: {
      transcripts: 1821,
      sessions: 1821,
      projects: 132,
      toolCalls: 38_749,
      bashCalls: 35_625,
      bashSec: 74_825,
      toolSec: 81_062,
      humanWaitSec: 0,
      kinds: { main: 401, subagent: 163, "workflow-agent": 1257 },
      agents: { claude: 1821 },
    },
    causes: [],
    fails: [],
    time: [],
    waiting: {
      calls: 643,
      totalSec: 13_196,
      share: 0.5,
      sessions: 197,
      projects: 35,
      medianSec: 4,
      p95Sec: 87,
      byShape: [
        { key: "loop-poll", calls: 207, totalSec: 8956, share: 0.1 },
        { key: "watcher", calls: 0, totalSec: 0, share: 0 },
      ],
      bgRuns: 307,
      bgSec: 2688,
      bgDeclaredSec: 14_608,
      samples: [],
    },
    paths: { [SUB]: PATH },
    ...over,
  }) as StatsReport;

const prompt = (
  r: StatsReport,
  target: Parameters<typeof statsPrompt>[0]["target"]
) => statsPrompt({ format: FORMAT, report: r, target }) ?? "";

describe("the report prompt", () => {
  const r = report({
    causes: [
      causeRow({ rule: FIRING }),
      causeRow({
        key: "cause:output-too-large",
        cause: "output-too-large",
        failing: false,
      }),
      causeRow({ key: "cause:sleep-blocked", cause: "sleep-blocked", hits: 0 }),
    ],
    fails: [
      failRow({}),
      failRow({ key: "flag:Bash", label: "Bash errors", note: null }),
    ],
    time: [
      timeRow({}),
      timeRow({ key: "git", label: "git", totalSec: 9060, share: 0.12 }),
    ],
  });
  const text = prompt(r, { table: "report" });

  test("leads with the goal and stays on one screen", () => {
    expect(
      text.startsWith("Goal: make my coding agents faster and cheaper")
    ).toBe(true);
    expect(text).toContain("don't apply until I confirm");
    expect(text.split("\n").length).toBeLessThanOrEqual(REPORT_MAX_LINES);
  });

  test("states the window, the corpus and each table's top rows", () => {
    expect(text).toContain("(last 30 days, all agents)");
    expect(text).toContain("Data (2026-09-21 → 2026-10-21):");
    expect(text).toContain(
      "1,821 transcripts (401 main, 1,420 subagent), 132 projects"
    );
    expect(text).toContain(
      "avoidable retries: 1,356 round trips, 5396s, 64K ctx tokens re-read"
    );
    expect(text).toContain(
      "  - zsh-glob-flag: 1,492 hits · 678 retries (main 41 / sub 637) · 2698s · 32K ctx · rule in memory\n"
    );
    expect(text).toContain("  - output-too-large: 1,492 hits · 678 read-backs");
    // A cause that never fired is not a finding.
    expect(text).not.toContain("sleep-blocked");
    expect(text).toContain(
      "  - scan:tsc (error TS####): 206 calls, 137 sessions · 186 of 206"
    );
    expect(text).toContain(
      "  - flag:Bash (Bash errors): 206 calls, 137 sessions\n"
    );
    expect(text).toContain(
      "shell time (--time job): wait/poll 13196s (50%) · git 9060s (12%)"
    );
    expect(text).toContain("waiting: 643 calls, 13196s, 307 backgrounded");
  });

  test("names a rule already in memory that still fires", () => {
    expect(text).toContain(
      "rules in memory but still firing: zsh-glob-flag (rule in ~/.claude/CLAUDE.md:101 · still firing"
    );
    expect(
      prompt(report({ causes: [causeRow({})] }), { table: "report" })
    ).toContain("rules in memory but still firing: none");
  });

  test("ends with the real commands, in the report's window and agent scope", () => {
    expect(text).toContain("- full report: peektrace stats --days 30 --json");
    expect(text).toContain(
      "peektrace stats prompt --cause <id> | --fail <key> | --time <job> --days 30"
    );
    expect(text).toContain(
      "peektrace stats drill --cause cause:<id> | --fail <key> | --time <job> --days 30 --json"
    );
    const scoped = statsPrompt({
      agent: "codex",
      format: FORMAT,
      report: r,
      target: { table: "report" },
    });
    expect(scoped).toContain("(last 30 days, codex)");
    expect(scoped).toContain("peektrace stats --days 30 --agent codex --json");
  });
});

describe("a cause prompt", () => {
  const pair = {
    failed: sample({}),
    retry: sample({
      seq: 19,
      toolUseId: "toolu_02",
      line: "grep -rn \"x\" --include='*.ts' .",
    }),
  };
  const row = causeRow({
    rule: FIRING,
    pairSamples: [pair],
    // The paired call repeats in `samples`; it is named once.
    samples: [
      sample({}),
      sample({
        sid: "lost",
        parent: null,
        seq: 3,
        toolUseId: null,
        ts: null,
        line: "ls *.md",
      }),
    ],
  });
  const r = report({ causes: [row] });

  test("is the goal, then the row's data, the samples and the drill command", () => {
    expect(prompt(r, { table: "cause", key: "zsh-glob-flag" })).toBe(
      [
        "Goal: make my coding agents faster and cheaper: fewer model round trips, less compute and waiting, fewer failed tool calls.",
        "Below is measured data for one pattern from peektrace (last 30 days, all agents). Read the sample transcripts to confirm it and find its root cause. Propose concrete changes to my environment (shell config, installed tools, hooks), AGENTS.md/CLAUDE.md, skills or prompts that prevent it. Rank by impact; show exact edits; don't apply until I confirm.",
        "",
        "Pattern: unquoted glob in a flag value",
        "- cause: zsh-glob-flag · fix lives in: memory file (AGENTS.md/CLAUDE.md)",
        "- hits: 1,492 (1,457 exited 0) in 913 sessions, 4 projects · main 125 / sub 1,367",
        "- wasted: 678 retries (main 41 / sub 637) · 2698s · 32K ctx tokens re-read in 552 sessions",
        "- culprits: *.ts ×1,196",
        "- retries changed: --include=*.ts → --include='*.ts' ×145",
        "- suggested fix: quote the glob",
        "- rule in ~/.claude/CLAUDE.md:101 · still firing main 3/1k, sub 20/1k since 2026-10-01",
        "  The rule is in memory yet still fires: find why it isn't followed and fix it in a way that doesn't depend on the agent remembering (e.g. setopt nonomatch in ~/.zshenv).",
        "Samples (failed → retry):",
        `- 2026-10-15T07:46:55Z · toolu_01 · ${PATH}`,
        '  failed: grep -rn "x" --include=*.ts .',
        "  retry: grep -rn \"x\" --include='*.ts' .",
        "- session lost",
        "  call: ls *.md",
        "All calls: peektrace stats drill --cause cause:zsh-glob-flag --days 30 --json",
      ].join("\n")
    );
  });

  test("the full key finds the same row", () => {
    expect(prompt(r, { table: "cause", key: "cause:zsh-glob-flag" })).toBe(
      prompt(r, { table: "cause", key: "zsh-glob-flag" })
    );
  });

  test("a rule cause no memory file states says so; an env cause does not", () => {
    const none = prompt(report({ causes: [causeRow({})] }), {
      table: "cause",
      key: "zsh-glob-flag",
    });
    expect(none).toContain("- rule: none in AGENTS.md/CLAUDE.md");
    const env = prompt(
      report({
        causes: [
          causeRow({
            kind: "env",
            key: "cause:command-not-found",
            cause: "command-not-found",
          }),
        ],
      }),
      { table: "cause", key: "command-not-found" }
    );
    expect(env).toContain("fix lives in: host environment");
    expect(env).not.toContain("- rule");
  });

  test("a rule too new to measure passes on what to do if it keeps firing", () => {
    const fresh = prompt(
      report({
        causes: [
          causeRow({
            rule: {
              ...FIRING,
              callsSince: { main: 0, nested: 0 },
              hitsSince: { main: 0, nested: 0 },
            },
          }),
        ],
      }),
      { table: "cause", key: "zsh-glob-flag" }
    );
    expect(fresh).toContain(
      "not measured yet (added 2026-10-01)\n  Too new to measure; if it keeps firing: setopt nonomatch in ~/.zshenv."
    );
    expect(fresh).not.toContain("still fires");
  });

  test("an unknown key is no prompt", () => {
    expect(
      statsPrompt({
        format: FORMAT,
        report: r,
        target: { table: "cause", key: "nope" },
      })
    ).toBeNull();
    expect(
      statsPrompt({
        format: FORMAT,
        report: r,
        target: { table: "fail", key: "nope" },
      })
    ).toBeNull();
    expect(
      statsPrompt({
        format: FORMAT,
        report: r,
        target: { table: "time", key: "test" },
      })
    ).toBeNull();
  });
});

describe("fail and time prompts", () => {
  test("a fail row names its key, detector, note and calls", () => {
    const text = prompt(
      report({
        fails: [
          failRow({ samples: [sample({ line: "bun run typecheck | tail" })] }),
        ],
      }),
      {
        table: "fail",
        key: "scan:tsc",
      }
    );
    expect(text).toContain("Pattern: error TS#### (Bash exit 0)");
    expect(text).toContain("- key: scan:tsc · detector: piped-exit-code");
    expect(text).toContain(
      "- calls: 206 (0 set the error flag) in 137 sessions, 2 projects"
    );
    expect(text).toContain("- note: 186 of 206 pipe into head/tail/grep");
    expect(text).toContain(
      `Samples:\n- 2026-10-15T07:46:55Z · toolu_01 · ${PATH}\n  call: bun run typecheck | tail`
    );
    expect(
      text.endsWith(
        "All calls: peektrace stats drill --fail scan:tsc --days 30 --json"
      )
    ).toBe(true);
  });

  test("the waiting row carries the waiting shapes", () => {
    const text = prompt(
      report({ time: [timeRow({ cappedRuns: 8, cappedSec: 2880 })] }),
      {
        table: "time",
        key: "wait/poll",
      }
    );
    expect(text).toContain(
      "- job: wait/poll · 643 runs, 13196s (50% of shell time) · median 4s, p95 87s"
    );
    expect(text).toContain(
      "- shapes: loop-poll 207 calls 8956s; 307 backgrounded, declaring 14608s of sleep"
    );
    expect(text).toContain(
      "- note: 8 of 643 runs returned at a harness ceiling"
    );
    expect(text).toContain(
      "peektrace stats drill --time wait/poll --days 30 --json"
    );
  });
});
