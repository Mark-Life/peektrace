/** Serve-stage cases for the paged session routes. TS has no such routes, so
 * each Rust part is compared with the matching cut of the TS analysis:
 * pages concatenated are its events, the summary is the rest of it, the
 * index and growth are projections of it. Turn numbers and search hits come
 * from the inspector's former in-browser rules, recorded like any TS value.
 * Hashes are opaque: they are only checked against each other. */

import type {
  AnalyzedSession,
  TimelineEvent,
} from "../../packages/core/src/services/sessions/schema";
import {
  crossEvtIdx,
  searchIndexOf,
  turnNumbers,
} from "./parity-history-rules";
import { tsValue } from "./parity-oracle";
import { api, jsonDiff, type RpcExit } from "./parity-serve-lib";

interface Tally {
  cases: number;
}

/** Differences found, as report lines; the caller reports or, for a session
 *  written meanwhile, drops them. */
type Problems = string[];

const same = (out: Problems, label: string, ts: unknown, rust: unknown) => {
  for (const line of jsonDiff(ts, rust)) {
    out.push(`serve ${label}: ${line}`);
  }
};

type Json = Record<string, unknown>;

const PAGE = 100;
const LIMITS = [PAGE, 7, 500] as const;
const BAD_BOUNDS = ["limit=0", "limit=501", "offset=-1", "offset=1.5"];
const QUERIES = [
  "a",
  "Read",
  "TOOL",
  " ",
  "\n",
  "İ",
  "Σ",
  "ß",
  "ς",
  "zzz-no-match",
];
/** UTF-16 units of the derived body query. */
const PROBE_UNITS = 12;
const STACK = [
  "system_tools",
  "listings",
  "memory",
  "files",
  "prompts",
  "tool_results",
  "assistant_text",
  "thinking",
  "other",
  "unattributed",
] as const;
const ROUTES = [
  "summary",
  "growth",
  "history",
  "history/index",
  "history/search?q=a",
];

const omit = (v: Json, keys: readonly string[]) =>
  Object.fromEntries(Object.entries(v).filter(([k]) => !keys.includes(k)));

const LEADING_MARK = /^\?/;

const withQuery = (path: string, q: string, extra: string) => {
  const all = [q.replace(LEADING_MARK, ""), extra].filter(Boolean).join("&");
  return all === "" ? path : `${path}?${all}`;
};

/** The index row `/history/index` serves for event `pos`. */
const rowOf = (
  e: TimelineEvent,
  extra: { readonly bodyId: number; readonly turn: number }
) => {
  const out: Json = {};
  for (const [k, v] of Object.entries(e)) {
    if (k === "contextEdit" && v && typeof v === "object") {
      out[k] = omit(v as Json, ["original"]);
    } else if (!["body", "preview", "requestId", "ts"].includes(k)) {
      out[k] = v;
    }
  }
  return {
    ...out,
    bodyId: extra.bodyId,
    bodyLen: e.body.length,
    turn: extra.turn,
  };
};

const bodyIds = (events: readonly TimelineEvent[]) => {
  const first = new Map<string, number>();
  return events.map((e, pos) => {
    const held = first.get(e.body);
    if (held !== undefined) {
      return held;
    }
    first.set(e.body, pos);
    return pos;
  });
};

const TURN_KEYS = [
  "model",
  "contextTokens",
  "outputTokens",
  "thinkingTokens",
  "effort",
  "webSearchRequests",
  "webFetchRequests",
];

/** `/growth` without its hashes, from the TS analysis. */
const growthOf = (a: AnalyzedSession) => {
  const snaps = a.snapshots;
  const slices = Object.fromEntries(
    STACK.map((k) => [k, snaps.map((s) => (s.slices as Json)[k] ?? 0)])
  );
  return {
    snapshots: {
      ctx: snaps.map((s) => s.ctx),
      outputTokens: snaps.map((s) => s.outputTokens),
      slices,
      turnIndex: snaps.map((s) => s.turnIndex),
    },
    turns: a.turns.map((t) =>
      Object.fromEntries(
        Object.entries(t).filter(([k]) => TURN_KEYS.includes(k))
      )
    ),
  };
};

/** The first `PROBE_UNITS` units of `text`, never ending on half a pair. */
const probe = (text: string) => {
  const cut = text.slice(0, PROBE_UNITS);
  const last = cut.charCodeAt(cut.length - 1);
  return last >= 0xd8_00 && last < 0xdc_00 ? cut.slice(0, -1) : cut;
};

const queriesFor = (events: readonly TimelineEvent[]) => {
  const middle = events[Math.floor(events.length / 2)];
  const derived =
    middle === undefined
      ? []
      : [probe(middle.body), probe(middle.body).toUpperCase(), middle.title];
  return [...new Set([...QUERIES, ...derived])].filter(
    (q) => q !== "" && q.isWellFormed()
  );
};

const get = async (port: number, path: string) => {
  const r = await api(port, path);
  return { json: r.json as Json, status: r.status };
};

interface SummaryHistory {
  readonly growth: string;
  readonly index: string;
  readonly pageSize: number;
  readonly pages: readonly string[];
  readonly rev: string;
  readonly total: number;
}

/** One session's checks: where to ask, what TS said, what to collect. */
interface Run {
  readonly a: AnalyzedSession;
  readonly at: (route: string, extra?: string) => string;
  readonly c: Tally;
  readonly history: SummaryHistory;
  readonly label: string;
  readonly out: Problems;
  readonly port: number;
  /** Every `rev` the parts answered with; one when nothing moved. */
  readonly revs: Set<string>;
}

const ask = async (r: Run, route: string, extra = "") => {
  const got = await get(r.port, r.at(route, extra));
  if (got.json?.rev !== undefined) {
    r.revs.add(String(got.json.rev));
  }
  return got;
};

const summaryChecks = (r: Run, summary: Json) => {
  const { a, history, label, out } = r;
  same(
    out,
    `${label} summary`,
    omit(a as unknown as Json, ["events", "turns", "snapshots"]),
    omit(summary, ["history"])
  );
  const total = a.events.length;
  const pages = Math.ceil(total / PAGE);
  if (
    history.total !== total ||
    history.pageSize !== PAGE ||
    history.pages.length !== pages
  ) {
    out.push(
      `serve ${label} summary history: ${JSON.stringify(omit(history as unknown as Json, ["pages"]))}, ${history.pages.length} pages for ${total} events`
    );
  }
};

const pageChecks = async (r: Run) => {
  const { a, history, label, out } = r;
  const total = a.events.length;
  for (const limit of LIMITS) {
    r.c.cases += 1;
    const events: unknown[] = [];
    for (let offset = 0; offset < total; offset += limit) {
      const page = await ask(r, "history", `offset=${offset}&limit=${limit}`);
      events.push(...((page.json.events as unknown[]) ?? []));
      const grid = offset / PAGE;
      if (limit === PAGE && page.json.hash !== history.pages[grid]) {
        out.push(`serve ${label} page ${grid}: hash is not the summary's`);
      }
    }
    same(out, `${label} pages limit=${limit}`, a.events, events);
  }
  r.c.cases += 1;
  const past = await ask(r, "history", `offset=${total}`);
  if (past.status !== 200 || (past.json.events as unknown[]).length !== 0) {
    out.push(`serve ${label} page past the end: ${past.status}`);
  }
  for (const bad of BAD_BOUNDS) {
    r.c.cases += 1;
    const got = await get(r.port, r.at("history", bad));
    if (got.status !== 400) {
      out.push(`serve ${label} history ${bad}: rust ${got.status} (want 400)`);
    }
  }
};

const indexChecks = async (r: Run) => {
  const { a, history, label, out } = r;
  const pinned = await tsValue(`pages turns ${label}`, () => ({
    cross: crossEvtIdx(a),
    turns: turnNumbers(a),
  }));
  const ids = bodyIds(a.events);
  r.c.cases += 1;
  const index = await ask(r, "history/index");
  const rows = a.events.map((e, pos) =>
    rowOf(e, { bodyId: ids[pos] ?? pos, turn: pinned.turns[pos] ?? 0 })
  );
  same(
    out,
    `${label} index`,
    { crossEvent: pinned.cross, rows, total: a.events.length },
    omit(index.json, ["rev", "hash"])
  );
  if (index.json.hash !== history.index) {
    out.push(`serve ${label} index: hash is not the summary's`);
  }
  r.c.cases += 1;
  const growth = await ask(r, "growth");
  same(out, `${label} growth`, growthOf(a), omit(growth.json, ["rev", "hash"]));
  if (growth.json.hash !== history.growth) {
    out.push(`serve ${label} growth: hash is not the summary's`);
  }
};

const searchChecks = async (r: Run) => {
  const { a, label, out } = r;
  const text = searchIndexOf(a.events);
  for (const q of queriesFor(a.events)) {
    r.c.cases += 1;
    const key = `pages search ${label} ${JSON.stringify(q)}`;
    const want = await tsValue(key, () => {
      const needle = q.toLowerCase();
      return text.flatMap((t, pos) => (t.includes(needle) ? [pos] : []));
    });
    const got = await ask(
      r,
      "history/search",
      new URLSearchParams({ q }).toString()
    );
    same(out, `${label} search ${JSON.stringify(q)}`, want, got.json.positions);
  }
  r.c.cases += 1;
  const noQuery = await get(r.port, r.at("history/search"));
  if (noQuery.status !== 400) {
    out.push(
      `serve ${label} search without q: rust ${noQuery.status} (want 400)`
    );
  }
};

/** Every paged route of one session, against the TS analysis `exit`. */
export const pageCases = async ({
  c,
  exit,
  label,
  path,
  port,
  query,
}: {
  readonly c: Tally;
  readonly exit: RpcExit;
  readonly label: string;
  /** `/api/sessions/<id>`. */
  readonly path: string;
  readonly port: number;
  /** The `/analysis` query, `""` or `?...`. */
  readonly query: string;
}): Promise<Problems> => {
  const out: Problems = [];
  if (exit.kind !== "ok") {
    await errorCases({ c, exit, label, out, path, port, query });
    return out;
  }
  const at = (route: string, extra = "") =>
    withQuery(`${path}/${route}`, query, extra);
  c.cases += 1;
  const summary = await get(port, at("summary"));
  if (summary.status !== 200) {
    out.push(`serve ${label} summary: rust ${summary.status}`);
    return out;
  }
  const history = summary.json.history as SummaryHistory;
  const r: Run = {
    a: exit.value as AnalyzedSession,
    at,
    c,
    history,
    label,
    out,
    port,
    revs: new Set([history.rev]),
  };
  summaryChecks(r, summary.json);
  await pageChecks(r);
  await indexChecks(r);
  await searchChecks(r);
  if (r.revs.size > 1) {
    out.push(`serve ${label}: parts answered from ${r.revs.size} revisions`);
  }
  return out;
};

/** A session TS cannot analyze: every paged route answers the same error. */
const errorCases = async ({
  c,
  exit,
  label,
  out,
  path,
  port,
  query,
}: {
  readonly c: Tally;
  readonly exit: Exclude<RpcExit, { kind: "ok" }>;
  readonly label: string;
  readonly out: Problems;
  readonly path: string;
  readonly port: number;
  readonly query: string;
}) => {
  for (const route of ROUTES) {
    c.cases += 1;
    const [p, extra = ""] = route.split("?");
    const r = await get(port, withQuery(`${path}/${p}`, query, extra));
    if (exit.kind === "die") {
      if (r.status !== 400) {
        out.push(`serve ${label} ${p}: ts die, rust ${r.status} (want 400)`);
      }
      continue;
    }
    if (r.status < 400) {
      out.push(`serve ${label} ${p}: ts fail, rust ${r.status}`);
      continue;
    }
    same(out, `${label} ${p}`, exit.error, r.json.error);
  }
};
