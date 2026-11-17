/** How the stats screen spells a table cell, and how it renders the text
 * core wrote. Which findings lead and what they say is core's, tested there.
 */
import { expect, test } from "bun:test";

import type {
  CauseRow,
  FailRow,
  Finding,
  RuleCheck,
  TimeRow,
} from "@workspace/view-core/api-types.gen";
import { fmtK } from "@workspace/viz/lib/session-format";
import { renderToStaticMarkup } from "react-dom/server";

import spelling from "../../../tests/fixtures/stats/number-spelling.json";
import { markedSessionHash } from "../src/lib/routes";
import {
  hms,
  pct,
  projectTail,
  promptsOf,
  rateText,
  sessionChip,
} from "../src/lib/stats-format";
import { CauseTable } from "../src/routes/stats/cause-table";
import { FailTable } from "../src/routes/stats/fail-table";
import { Findings } from "../src/routes/stats/findings";
import { ReportHeader } from "../src/routes/stats/report-header";
import { TimeTable } from "../src/routes/stats/time-table";
import { report, statsText } from "./stats-report-fixture";

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

/** Cases `peektrace_js::inspector` spells the same way, for core's text. */
const cases = (rows: readonly (readonly (number | string)[])[]) =>
  rows.map(([input, want]) => [Number(input), String(want)] as const);

test("cells spell numbers as core spells them in its text", () => {
  for (const [sec, want] of cases(spelling.hms)) {
    expect(hms(sec)).toBe(want);
  }

  for (const [share, want] of cases(spelling.pct)) {
    expect(pct(share)).toBe(want);
  }

  for (const [value, want] of cases(spelling.fmtK)) {
    expect(fmtK(value)).toBe(want);
  }
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
    parent: PARENT,
    project: "-Users-andrey-Code-jetbrains-thinkrail",
    sid: SUBAGENT,
  });

  expect(chip).toEqual({
    project: "…thinkrail",
    session: "3f2a1b4c",
    subagent: true,
  });
  expect(sessionChip({ parent: null, project: "p", sid: PARENT })).toEqual({
    project: "p",
    session: "3f2a1b4c",
    subagent: false,
  });
});

test("a subagent link opens the subagent through its parent", () => {
  const params = new URLSearchParams(
    markedSessionHash({
      id: SUBAGENT,
      mark: "piped-exit-code",
      parent: PARENT,
    }).split("?")[1]
  );

  expect(params.get("id")).toBe(SUBAGENT);
  expect(params.get("parent")).toBe(PARENT);
  expect(params.get("mark")).toBe("piped-exit-code");
  expect(params.get("view")).toBe("chat");
  expect(
    markedSessionHash({ id: PARENT, mark: "", parent: null }).includes(
      "parent="
    )
  ).toBe(false);
});

test("a drill link names the call to scroll to", () => {
  const params = new URLSearchParams(
    markedSessionHash({
      call: "toolu_42",
      id: PARENT,
      mark: "retry",
      parent: null,
    }).split("?")[1]
  );

  expect(params.get("call")).toBe("toolu_42");

  for (const call of [undefined, null]) {
    expect(
      markedSessionHash({ call, id: PARENT, mark: "", parent: null }).includes(
        "call="
      )
    ).toBe(false);
  }
});

const causeRow = (over: Partial<CauseRow>): CauseRow => ({
  cause: "zsh-glob-flag",
  details: [{ hits: 1191, value: "*.ts" }],
  diffs: [{ from: "--include=*.ts", pairs: 135, to: "--include='*.ts'" }],
  failing: true,
  fix: "quote every glob passed as a flag value",
  flagged: 34,
  hits: 1488,
  hitsBy: { main: 125, nested: 1363 },
  key: "cause:zsh-glob-flag",
  kind: "rule",
  label: "unquoted glob in a flag value",
  pairSamples: [],
  pairs: 675,
  pairsBy: { main: 41, nested: 634 },
  per1k: { main: 25.8, nested: 53.1 },
  projects: 4,
  retryCtxTokens: 32_313_906,
  retrySec: 2660,
  retrySessions: 550,
  rule: null,
  samples: [],
  secMeasured: 675,
  sessions: 914,
  tokensMeasured: 675,
  ...over,
});

const FIRING: RuleCheck = {
  advice: "setopt nonomatch in ~/.zshenv",
  callsSince: { main: 1348, nested: 7765 },
  hitsSince: { main: 42, nested: 442 },
  matches: [
    {
      excerpt: "Quote globs",
      line: 100,
      path: "~/.claude/CLAUDE.md",
    },
  ],
  per1kSince: { main: 31.16, nested: 56.92 },
  sinceIso: "2026-10-15T19:06:47.272Z",
};

/** A report with no row to prompt for: buttons that need one render nothing. */
const NO_PROMPT = () => null;

test("a per-1k rate keeps two digits and never reads 0 for a side with hits", () => {
  expect(rateText(31.16, 42)).toBe("31");
  expect(rateText(4.24, 1)).toBe("4.2");
  expect(rateText(0.001, 1)).toBe("<0.01");
  expect(rateText(0, 0)).toBe("0");
});

/** The rule line core writes for `FIRING`. */
const FIRING_LINE =
  "rule in ~/.claude/CLAUDE.md:100 · still firing main 31/1k, sub 57/1k since 2026-10-15";

/** A cause finding as core writes it for `causeRow({})`. */
const CAUSE_FINDING: Finding = {
  detector: "zsh-glob-flag",
  impact: "675 retries (main 41 / sub 634) · 44m20s · 32.3M ctx tokens re-read",
  key: "cause:zsh-glob-flag",
  note: "quote every glob passed as a flag value",
  rule: "quote every glob passed as a flag value",
  samples: [],
  spread: "1,488 hits (34 at exit 0) · 550 sessions with a retry",
  table: "cause",
  title: "unquoted glob in a flag value",
};

test("the findings band renders core's findings and suggested rules", () => {
  const text = statsText({
    findings: [CAUSE_FINDING],
    prompts: {
      cause: { [CAUSE_FINDING.key]: "Goal: x" },
      fail: {},
      report: "Goal: report",
      time: {},
    },
    suggested: ["quote every glob passed as a flag value"],
  });

  const html = renderToStaticMarkup(
    <Findings
      findings={text.findings}
      onDrill={() => {}}
      onShowAll={() => {}}
      prompt={promptsOf(text)}
      working={[]}
      showAll={false}
      suggested={text.suggested}
    />
  );

  expect(html).toContain('data-testid="stats-copy-all"');
  expect(html).toContain('data-testid="finding-copy"');
  expect(html).toContain('data-testid="finding-prompt"');
  expect(html).toContain("retries");
  expect(html).toContain(CAUSE_FINDING.impact);
});

/** A cost finding as core writes it: no table, so no drill or prompt. */
const COST_FINDING: Finding = {
  detector: null,
  impact: "~$95 of ~$129 (73.6%)",
  key: "fanout",
  note: "Spawn fewer agents per task, or give each a shorter prompt.",
  rule: null,
  samples: [],
  spread: "43 agent transcripts from 8 sessions",
  table: null,
  title: "Subagents and workflow agents",
};

test("a cost finding shows no drill, and the first three findings show before Show more", () => {
  const findings = [
    COST_FINDING,
    CAUSE_FINDING,
    { ...CAUSE_FINDING, key: "cause:b" },
    { ...CAUSE_FINDING, key: "cause:c" },
  ];

  const html = renderToStaticMarkup(
    <Findings
      findings={findings}
      onDrill={() => {}}
      onShowAll={() => {}}
      prompt={NO_PROMPT}
      showAll={false}
      suggested={[]}
      working={[]}
    />
  );

  expect(html.split('data-testid="stats-finding"').length - 1).toBe(3);
  expect(html).toContain("Show 1 more");
  expect(html).toContain(">cost<");
  expect(html.split("All calls").length - 1).toBe(2);
});

test("causes a rule stopped sit in a collapsed Working list", () => {
  const html = renderToStaticMarkup(
    <Findings
      findings={[CAUSE_FINDING]}
      onDrill={() => {}}
      onShowAll={() => {}}
      prompt={NO_PROMPT}
      showAll={false}
      suggested={[]}
      working={[{ ...CAUSE_FINDING, key: "cause:zsh-glob-path" }]}
    />
  );

  expect(html).toContain('data-testid="stats-working"');
  expect(html).toContain("Working (1)");
  // Collapsed: the stopped cause's card is not rendered until opened.
  expect(html).not.toContain('data-testid="stats-working-finding"');
});

test("no finding says so in core's words", () => {
  const html = renderToStaticMarkup(
    <Findings
      findings={[]}
      onDrill={() => {}}
      onShowAll={() => {}}
      prompt={NO_PROMPT}
      working={[]}
      showAll={false}
      suggested={[]}
    />
  );

  expect(html).toContain("No row in this window carries a named fix.");
});

test("the cause table renders its rows, and the open one shows the rule check", () => {
  const html = renderToStaticMarkup(
    <CauseTable
      onSelect={() => {}}
      prompt={NO_PROMPT}
      rows={[causeRow({ rule: FIRING })]}
      selectedKey="cause:zsh-glob-flag"
      text={statsText({ ruleLines: { "cause:zsh-glob-flag": FIRING_LINE } })}
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
  expect(html).toContain("--include=*.ts → --include=&#x27;*.ts&#x27; (135)");
  // The split column says it counts hits, and the detail's rate names its
  // window, apart from the rule line's rate since the file changed.
  expect(html).toContain("Hits main / sub");
  expect(html).toContain("per 1k calls in this window main 26 / sub 53");
});

test("a phone reads each cause row as one wrapped summary line", () => {
  const html = renderToStaticMarkup(
    <CauseTable
      onSelect={() => {}}
      prompt={NO_PROMPT}
      rows={[causeRow({ rule: FIRING })]}
      selectedKey={null}
      text={statsText({ ruleLines: { "cause:zsh-glob-flag": FIRING_LINE } })}
    />
  );

  expect(html).toContain('data-testid="cause-narrow"');
  expect(html).toContain(
    "1,488 hits · 675 retries in 550 sessions · 44m20s · 32.3M ctx re-read"
  );
  expect(html).toContain(FIRING_LINE);
});

/** The class list of a finding's impact span, from static markup. */
const IMPACT_CLASS =
  /<span class="(?<cls>[^"]*)" data-testid="stats-finding-impact"/u;

test("a finding's impact wraps instead of running past the card", () => {
  const html = renderToStaticMarkup(
    <Findings
      findings={[CAUSE_FINDING]}
      onDrill={() => {}}
      onShowAll={() => {}}
      prompt={NO_PROMPT}
      working={[]}
      showAll={false}
      suggested={[]}
    />
  );

  const impact = IMPACT_CLASS.exec(html);
  expect(impact?.groups?.cls).toContain("min-w-0");
  expect(impact?.groups?.cls).not.toContain("shrink-0");
});

/** The accessible name of each prompt button in static markup. */
const PROMPT_LABEL = /aria-label="(?<label>Copy prompt: investigate [^"]*)"/gu;

test("every open row and the header copy an agent prompt, each named for its row", () => {
  const cause = causeRow({ rule: FIRING });

  const fail = failRow({
    key: "scan:tsc",
    label: "error TS####",
    note: "pipes",
  });

  const time = timeRow({ key: "test", label: "tests" });

  const text = statsText({
    prompts: {
      cause: { [cause.key]: "Goal: cause" },
      fail: { [fail.key]: "Goal: fail" },
      report: "Goal: report",
      time: { [time.key]: "Goal: time" },
    },
    ruleLines: { [cause.key]: FIRING_LINE },
  });

  const r = report({ causes: [cause], fails: [fail], text, time: [time] });
  const prompt = promptsOf(text);

  const html = [
    renderToStaticMarkup(
      <ReportHeader
        agent={null}
        days={30}
        onAgent={() => {}}
        onDays={() => {}}
        onRescan={() => {}}
        prompt={prompt({ table: "report" })}
        report={r}
        rescanning={false}
      />
    ),
    renderToStaticMarkup(
      <CauseTable
        onSelect={() => {}}
        prompt={prompt}
        rows={[cause]}
        selectedKey={cause.key}
        text={text}
      />
    ),
    renderToStaticMarkup(
      <FailTable
        onSelect={() => {}}
        prompt={prompt}
        rows={[fail]}
        selectedKey={fail.key}
      />
    ),
    renderToStaticMarkup(
      <TimeTable
        onSelect={() => {}}
        prompt={prompt}
        rows={[time]}
        selectedKey={time.key}
        timeNotes={text.timeNotes}
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

  expect([...html.matchAll(PROMPT_LABEL)].map((m) => m.groups?.label)).toEqual([
    "Copy prompt: investigate this report",
    "Copy prompt: investigate unquoted glob in a flag value",
    "Copy prompt: investigate error TS####",
    "Copy prompt: investigate tests time",
  ]);
  // The tooltip names what is copied, not the whole prompt.
  expect(html).not.toContain('title="Goal:');
  expect(html).toContain("Copy prompt");
});

test("a closed row renders no prompt button", () => {
  const html = renderToStaticMarkup(
    <FailTable
      onSelect={() => {}}
      prompt={() => "Goal: x"}
      rows={[failRow({ key: "scan:tsc" })]}
      selectedKey={null}
    />
  );

  expect(html).not.toContain('data-testid="fail-prompt"');
});

test("a prompt for a row core wrote none for is null", () => {
  const prompt = promptsOf(
    statsText({
      prompts: { cause: {}, fail: { a: "Goal: a" }, report: "r", time: {} },
    })
  );

  expect(prompt({ key: "a", table: "fail" })).toBe("Goal: a");
  expect(prompt({ key: "b", table: "fail" })).toBeNull();
  expect(prompt({ key: "a", table: "cluster" })).toBeNull();
  expect(prompt({ table: "report" })).toBe("r");
});
