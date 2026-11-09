/** Formatters for the stats screen, and the findings order bound to them.
 *
 * Which rows lead a cold start, and what each one says, is core's answer
 * (`services/stats/findings`); this file only supplies how a number is spelled
 * in a browser. The terminal binds the same functions to its own primitives, so
 * neither surface can drift from the other by editing a sentence.
 */
import {
  findingsOf as coreFindingsOf,
  timeNote as coreTimeNote,
  type FindingFormat,
} from "@workspace/core/services/stats/findings";
import {
  type PromptTarget,
  statsPrompt,
} from "@workspace/core/services/stats/prompts";
import { fmtK } from "@workspace/viz/lib/session-format";

export type { Finding } from "@workspace/core/services/stats/findings";
export {
  NO_RULE,
  ruleLine,
  suggestedRules,
} from "@workspace/core/services/stats/findings";

import type { AgentId } from "@workspace/core/services/agent-id";
import type {
  SampleRef,
  StatsReport,
  TimeRow,
} from "@workspace/core/services/stats/schema";

const SEC_PER_MIN = 60;
const SEC_PER_HOUR = 3600;
const PERCENT = 100;

/** Decimals a share keeps once it is a percentage. */
const PCT_DIGITS = 2;

/** `4h12m`, `3m07s`, `0.42s` — never a bare float of 15,000 seconds. */
export const hms = (sec: number): string => {
  if (sec < SEC_PER_MIN) {
    return `${sec.toFixed(sec < 10 ? 2 : 0)}s`;
  }
  if (sec < SEC_PER_HOUR) {
    const m = Math.floor(sec / SEC_PER_MIN);
    return `${m}m${String(Math.round(sec - m * SEC_PER_MIN)).padStart(2, "0")}s`;
  }
  const h = Math.floor(sec / SEC_PER_HOUR);
  return `${h}h${String(Math.floor((sec - h * SEC_PER_HOUR) / SEC_PER_MIN)).padStart(2, "0")}m`;
};

/** A 0-1 share as a percentage string. */
export const pct = (share: number): string =>
  `${(share * PERCENT).toFixed(PCT_DIGITS)}%`;

/** Thousands separators, one call site for every count in the screen. */
export const n = (value: number): string => value.toLocaleString();

/** A short local timestamp for the report header. */
export const at = (epochMs: number): string =>
  new Date(epochMs).toLocaleString();

/** How this surface spells a number, for the sentences core composes. */
const FORMAT: FindingFormat = { count: n, hms, pct, tokens: fmtK };

/** What a time row can say for itself, from its own columns. */
export const timeNote = (row: TimeRow): string | null =>
  coreTimeNote(row, FORMAT);

/** How many lines held this job but were charged to a higher-priority one. */
export const coPresentNote = (row: TimeRow): string =>
  `${n(row.coPresentRuns)} lines hold this job and were charged elsewhere`;

/** The cold-start list: rows that carry a fix, cheapest fix first. */
export const findingsOf = (report: StatsReport) =>
  coreFindingsOf(report, FORMAT);

/** The investigation prompt for one row, or null when no row has the key. */
export type PromptFor = (target: PromptTarget) => string | null;

/** Bind core's prompt builder to this report, its agent scope and this
 * surface's number spelling, so every button copies the CLI's text. */
export const promptsOf =
  (report: StatsReport, agent: AgentId | null): PromptFor =>
  (target) =>
    statsPrompt({ agent, format: FORMAT, report, target });

/** Chars of a project slug a session chip keeps. */
const PROJECT_TAIL = 18;

/** The end of a long project slug, cut at a `-` so a chip shows the folder
 * name rather than the shared `-Users-<name>-…` head. Full slug goes in a title. */
export const projectTail = (project: string, max = PROJECT_TAIL) => {
  if (project.length <= max) {
    return project;
  }
  const tail = project.slice(-max);
  const clean = project.at(-max - 1) === "-";
  const dash = tail.indexOf("-");
  const cut =
    clean || dash < 0 || dash === tail.length - 1 ? tail : tail.slice(dash + 1);
  return `…${cut}`;
};

/** Chars of a session id a chip shows; the full id is in the title. */
const SID_HEAD = 8;

/** What a session chip says. A subagent call names the session it ran under,
 * because the subagent id alone is not a session a reader can find. */
export const sessionChip = (
  ref: Pick<SampleRef, "parent" | "project" | "sid">
) => ({
  project: projectTail(ref.project),
  session: (ref.parent ?? ref.sid).slice(0, SID_HEAD),
  subagent: ref.parent !== null,
});

/** Session titles by `titleKey`, as the sessions list knows them. */
export type SessionTitles = ReadonlyMap<string, string>;

/** The key a session title is stored under: agent plus transcript id. */
export const titleKey = (agent: AgentId, id: string) => `${agent}:${id}`;

/** Longest title a chip shows before it is cut. */
const TITLE_MAX = 40;

/** A title cut to `TITLE_MAX` chars, with an ellipsis. */
const clip = (title: string) => {
  const chars = Array.from(title);
  return chars.length > TITLE_MAX
    ? `${chars.slice(0, TITLE_MAX - 1).join("")}…`
    : title;
};

/** A title on one line, or null when it says nothing. */
const cleanTitle = (title: string | undefined) => {
  const flat = title?.replace(/\s+/g, " ").trim();
  return flat ? flat : null;
};

/** What a sample's chip says: the session title, else `project/id`. `id` is
 * the short session id, shown beside a title so equal titles stay apart. */
export const chipLabelOf = (
  ref: Pick<SampleRef, "agent" | "parent" | "project" | "sid">,
  titles: SessionTitles
) => {
  const chip = sessionChip(ref);
  const title = cleanTitle(
    titles.get(titleKey(ref.agent, ref.parent ?? ref.sid))
  );
  const fallback = `${chip.project}/${chip.session}`;
  return {
    ...chip,
    id: title ? chip.session : null,
    label: title ? clip(title) : fallback,
  };
};

/** Samples that name one session, folded into one chip. */
export interface SessionChipGroup {
  /** The sample a click opens. */
  readonly first: SampleRef;
  readonly samples: readonly SampleRef[];
}

/** Fold samples by the session they name (`agent:(parent ?? sid)`), keeping
 * first-seen order, so three subagent calls under one parent read as one chip. */
export const sessionChips = (
  samples: readonly SampleRef[]
): readonly SessionChipGroup[] => {
  const groups = new Map<string, SampleRef[]>();
  for (const sample of samples) {
    const key = titleKey(sample.agent, sample.parent ?? sample.sid);
    const group = groups.get(key);
    if (group) {
      group.push(sample);
    } else {
      groups.set(key, [sample]);
    }
  }
  return [...groups.values()].flatMap((group) =>
    group[0] ? [{ first: group[0], samples: group }] : []
  );
};
