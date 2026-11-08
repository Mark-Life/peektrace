/** Filtering the session rail: facets, the free-text query, and the
 * predicates over `SessionHeader` rows. */
import type { SessionHeader } from "@workspace/core/services/sessions/schema";
import type { DateRange } from "react-day-picker";
import { toDayKey } from "../routes/sessions/date-range-filter";

/** Human-readable label per agent id for the badge. */
export const AGENT_LABEL: Record<string, string> = {
  claude: "Claude",
  codex: "Codex",
  pi: "Pi",
  opencode: "OpenCode",
};

/** Sentinel option meaning "this facet is not constraining the list". */
export const ANY = "all";

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

/** No facet selected, no query — the state the reset action restores. */
export const NO_FILTERS: Filters = {
  query: "",
  agent: ANY,
  project: ANY,
  branch: ANY,
  model: ANY,
  range: undefined,
  source: ANY,
};

/** How many facets (query counts as one) currently constrain the list. */
export const activeCount = (f: Filters): number =>
  (f.query.length > 0 ? 1 : 0) +
  (f.range?.from ? 1 : 0) +
  [f.agent, f.project, f.branch, f.model, f.source].filter((v) => v !== ANY)
    .length;

/** Distinct, sorted non-empty values of one header field. */
export const distinct = (
  headers: readonly SessionHeader[],
  pick: (h: SessionHeader) => string | undefined
): readonly string[] =>
  [...new Set(headers.map(pick).filter((v): v is string => Boolean(v)))].sort();

/** `YYYY-MM-DD` of an ISO timestamp, or `""`. */
export const dayOf = (iso: string | undefined): string =>
  iso ? (iso.slice(0, 10) ?? "") : "";

/** The row's headline: the transcript title, else the harness that produced it
 * (`Codex session`) — mirrors how the detail header names untitled sessions. */
export const titleOf = (h: SessionHeader): string =>
  h.title || `${AGENT_LABEL[h.agent] ?? h.agent} session`;

/** Predicate: the session started on a day inside the picked range. A range with
 * no `to` yet (mid-drag, or a single click) matches that one day. */
export const inRange = (h: SessionHeader, range: DateRange): boolean => {
  if (!range.from) {
    return true;
  }
  const day = dayOf(h.startedAt);
  if (!day) {
    return false;
  }
  const from = toDayKey(range.from);
  const to = range.to ? toDayKey(range.to) : from;
  return day >= from && day <= to;
};

/** Predicate: header passes every active filter + the title/id search. */
export const matches = (h: SessionHeader, f: Filters): boolean => {
  if (f.agent !== ANY && h.agent !== f.agent) {
    return false;
  }
  if (f.project !== ANY && h.project !== f.project) {
    return false;
  }
  if (f.branch !== ANY && h.gitBranch !== f.branch) {
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
