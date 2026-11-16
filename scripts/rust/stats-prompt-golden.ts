/** Golden prompts for crates/core/tests/stats_prompts.rs: the prompts.test.ts
 * fixtures rendered with the CLI number formatters. Run:
 * bun scripts/rust/stats-prompt-golden.ts > crates/core/tests/fixtures/stats-prompts.json */
import { FORMAT as CLI_FORMAT } from "../../apps/cli/src/commands/stats-render";
import { statsPrompt } from "../../packages/core/src/services/stats/prompts";
import type {
  CauseRow,
  FailRow,
  RuleCheck,
  SampleRef,
  StatsReport,
  TimeRow,
} from "../../packages/core/src/services/stats/schema";

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
) => statsPrompt({ format: CLI_FORMAT, report: r, target }) ?? "";

const out: Record<string, string | null> = {};
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
out.report = prompt(r, { table: "report" });
out.scoped = statsPrompt({
  agent: "codex",
  format: CLI_FORMAT,
  report: r,
  target: { table: "report" },
});
const quiet = {
  ...FIRING,
  hitsSince: { main: 0, nested: 0 },
  per1kSince: { main: 0, nested: 0 },
};
out.quiet = prompt(report({ causes: [causeRow({ rule: quiet })] }), {
  table: "report",
});
out.norule = prompt(report({ causes: [causeRow({})] }), { table: "report" });
out.unmeasured = prompt(
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
  { table: "report" }
);
out.empty = prompt(
  report({
    causes: [],
    fails: [],
    time: [],
    waiting: { ...r.waiting, calls: 0 },
  }),
  { table: "report" }
);
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
const rc = report({ causes: [row] });
out.cause = prompt(rc, { table: "cause", key: "zsh-glob-flag" });
out.none = prompt(report({ causes: [causeRow({})] }), {
  table: "cause",
  key: "zsh-glob-flag",
});
out.env = prompt(
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
out.fresh = prompt(
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
out.fail = prompt(
  report({
    fails: [
      failRow({ samples: [sample({ line: "bun run typecheck | tail" })] }),
    ],
  }),
  { table: "fail", key: "scan:tsc" }
);
out.time = prompt(
  report({ time: [timeRow({ cappedRuns: 8, cappedSec: 2880 })] }),
  { table: "time", key: "wait/poll" }
);
console.log(JSON.stringify(out, null, 2));
