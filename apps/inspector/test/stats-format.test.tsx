/** The two rules the stats screen owns: how a second reads, and what leads.
 *
 * The findings order is the one piece of judgement in the UI, so it is pinned
 * here rather than left to the JSX: a row whose detector names a one-line fix
 * outranks a row that only carries a measured note, which outranks a time row —
 * and a fail count is never added to a second.
 */
import { expect, test } from "bun:test";
import type {
  CauseRow,
  FailRow,
  StatsReport,
  TimeRow,
} from "@workspace/core/services/stats/schema";
import { DETECTOR_VERSION } from "@workspace/core/services/stats/schema";
import { renderToStaticMarkup } from "react-dom/server";
import { markedSessionHash } from "../src/lib/routes";
import {
  findingsOf,
  hms,
  projectTail,
  promptsOf,
  ruleLine,
  sessionChip,
  suggestedRules,
  timeNote,
} from "../src/lib/stats-format";
import { CauseTable } from "../src/routes/stats/cause-table";
import { FailTable } from "../src/routes/stats/fail-table";
import { Findings } from "../src/routes/stats/findings";
import { ReportHeader } from "../src/routes/stats/report-header";
import { TimeTable } from "../src/routes/stats/time-table";

const failRow = (
  over: Partial<FailRow> & { readonly key: string }
): FailRow => ({
  detector: null,
  flagged: 0,
  label: over.key,
  note: null,
  projects: 1,
  rows: 1,
  samples: [],
  sessions: 1,
  where: "Bash exit 0",
  ...over,
});

const timeRow = (
  over: Partial<TimeRow> & { readonly key: TimeRow["key"] }
) => ({
  bgRuns: 0,
  bgSec: 0,
  cappedRuns: 0,
  cappedSec: 0,
  coPresentRuns: 0,
  label: over.key,
  medianSec: 1,
  p95Sec: 2,
  runs: 10,
  samples: [],
  share: 0.1,
  totalSec: 100,
  ...over,
});

const report = (args: {
  readonly causes?: readonly CauseRow[];
  readonly fails: readonly FailRow[];
  readonly time: readonly TimeRow[];
}): StatsReport => ({
  causes: args.causes ?? [],
  caveats: [],
  clusters: [],
  context: [],
  corpus: {
    agents: {},
    bashCalls: 0,
    bashSec: 0,
    kinds: {},
    projects: 0,
    sessions: 0,
    toolCalls: 0,
    toolSec: 0,
    humanWaitSec: 0,
    transcripts: 0,
  },
  cost: [],
  detectorVersion: DETECTOR_VERSION,
  fails: args.fails,
  fanout: [],
  generatedAt: 0,
  paths: {},
  schemaVersion: "peektrace-stats/v2",
  skipped: [],
  time: args.time,
  waiting: {
    bgDeclaredSec: 0,
    bgRuns: 0,
    bgSec: 0,
    byShape: [],
    calls: 0,
    medianSec: 0,
    p95Sec: 0,
    projects: 0,
    samples: [],
    sessions: 0,
    share: 0,
    totalSec: 0,
  },
  web: [],
  window: {
    days: 30,
    fromIso: "2026-09-21T00:00:00.000Z",
    toIso: "2026-10-21T00:00:00.000Z",
  },
});

test("seconds read as a duration, never as a bare float", () => {
  expect(hms(0.4)).toBe("0.40s");
  expect(hms(42)).toBe("42s");
  expect(hms(187)).toBe("3m07s");
  expect(hms(18_277)).toBe("5h04m");
});

test("a time row says only what its own columns measured", () => {
  expect(timeNote(timeRow({ key: "typecheck" }))).toBeNull();
  expect(
    timeNote(timeRow({ bgRuns: 3, cappedRuns: 2, cappedSec: 60, key: "test" }))
  ).toContain("harness ceiling");
  expect(timeNote(timeRow({ bgRuns: 3, key: "wait/poll" }))).toContain(
    "backgrounded"
  );
});

test("findings lead with the cheapest fix, then each table's own impact", () => {
  const findings = findingsOf(
    report({
      fails: [
        failRow({
          key: "scan:tsc",
          note: "555 of 576 pipe into head/tail",
          sessions: 225,
        }),
        failRow({
          detector: "zsh-equals-abort",
          key: "detect:zsh-equals-abort",
          note: 'quote the separator: echo "==="',
          sessions: 91,
        }),
        failRow({ key: "flag:Bash", sessions: 300 }),
      ],
      time: [timeRow({ cappedRuns: 4, cappedSec: 300, key: "test" })],
    })
  );
  expect(findings.map((f) => f.key)).toEqual([
    "detect:zsh-equals-abort",
    "scan:tsc",
    "test",
  ]);
  // The unnoted flag row outranks both on sessions and is still not a finding:
  // a finding is a row that carries something to do about it.
  expect(findings.some((f) => f.key === "flag:Bash")).toBe(false);
  expect(findings[0]?.impact).toBe("91 sessions");
  expect(findings.at(-1)?.impact).toBe("1m40s");
});

test("projectTail keeps short slugs and cuts long ones at a dash", () => {
  expect(projectTail("peephole")).toBe("peephole");
  expect(
    projectTail(
      "-Users-andrey-Library-Mobile-Documents-iCloud-md-obsidian-Documents-Obsidian-Vault"
    )
  ).toBe("…Obsidian-Vault");
  expect(projectTail("-Users-andrey-Code-personal-peephole")).toBe(
    "…personal-peephole"
  );
  expect(projectTail("-Users-x-personal-thinkrail")).toBe(
    "…personal-thinkrail"
  );
  expect(projectTail("a".repeat(30), 10)).toBe(`…${"a".repeat(10)}`);
});

const PARENT = "3f2a1b4c-9d8e-4f00-8000-000000000000";
const SUBAGENT = "agent-aac481ccea1ab4b5a";

test("a subagent chip names its parent session and says subagent", () => {
  const chip = sessionChip({
    project: "-Users-andrey-Code-jetbrains-thinkrail",
    sid: SUBAGENT,
    parent: PARENT,
  });
  expect(chip).toEqual({
    project: "…thinkrail",
    session: "3f2a1b4c",
    subagent: true,
  });
  expect(sessionChip({ project: "p", sid: PARENT, parent: null })).toEqual({
    project: "p",
    session: "3f2a1b4c",
    subagent: false,
  });
});

test("a subagent link opens the subagent through its parent", () => {
  const params = new URLSearchParams(
    markedSessionHash({
      id: SUBAGENT,
      parent: PARENT,
      mark: "piped-exit-code",
    }).split("?")[1]
  );
  expect(params.get("id")).toBe(SUBAGENT);
  expect(params.get("parent")).toBe(PARENT);
  expect(params.get("mark")).toBe("piped-exit-code");
  expect(params.get("view")).toBe("chat");
  expect(
    markedSessionHash({ id: PARENT, parent: null, mark: "" }).includes(
      "parent="
    )
  ).toBe(false);
});

test("a drill link names the call to scroll to", () => {
  const params = new URLSearchParams(
    markedSessionHash({
      id: PARENT,
      parent: null,
      mark: "retry",
      call: "toolu_42",
    }).split("?")[1]
  );
  expect(params.get("call")).toBe("toolu_42");
  for (const call of [undefined, null]) {
    expect(
      markedSessionHash({ id: PARENT, parent: null, mark: "", call }).includes(
        "call="
      )
    ).toBe(false);
  }
});

const causeRow = (over: Partial<CauseRow>): CauseRow => ({
  key: "cause:zsh-glob-flag",
  cause: "zsh-glob-flag",
  kind: "rule",
  label: "unquoted glob in a flag value",
  fix: "quote every glob passed as a flag value",
  failing: true,
  hits: 1488,
  flagged: 34,
  hitsBy: { main: 125, nested: 1363 },
  pairs: 675,
  pairsBy: { main: 41, nested: 634 },
  retrySessions: 550,
  sessions: 914,
  projects: 4,
  retrySec: 2660,
  secMeasured: 675,
  retryCtxTokens: 32_313_906,
  tokensMeasured: 675,
  per1k: { main: 25.8, nested: 53.1 },
  details: [{ value: "*.ts", hits: 1191 }],
  diffs: [{ from: "--include=*.ts", to: "--include='*.ts'", pairs: 135 }],
  rule: null,
  samples: [],
  pairSamples: [],
  ...over,
});

const FIRING: CauseRow["rule"] = {
  matches: [
    {
      path: "~/.claude/CLAUDE.md",
      line: 100,
      excerpt: "Quote globs",
    },
  ],
  sinceIso: "2026-10-15T19:06:47.272Z",
  hitsSince: { main: 42, nested: 442 },
  callsSince: { main: 1348, nested: 7765 },
  per1kSince: { main: 31.16, nested: 56.92 },
  advice: "setopt nonomatch in ~/.zshenv",
};

/** A report with no row to prompt for: buttons that need one render nothing. */
const NO_PROMPT = () => null;

test("a cause finding leads, ahead of the fail rows", () => {
  const findings = findingsOf(
    report({
      causes: [causeRow({})],
      fails: [failRow({ key: "scan:tsc", note: "pipes", sessions: 225 })],
      time: [],
    })
  );
  expect(findings.map((f) => [f.table, f.key])).toEqual([
    ["cause", "cause:zsh-glob-flag"],
    ["fail", "scan:tsc"],
  ]);
  expect(findings[0]?.impact).toBe(
    "675 retries (main 41 / sub 634) · 44m20s · 32.3M ctx tokens re-read"
  );
});

test("a rule already present says so, with its rate since the edit", () => {
  expect(ruleLine(FIRING ?? (null as never))).toBe(
    "rule in ~/.claude/CLAUDE.md:100 · still firing main 31/1k, sub 57/1k since 2026-10-15"
  );
});

test("suggested rules skip causes a memory file already states", () => {
  const r = report({
    causes: [
      causeRow({}),
      causeRow({
        key: "cause:zsh-equals-abort",
        cause: "zsh-equals-abort",
        rule: FIRING,
        fix: "quote ===",
      }),
      causeRow({
        key: "cause:command-not-found",
        cause: "command-not-found",
        kind: "env",
        fix: "install jq",
      }),
    ],
    fails: [],
    time: [],
  });
  expect(suggestedRules(r)).toEqual([
    "quote every glob passed as a flag value",
  ]);
  const html = renderToStaticMarkup(
    <Findings
      findings={findingsOf(r)}
      onDrill={() => undefined}
      onShowAll={() => undefined}
      prompt={promptsOf(r, null)}
      showAll={false}
      suggested={suggestedRules(r)}
    />
  );
  expect(html).toContain('data-testid="stats-copy-all"');
  expect(html).toContain('data-testid="finding-copy"');
  expect(html).toContain('data-testid="finding-prompt"');
  expect(html).toContain("retries");
});

test("the cause table renders its rows, and the open one shows the rule check", () => {
  const html = renderToStaticMarkup(
    <CauseTable
      onSelect={() => undefined}
      prompt={NO_PROMPT}
      rows={[causeRow({ rule: FIRING })]}
      selectedKey="cause:zsh-glob-flag"
    />
  );
  expect(html).toContain('data-testid="cause-row"');
  expect(html).toContain('data-testid="cause-detail"');
  expect(html).toContain("still firing main 31/1k, sub 57/1k");
  expect(html).toContain("setopt nonomatch");
  // The split and the context re-read are in the detail, whatever the width.
  expect(html).toContain('data-testid="cause-split"');
  expect(html).toContain("hits main 125 / sub 1,363");
  expect(html).toContain("context re-read 32.3M");
  expect(html).toContain("--include=*.ts → --include=&#x27;*.ts&#x27; (135)"); // The split column says it counts hits, and the detail's rate names its
  // window, apart from the rule line's rate since the file changed.
  expect(html).toContain("Hits main / sub");
  expect(html).toContain("per 1k calls in this window main 26 / sub 53");
});

test("a phone reads each cause row as one wrapped summary line", () => {
  const html = renderToStaticMarkup(
    <CauseTable
      onSelect={() => undefined}
      prompt={NO_PROMPT}
      rows={[causeRow({ rule: FIRING })]}
      selectedKey={null}
    />
  );
  expect(html).toContain('data-testid="cause-narrow"');
  expect(html).toContain(
    "1,488 hits · 675 retries in 550 sessions · 44m20s · 32.3M ctx re-read"
  );
});

/** The class list of a finding's impact span, from static markup. */
const IMPACT_CLASS = /<span class="([^"]*)" data-testid="stats-finding-impact"/;

test("a finding's impact wraps instead of running past the card", () => {
  const html = renderToStaticMarkup(
    <Findings
      findings={findingsOf(
        report({ causes: [causeRow({})], fails: [], time: [] })
      )}
      onDrill={() => undefined}
      onShowAll={() => undefined}
      prompt={NO_PROMPT}
      showAll={false}
      suggested={[]}
    />
  );
  const impact = html.match(IMPACT_CLASS);
  expect(impact?.[1]).toContain("min-w-0");
  expect(impact?.[1]).not.toContain("shrink-0");
});

/** The accessible name of each prompt button in static markup. */
const PROMPT_LABEL =
  /aria-label="(Copy an agent prompt to investigate [^"]*)"/g;

test("every open row and the header copy an agent prompt, each named for its row", () => {
  const cause = causeRow({ rule: FIRING });
  const fail = failRow({
    key: "scan:tsc",
    label: "error TS####",
    note: "pipes",
  });
  const time = timeRow({ key: "test", label: "tests" });
  const r = report({ causes: [cause], fails: [fail], time: [time] });
  const prompt = promptsOf(r, null);
  const html = [
    renderToStaticMarkup(
      <ReportHeader
        agent={null}
        days={30}
        onAgent={() => undefined}
        onDays={() => undefined}
        onRescan={() => undefined}
        prompt={prompt({ table: "report" })}
        report={r}
      />
    ),
    renderToStaticMarkup(
      <CauseTable
        onSelect={() => undefined}
        prompt={prompt}
        rows={[cause]}
        selectedKey={cause.key}
      />
    ),
    renderToStaticMarkup(
      <FailTable
        onSelect={() => undefined}
        prompt={prompt}
        rows={[fail]}
        selectedKey={fail.key}
      />
    ),
    renderToStaticMarkup(
      <TimeTable
        onSelect={() => undefined}
        prompt={prompt}
        rows={[time]}
        selectedKey={time.key}
      />
    ),
  ].join("");
  for (const id of [
    "stats-prompt",
    "cause-prompt",
    "fail-prompt",
    "time-prompt",
  ]) {
    expect(html).toContain(`data-testid="${id}"`);
  }
  expect([...html.matchAll(PROMPT_LABEL)].map((m) => m[1])).toEqual([
    "Copy an agent prompt to investigate this report",
    "Copy an agent prompt to investigate unquoted glob in a flag value",
    "Copy an agent prompt to investigate error TS####",
    "Copy an agent prompt to investigate tests time",
  ]);
  // The tooltip names what is copied, not the whole prompt.
  expect(html).not.toContain('title="Goal:');
  expect(html).toContain("Copy prompt");
});

test("a closed row renders no prompt button", () => {
  const html = renderToStaticMarkup(
    <FailTable
      onSelect={() => undefined}
      prompt={() => "Goal: x"}
      rows={[failRow({ key: "scan:tsc" })]}
      selectedKey={null}
    />
  );
  expect(html).not.toContain('data-testid="fail-prompt"');
});
