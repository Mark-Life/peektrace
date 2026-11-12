/** The TS answers to the `ui_helpers` probe's questions: the inspector's
 * real list, format, hit-test, chat-plan and tool-pane helpers, and the
 * growth chart rendered to SVG and read back. */

import {
  buildChatPlan,
  type ChatNode,
  chatIdsOf,
  chatNodeKeys,
  chipLabel,
  laneOf,
  needsClamp,
} from "../../apps/inspector/src/lib/chat-lane";
import {
  crossEvtIdx,
  turnNumbers,
} from "../../apps/inspector/src/lib/history-turns";
import {
  bodyView,
  callView,
  indexTools,
  resultPane,
  type ToolPane,
  toolNameOf,
  toolState,
} from "../../apps/inspector/src/lib/tool-event";
import {
  activeCount,
  activeDays,
  type Filters,
  fmtStamp,
  hidingFilters,
  isLive,
  matches,
  NO_FILTERS,
  projectKey,
  projectLabel,
  projectOptions,
  sortByRecent,
  stampTitle,
  titleOf,
  visibleBranch,
} from "../../apps/inspector/src/routes/sessions/session-list-model";
import { costSummary } from "../../packages/core/src/services/sessions/cost";
import type {
  AnalyzedSession,
  SessionHeader,
  TimelineEvent,
} from "../../packages/core/src/services/sessions/schema";
import { analyzedTitle } from "../../packages/viz/src/components/verdict-header";
import {
  firstLine,
  fmt,
  fmtBytes,
  fmtK,
  fmtPct,
  zoneOf,
} from "../../packages/viz/src/lib/session-format";
import {
  clickAction,
  snapIndexAtX,
} from "../../packages/viz/src/lib/timeline-hit";

/** Tool bodies past this many chars are skipped on both sides. */
export const BODY_MAX = 200_000;

const PROVIDER_LABEL: Record<string, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  pi: "Pi",
  opencode: "OpenCode",
};

export type FilterCase = Partial<Omit<Filters, "range">>;

export const listAnswer = (
  headers: readonly SessionHeader[],
  now: number,
  cases: readonly FilterCase[]
) => ({
  per: headers.map((h) => ({
    stamp: fmtStamp(h.updatedAt),
    stampTitle: stampTitle(h),
    title: titleOf(h),
    projectLabel: projectLabel(h),
    projectKey: projectKey(h),
    branch: visibleBranch(h.gitBranch) ?? null,
    live: isLive(h, now),
    days: activeDays(h),
  })),
  order: sortByRecent(headers, now).map((h) => h.id),
  options: projectOptions(headers).map((o) => [o.value, o.label]),
  cases: cases.map((c) => {
    const f = { ...NO_FILTERS, ...c };
    return {
      matches: headers.map((h) => matches(h, f)),
      hiding: headers.map((h) => Object.keys(hidingFilters(h, f))),
      active: activeCount(f),
    };
  }),
});

export const numAnswer = (
  xs: readonly number[],
  zones: readonly [number, number][],
  lines: readonly string[]
) => ({
  fmt: xs.map(fmt),
  fmtK: xs.map(fmtK),
  fmtPct: xs.map(fmtPct),
  fmtBytes: xs.filter((x) => Number.isFinite(x) && x >= 0).map(fmtBytes),
  zones: zones.map(([frac, dumbZone]) => zoneOf({ frac, dumbZone })),
  firstLine: lines.map((text) => firstLine({ text })),
});

export const hitAnswer = (
  snaps: readonly [number, number, number, number][],
  clicks: readonly [number, number, boolean][]
) => ({
  snaps: snaps.map(([left, n, plotW, px]) =>
    snapIndexAtX({ left, n, plotW, px })
  ),
  clicks: clicks.map(([armed, i, touch]) => clickAction({ armed, i, touch })),
});

const node = (n: ChatNode) => {
  switch (n.type) {
    case "tool":
      return ["tool", n.call?.pos ?? null, n.result?.pos ?? null];
    case "run":
      return ["run", n.items.map((i) => i.pos), n.items.map((i) => i.repeat)];
    case "turn":
      return ["turn", n.turn];
    case "dumbzone":
      return ["dumbzone"];
    default:
      return [n.type, n.row.pos];
  }
};

const pane = (p: ToolPane) => {
  if (p.type === "diff") {
    return [p.label, "diff", p.language, null, p.diff.added, p.diff.removed];
  }
  return p.type === "code"
    ? [p.label, "code", p.language, p.code, null, null]
    : [p.label, "prose", null, p.text, null, null];
};

const isTool = (e: TimelineEvent) =>
  e.kind === "tool-call" || e.kind === "tool-result";

export const analysisAnswer = (a: AnalyzedSession) => {
  const turns = turnNumbers(a);
  const cross = crossEvtIdx(a);
  const rows = a.events.map((e, pos) => ({ e, pos }));
  const plan = buildChatPlan({ crossEvtIdx: cross, rows, turns });
  const tools = indexTools(a);
  return {
    title: analyzedTitle(a),
    turns,
    cross,
    plan: plan.map(node),
    keys: chatNodeKeys(plan),
    ids: chatIdsOf(rows),
    lanes: a.events.map(laneOf),
    chips: a.events.map(chipLabel),
    clamp: a.events.map(needsClamp),
    bodyView: a.events.map((e) => {
      const v = bodyView(e);
      return v.type === "code" ? v.language : null;
    }),
    tools: rows
      .filter(({ e }) => isTool(e) && e.body.length <= BODY_MAX)
      .map(({ e, pos }) => {
        const base = {
          pos,
          state: toolState(e, tools),
          name: toolNameOf(e, tools),
        };
        if (e.kind === "tool-call") {
          const v = callView(e);
          return { ...base, summary: v.summary, panes: v.panes.map(pane) };
        }
        const r = resultPane(e);
        return {
          ...base,
          result:
            r.type === "code"
              ? ["code", r.language, r.code]
              : ["prose", null, r.text],
        };
      }),
    cost: a.cost
      ? (() => {
          const s = costSummary(
            a.cost,
            PROVIDER_LABEL[a.provider] ?? a.provider
          );
          return [s.total, s.notes];
        })()
      : null,
  };
};
