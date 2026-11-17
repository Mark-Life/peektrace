/** Pure helpers behind the session rail: filtering, ordering and row labels. */
import type { SessionHeader } from "@workspace/view-core/api-types.gen";
import { sessionBranch } from "@workspace/view-core/session-marks";
import type { DateRange } from "react-day-picker";

import { AGENT_LABEL } from "../../lib/agent-label";
import type { ListKey } from "../../lib/routes";
import { fromDayKey, toDayKey } from "./date-range-filter";

/** Sentinel option meaning "this facet is not constraining the list". */
export const ANY = "all";

/** A session whose last line is newer than this counts as live. Long tool
 * calls write nothing for minutes, so the window is generous. */
export const LIVE_MS = 300_000;

/** Clock skew tolerated for a last line stamped slightly in the future. */
export const SKEW_MS = 60_000;

/** Short month names, indexed by `MM - 1` from an ISO timestamp. */
const MONTHS = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
];

/** The facets a header can be narrowed by, plus the free-text query. */
export interface Filters {
  readonly agent: string;
  readonly branch: string;
  readonly model: string;
  readonly project: string;
  readonly query: string;
  readonly range: DateRange | undefined;
  readonly source: string;
}

/** No facet selected, no query: the state the reset action restores. */
export const NO_FILTERS: Filters = {
  agent: ANY,
  branch: ANY,
  model: ANY,
  project: ANY,
  query: "",
  range: undefined,
  source: ANY,
};

/** Shape of a calendar day key. */
const DAY_KEY = /^\d{4}-\d{2}-\d{2}$/u;

/** Non-empty string; `undefined`, `null` and `""` are not. */
const hasText = (value: string | null | undefined): value is string =>
  value !== undefined && value !== null && value !== "";

/** A `YYYY-MM-DD` hash value as a local day, or `undefined` if malformed or
 * not a real calendar day (`2026-02-31` would otherwise roll into March). */
const dayParam = (value: string | null) => {
  const day =
    value !== null && DAY_KEY.test(value) ? fromDayKey(value) : undefined;

  return day && toDayKey(day) === value ? day : undefined;
};

/** The range in `from`/`to`; a `to` without a valid `from` is ignored. A
 * missing `to` reads as a one-day range, and a reversed pair is swapped. */
const rangeFromParams = (params: URLSearchParams): DateRange | undefined => {
  const from = dayParam(params.get("from"));

  if (from === undefined) {
    return from;
  }

  const to = dayParam(params.get("to")) ?? from;

  return to < from ? { from: to, to: from } : { from, to };
};

/** The string filters and the hash key each one is stored under. */
const FILTER_KEY = {
  agent: "agent",
  branch: "branch",
  model: "model",
  project: "project",
  query: "q",
  source: "source",
} as const satisfies Record<Exclude<keyof Filters, "range">, ListKey>;

/** The string filter names, in hash key order. */
const STRING_FILTERS = [
  "query",
  "agent",
  "project",
  "branch",
  "model",
  "source",
] as const satisfies readonly (keyof typeof FILTER_KEY)[];

/** Filters read from hash params; a missing or empty key is the default. */
export const filtersFromParams = (params: URLSearchParams): Filters => ({
  ...NO_FILTERS,
  ...Object.fromEntries(
    STRING_FILTERS.map((name) => {
      const value = params.get(FILTER_KEY[name]);

      return [name, hasText(value) ? value : NO_FILTERS[name]];
    })
  ),
  range: rangeFromParams(params),
});

/** `value`, or `null` when it is the default, so defaults stay out of the URL. */
const orNull = (value: string, fallback: string) =>
  value === fallback ? null : value;

/** The `from`/`to` hash values of a range; `to` only when it is a later day. */
const rangeToParams = (range: DateRange | undefined) => {
  const from = range?.from ? toDayKey(range.from) : null;
  const to = from !== null && range?.to ? toDayKey(range.to) : null;

  return { from, to: to !== null && from !== null && to > from ? to : null };
};

/** Hash params for the filters in `f`; only the keys present in `f` are set. */
export const filtersToParams = (
  f: Partial<Filters>
): Partial<Record<ListKey, string | null>> => {
  const facets = Object.fromEntries(
    STRING_FILTERS.flatMap((name) => {
      const value = f[name];

      return value === undefined
        ? []
        : [[FILTER_KEY[name], orNull(value, NO_FILTERS[name])]];
    })
  );

  return "range" in f ? { ...facets, ...rangeToParams(f.range) } : facets;
};

/** A filter option: `value` is matched against the header, `label` is shown. */
export interface FilterOption {
  readonly label: string;
  readonly value: string;
}

/** How many facets (query counts as one) currently constrain the list. */
export const activeCount = (f: Filters) =>
  (f.query.length > 0 ? 1 : 0) +
  (f.range?.from ? 1 : 0) +
  [f.agent, f.project, f.branch, f.model, f.source].filter((v) => v !== ANY)
    .length;

/** Distinct, sorted non-empty values of one header field. */
export const distinct = (
  headers: readonly SessionHeader[],
  pick: (h: SessionHeader) => string | undefined
): readonly string[] =>
  [
    ...new Set(
      headers.flatMap((h) => {
        const value = pick(h);

        return hasText(value) ? [value] : [];
      })
    ),
  ].toSorted();

/** Length of the `YYYY-MM-DD` head of an ISO timestamp. */
const ISO_DAY_LENGTH = 10;

/** `YYYY-MM-DD` of an ISO timestamp, or `""` when it does not start with a
 * real calendar day. */
const dayOf = (iso: string | undefined) => {
  const day = iso?.slice(0, ISO_DAY_LENGTH) ?? "";

  return DAY_KEY.test(day) && !Number.isNaN(Date.parse(day)) ? day : "";
};

/** Captures `YYYY`-`MM`-`DD`T`HH:MM` from the head of an ISO timestamp. */
const ISO_STAMP = /^\d{4}-(?<month>\d{2})-(?<day>\d{2})T(?<time>\d{2}:\d{2})/u;

/** `Jul 9, 08:18` of an ISO timestamp, or `"—"`. Read off the string rather than
 * `Date`-parsed, so the rendered wall-clock matches what the transcript recorded. */
export const fmtStamp = (iso: string | undefined) => {
  const groups = iso?.match(ISO_STAMP)?.groups;

  const month =
    groups === undefined ? undefined : MONTHS[Number(groups.month) - 1];

  if (groups === undefined || month === undefined) {
    return "—";
  }

  return `${month} ${Number(groups.day)}, ${groups.time}`;
};

/** Tooltip for the row stamp: the shown time is the last activity, so name it
 * and add the start when it differs. */
export const stampTitle = (
  h: Pick<SessionHeader, "startedAt" | "updatedAt">
) => {
  const last = h.updatedAt ?? h.startedAt;

  if (!hasText(last)) {
    return "unknown time";
  }

  const started =
    hasText(h.startedAt) && h.startedAt !== last
      ? ` · started ${fmtStamp(h.startedAt)}`
      : "";

  return `Last activity ${fmtStamp(last)}${started}`;
};

/** The row's headline: the transcript title, else the harness that produced it
 * (`Codex session`), as the detail header names untitled sessions. */
export const titleOf = (h: SessionHeader) =>
  hasText(h.title) ? h.title : `${AGENT_LABEL[h.agent] ?? h.agent} session`;

/** Either path separator. */
const PATH_SEP = /[\\/]/u;

/** Non-empty path segments, split on either separator. */
const segments = (path: string) => path.split(PATH_SEP).filter(Boolean);

/** The header fields that name a row's project. */
type ProjectFields = Pick<SessionHeader, "cwd" | "project" | "worktree">;

/** The folder that names the project: a worktree's main checkout, else `cwd`. */
const projectPath = (h: ProjectFields) =>
  h.worktree?.projectRoot ?? (hasText(h.cwd) ? h.cwd : undefined);

/** A worktree's parent project, else the folder the session ran in (last
 * segment of `cwd`), else the stored slug. */
export const projectLabel = (h: ProjectFields) =>
  h.worktree?.project ??
  (hasText(h.cwd) ? segments(h.cwd).at(-1) : undefined) ??
  h.project;

/** The project facet key: a worktree's main checkout, else the folder the
 * session ran in, else the stored slug. Harnesses encode the slug differently,
 * so the folder groups them. */
export const projectKey = (h: ProjectFields) => projectPath(h) ?? h.project;

/** Labels with more than one owner in `labels`. */
const collisions = (labels: readonly string[]) => {
  const seen = new Set<string>();
  const dup = new Set<string>();

  for (const l of labels) {
    (seen.has(l) ? dup : seen).add(l);
  }

  return dup;
};

/** Trailing path segments shown for a folder name shared by two folders. */
const PARENT_SEGMENTS = 2;

/** Project filter options, one per folder across every harness. Folder names
 * shared by two folders get their parent folder too; still-equal labels fall
 * back to the full key, so the options stay distinct. */
export const projectOptions = (
  headers: readonly ProjectFields[]
): readonly FilterOption[] => {
  const byKey = new Map<string, ProjectFields>();

  for (const h of headers) {
    const key = projectKey(h);

    if (key && !byKey.has(key)) {
      byKey.set(key, h);
    }
  }

  const entries = [...byKey];
  const short = collisions(entries.map(([, h]) => projectLabel(h)));

  const first = entries.map(([value, h]) => {
    const label = projectLabel(h);
    const path = projectPath(h);

    return {
      label:
        short.has(label) && path !== undefined
          ? segments(path).slice(-PARENT_SEGMENTS).join("/")
          : label,
      value,
    };
  });

  const long = collisions(first.map((o) => o.label));

  return first
    .map((o) => (long.has(o.label) ? { ...o, label: o.value } : o))
    .toSorted((a, b) => a.label.localeCompare(b.label));
};

/** The branch facet value: the shown branch, else the raw one, so a detached
 * `HEAD` stays filterable while a worktree row filters by its own branch. */
export const branchFacet = (h: Pick<SessionHeader, "gitBranch" | "worktree">) =>
  sessionBranch(h) ?? h.gitBranch;

/** Epoch ms of an ISO stamp; 0 if missing or unparseable. */
const msOf = (iso: string | undefined) => {
  const ms = Date.parse(iso ?? "");

  return Number.isNaN(ms) ? 0 : ms;
};

/** Epoch ms of the last activity (`updatedAt`, else `startedAt`), skipping a
 * stamp more than `SKEW_MS` ahead of `now` as a bad clock; 0 if none is left. */
const recentKey = (
  h: Pick<SessionHeader, "startedAt" | "updatedAt">,
  now: number
) =>
  [msOf(h.updatedAt), msOf(h.startedAt)].find(
    (ms) => ms > 0 && ms <= now + SKEW_MS
  ) ?? 0;

/** Newest activity first; ties by id so the order is stable across refetches. */
export const sortByRecent = <
  H extends Pick<SessionHeader, "id" | "startedAt" | "updatedAt">,
>(
  headers: readonly H[],
  now = Date.now()
) =>
  headers
    .map((h) => ({ h, key: recentKey(h, now) }))
    .toSorted((a, b) => b.key - a.key || a.h.id.localeCompare(b.h.id))
    .map(({ h }) => h);

/** Predicate: the session wrote a line within `LIVE_MS` of `now`. */
export const isLive = (
  h: Pick<SessionHeader, "startedAt" | "updatedAt">,
  now: number
) => {
  const key = recentKey(h, now);

  return key > 0 && now - key < LIVE_MS;
};

/** First and last day the session was active (`startedAt` to `updatedAt`),
 * either end standing in for a missing one; `undefined` with no time at all. */
const activeSpan = (h: Pick<SessionHeader, "startedAt" | "updatedAt">) => {
  const start = dayOf(h.startedAt);
  const end = dayOf(h.updatedAt);
  const from = start || end;
  const to = end || start;

  return from ? { from, to: to < from ? from : to } : undefined;
};

const DAY_MS = 86_400_000;

/** Upper bound on days listed per session, so a bad timestamp cannot blow up. */
const MAX_SPAN_DAYS = 366;

/** `YYYY-MM-DD` of the day after `day`, stepped in UTC so no zone shifts it. */
const nextDay = (day: string) =>
  new Date(Date.parse(`${day}T00:00:00Z`) + DAY_MS)
    .toISOString()
    .slice(0, ISO_DAY_LENGTH);

/** Every `YYYY-MM-DD` the session was active on, oldest first. */
export const activeDays = (
  h: Pick<SessionHeader, "startedAt" | "updatedAt">
): readonly string[] => {
  const span = activeSpan(h);

  if (!span) {
    return [];
  }

  const days = [span.from];
  let day = span.from;

  while (day < span.to && days.length < MAX_SPAN_DAYS) {
    day = nextDay(day);
    days.push(day);
  }

  return days;
};

/** Predicate: the session was active on some day inside the picked range. A
 * range with no `to` yet (mid-drag, or a single click) matches that one day. */
const inRange = (h: SessionHeader, range: DateRange) => {
  if (!range.from) {
    return true;
  }

  const span = activeSpan(h);

  if (!span) {
    return false;
  }

  const from = toDayKey(range.from);
  const to = range.to ? toDayKey(range.to) : from;

  return span.from <= to && span.to >= from;
};

/** Predicate: header passes every active filter + the title/id search. */
export const matches = (h: SessionHeader, f: Filters) => {
  if (f.agent !== ANY && h.agent !== f.agent) {
    return false;
  }

  if (f.project !== ANY && projectKey(h) !== f.project) {
    return false;
  }

  if (f.branch !== ANY && branchFacet(h) !== f.branch) {
    return false;
  }

  if (f.model !== ANY && h.model !== f.model) {
    return false;
  }

  if (f.source !== ANY && (h.sourceLabel ?? "") !== f.source) {
    return false;
  }

  if (f.range && !inRange(h, f.range)) {
    return false;
  }

  if (f.query.length > 0) {
    const hay = `${titleOf(h)} ${h.id}`.toLowerCase();

    return hay.includes(f.query.toLowerCase());
  }

  return true;
};

/** The filters that hide `h`, each set back to its default; empty when `h`
 * passes. Every filter is checked on its own, so the rest stay as they are. */
export const hidingFilters = (h: SessionHeader, f: Filters) => {
  const hides = (name: keyof Filters) =>
    !matches(h, { ...NO_FILTERS, [name]: f[name] });

  const hidden: { -readonly [K in keyof Filters]?: Filters[K] } = {};

  for (const name of STRING_FILTERS) {
    if (hides(name)) {
      hidden[name] = NO_FILTERS[name];
    }
  }

  if (hides("range")) {
    hidden.range = NO_FILTERS.range;
  }

  return hidden;
};
