/** Formatters for the stats screen, and access to the text core wrote.
 *
 * Which rows lead a cold start, and every sentence about them, come from core
 * in the report's `text`; this file only spells the numbers of table cells in
 * a browser.
 */
import type {
  AgentId,
  DrillTable,
  SampleRef,
  StatsText,
  TimeRow,
} from "@workspace/view-core/api-types.gen";

export { NO_RULE } from "@workspace/view-core/constants.gen";

const SEC_PER_MIN = 60;
const SEC_PER_HOUR = 3600;
const PERCENT = 100;

/** Decimals a share keeps once it is a percentage. */
const PCT_DIGITS = 1;

/** Under this many seconds a duration keeps two decimals. */
const SUB_TEN_SECONDS = 10;
const FRACTION_DIGITS = 2;
const PAD_WIDTH = 2;

/** `4h12m`, `3m07s`, `0.42s` — never a bare float of 15,000 seconds. */
export const hms = (sec: number): string => {
  if (sec < SEC_PER_MIN) {
    return `${sec.toFixed(sec < SUB_TEN_SECONDS ? FRACTION_DIGITS : 0)}s`;
  }

  if (sec < SEC_PER_HOUR) {
    const m = Math.floor(sec / SEC_PER_MIN);

    return `${m}m${String(Math.round(sec - m * SEC_PER_MIN)).padStart(PAD_WIDTH, "0")}s`;
  }

  const h = Math.floor(sec / SEC_PER_HOUR);

  return `${h}h${String(Math.floor((sec - h * SEC_PER_HOUR) / SEC_PER_MIN)).padStart(PAD_WIDTH, "0")}m`;
};

/** A 0-1 share as a percentage string; a share too small to show reads
 * `<0.1%`, never `0.0%`. */
export const pct = (share: number): string => {
  const fixed = (share * PERCENT).toFixed(PCT_DIGITS);

  return share > 0 && Number(fixed) === 0 ? "<0.1%" : `${fixed}%`;
};

/** Thousands separators, one call site for every count in the screen. */
export const n = (value: number): string => value.toLocaleString();

/** `~$26`, or `<$1`: a priced figure that is an estimate, spelled as core
 * spells it. */
export const approxUsd = (usd: number): string =>
  usd < 1 ? "<$1" : `~$${n(Math.round(usd))}`;

/** A short local timestamp for the report header. */
export const at = (epochMs: number): string =>
  new Date(epochMs).toLocaleString();

/** Rates at or above this print as whole numbers. */
const RATE_WHOLE = 10;

/** Significant digits of a smaller rate. */
const RATE_DIGITS = 2;

/** The smallest rate kept; a side with hits below it prints `<0.01`. */
const RATE_FLOOR = 0.01;

/** A per-1k rate: `41`, `4.2`, `0.31`. A side with hits never reads `0`. */
export const rateText = (rate: number, hits: number): string => {
  if (hits > 0 && rate < RATE_FLOOR) {
    return `<${RATE_FLOOR}`;
  }

  return rate >= RATE_WHOLE
    ? Math.round(rate).toLocaleString("en-US")
    : String(Number(rate.toPrecision(RATE_DIGITS)));
};

/** How many lines held this job but were charged to a higher-priority one. */
export const coPresentNote = (row: TimeRow): string =>
  `${n(row.coPresentRuns)} lines also run this job but are charged to another one`;

/** What a prompt is about: the whole report, or one row of a table. */
export type PromptTarget =
  | { readonly table: "report" }
  | { readonly table: DrillTable; readonly key: string };

/** The investigation prompt for one row, or null when no row has the key. */
export type PromptFor = (target: PromptTarget) => string | null;

/** Core's prompt for one target, or null when it wrote none. */
const promptIn = (text: StatsText, target: PromptTarget): string | null => {
  if (target.table === "report") {
    return text.prompts.report;
  }

  if (target.table === "cluster" || target.table === "wait") {
    return null;
  }

  return text.prompts[target.table][target.key] ?? null;
};

/** Core's prompts for this report, as the CLI writes them. */
export const promptsOf =
  (text: StatsText): PromptFor =>
  (target) =>
    promptIn(text, target);

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
    clean || dash === -1 || dash === tail.length - 1
      ? tail
      : tail.slice(dash + 1);

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
  // oxlint-disable-next-line typescript/no-misused-spread -- counting code points, not UTF-16 units, is the point
  const chars = [...title];

  return chars.length > TITLE_MAX
    ? `${chars.slice(0, TITLE_MAX - 1).join("")}…`
    : title;
};

/** A title on one line, or null when it says nothing. */
const cleanTitle = (title: string | undefined) => {
  const flat = title?.replaceAll(/\s+/gu, " ").trim();

  return flat === undefined || flat === "" ? null : flat;
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
    id: title === null ? null : chip.session,
    label: title === null ? fallback : clip(title),
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
