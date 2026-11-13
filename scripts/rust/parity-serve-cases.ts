/** Serve-stage API cases: each TS `/rpc` call next to the Rust `/api` route
 * that replaces it, compared value for value. */
import { appendFileSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { report } from "./parity-lib";
import { tsValue } from "./parity-oracle";
import {
  api,
  type Pair,
  type RpcExit,
  replaceText,
  rpc,
  sameJson,
} from "./parity-serve-lib";
import { pageCases } from "./parity-serve-pages";

export type { Pair } from "./parity-serve-lib";

export interface Counts {
  cases: number;
  /** Cases of the search, stats and memory routes. */
  features: number;
  live: number;
}

interface Header {
  readonly agent: string;
  readonly id: string;
  readonly path: string;
  readonly project: string;
  readonly sizeBytes: number;
}

const byPath = (rows: readonly { path: string }[]) =>
  [...rows].sort((a, b) => a.path.localeCompare(b.path, "en"));

const query = (q: Record<string, string | number | boolean | undefined>) => {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(q)) {
    if (v !== undefined) {
      p.set(k, String(v));
    }
  }
  const s = p.toString();
  return s === "" ? "" : `?${s}`;
};

const seg = encodeURIComponent;

/** Compare a TS exit with a Rust answer: success value, or the typed error
 * and its status. */
const compareExit = async (
  c: Counts,
  label: string,
  exit: RpcExit,
  rustPath: string,
  init: RequestInit = {},
  normalize: (v: unknown) => unknown = (v) => v
) => {
  c.cases += 1;
  const r = await api(rustPort().port, rustPath, init);
  if (exit.kind === "ok") {
    if (r.status !== 200) {
      report(
        `serve ${label}: ts ok, rust ${r.status} ${JSON.stringify(r.json)}`
      );
      return;
    }
    sameJson(label, normalize(exit.value), normalize(r.json));
    return;
  }
  if (exit.kind === "die") {
    if (r.status !== 400) {
      report(`serve ${label}: ts die, rust ${r.status} (want 400)`);
    }
    return;
  }
  const error = (r.json as { error?: unknown } | undefined)?.error;
  if (r.status < 400 || error === undefined) {
    report(`serve ${label}: ts fail, rust ${r.status}`);
    return;
  }
  sameJson(label, normalize(exit.error), normalize(error));
};

let current: Pair | undefined;
const rustPort = () => {
  if (!current) {
    throw new Error("no server pair");
  }
  return current.rust;
};

/** Listing, get, analysis and image cases on the fixture pair. */
export const sessionCases = async (pair: Pair, c: Counts) => {
  current = pair;
  const ts = pair.ts.port;
  const listing = await rpc(ts, "sessions.listing", {});
  const sortListing = (v: unknown) => {
    const l = v as { headers: Header[]; unreadable: { path: string }[] };
    return { headers: byPath(l.headers), unreadable: byPath(l.unreadable) };
  };
  await compareExit(
    c,
    "sessions.listing {}",
    listing,
    "/api/sessions",
    {},
    sortListing
  );
  for (const agent of ["claude", "codex", "pi", "opencode"]) {
    await compareExit(
      c,
      `sessions.listing ${agent}`,
      await rpc(ts, "sessions.listing", { agent }),
      `/api/sessions${query({ agent })}`,
      {},
      sortListing
    );
  }
  const headers =
    listing.kind === "ok"
      ? (listing.value as { headers: Header[] }).headers
      : [];
  const claudeSlug = headers.find((h) => h.agent === "claude")?.project ?? "";
  for (const project of [claudeSlug, "demo", "no-such-project"]) {
    await compareExit(
      c,
      `sessions.listing project=${project}`,
      await rpc(ts, "sessions.listing", { project }),
      `/api/sessions${query({ project })}`,
      {},
      sortListing
    );
  }
  await compareExit(
    c,
    "sessions.list {}",
    await rpc(ts, "sessions.list", {}),
    "/api/sessions",
    {},
    (v) => byPath(Array.isArray(v) ? v : (v as { headers: Header[] }).headers)
  );
  await compareExit(
    c,
    "sessions.list bad agent",
    await rpc(ts, "sessions.list", { agent: "bogus" }),
    "/api/sessions?agent=bogus"
  );
  for (const h of headers) {
    await idCases(c, ts, h.id, undefined);
    const got = await rpc(ts, "sessions.get", { id: h.id });
    const subs =
      got.kind === "ok"
        ? ((got.value as { subagents?: { id: string }[] }).subagents ?? [])
        : [];
    for (const s of subs) {
      await idCases(c, ts, s.id, h.id);
    }
  }
  const nope = await rpc(ts, "sessions.analyze", { id: "nope" });
  await compareExit(
    c,
    "sessions.analyze nope",
    nope,
    "/api/sessions/nope/analysis"
  );
  await pages(c, "pages nope", nope, "/api/sessions/nope", "");
};

/** The paged routes of one analysis; differences are reported. */
const pages = async (
  c: Counts,
  label: string,
  exit: RpcExit,
  path: string,
  q: string
) => {
  const port = rustPort().port;
  for (const line of await pageCases({
    c,
    exit,
    label,
    path,
    port,
    query: q,
  })) {
    report(line);
  }
};

/** get and analyze of one id, with the redact and override variants. */
const idCases = async (
  c: Counts,
  ts: number,
  id: string,
  parent: string | undefined
) => {
  const base = parent === undefined ? {} : { parent };
  const variants: [string, Record<string, unknown>][] = [
    ["", {}],
    [" redact=false", { redact: false }],
  ];
  for (const [name, extra] of variants) {
    await compareExit(
      c,
      `sessions.get ${id}${name}`,
      await rpc(ts, "sessions.get", { id, ...base, ...extra }),
      `/api/sessions/${seg(id)}${query({ ...base, ...extra })}`
    );
  }
  const analyses: [string, Record<string, unknown>][] = [
    ...variants,
    [" window=100000", { window: 100_000 }],
    [" dumbZone=0.5", { dumbZone: 0.5 }],
  ];
  for (const [name, extra] of analyses) {
    const exit = await rpc(ts, "sessions.analyze", { id, ...base, ...extra });
    const q = query({ ...base, ...extra });
    await compareExit(
      c,
      `sessions.analyze ${id}${name}`,
      exit,
      `/api/sessions/${seg(id)}/analysis${q}`
    );
    await pages(c, `pages ${id}${name}`, exit, `/api/sessions/${seg(id)}`, q);
  }
};

/** Static routes with no input. */
export const infoCases = async (pair: Pair, c: Counts) => {
  current = pair;
  const ts = pair.ts.port;
  await compareExit(
    c,
    "server.info",
    await rpc(ts, "server.info"),
    "/api/server"
  );
  await compareExit(
    c,
    "capabilities.list",
    await rpc(ts, "capabilities.list"),
    "/api/capabilities"
  );
};

const VALID = '{"roots":{"codex":[{"path":"~/c","label":"c"}],"claude":[]}}\n';
const SALVAGE =
  '{"roots":{"pi":[{"path":"p"},{"path":1}],"claude":[{"label":null,"path":"x"}]}}';
const GARBAGE = "{not json";
const FIXED = new Date("2026-03-04T05:06:07.891Z");

/** Settings reads and the write sequence; each server has its own dir. */
export const settingsCases = async (pair: Pair, c: Counts) => {
  current = pair;
  const norm = (v: unknown) =>
    replaceText(replaceText(v, pair.tsDir, "<DIR>"), pair.rustDir, "<DIR>");
  const file = (dir: string) => join(dir, "settings.json");
  const both = (text: string | undefined) => {
    for (const dir of [pair.tsDir, pair.rustDir]) {
      if (text === undefined) {
        rmSync(file(dir), { force: true });
      } else {
        writeFileSync(file(dir), text);
        utimesSync(file(dir), FIXED, FIXED);
      }
    }
  };
  const get = async (label: string) =>
    compareExit(
      c,
      label,
      await rpc(pair.ts.port, "settings.get"),
      "/api/settings",
      {},
      norm
    );
  both(undefined);
  await get("settings.get missing");
  for (const [name, text] of [
    ["valid", VALID],
    ["salvage", SALVAGE],
    ["garbage", GARBAGE],
  ] as const) {
    both(text);
    await get(`settings.get ${name}`);
  }
  const put = async (label: string, payload: Record<string, unknown>) => {
    const masked = (v: unknown) => {
      const n = norm(v) as { mtimeMs?: number };
      return n &&
        typeof n === "object" &&
        typeof n.mtimeMs === "number" &&
        n.mtimeMs > 0
        ? { ...n, mtimeMs: "<MTIME>" }
        : n;
    };
    await compareExit(
      c,
      label,
      await rpc(pair.ts.port, "settings.update", payload),
      "/api/settings",
      { method: "PUT", body: JSON.stringify(payload) },
      masked
    );
  };
  both(undefined);
  const settings = { roots: { claude: [{ path: "~/x", label: "w" }] } };
  await put("settings.update no CAS", { settings });
  await sameFiles(pair, "settings.update no CAS");
  await put("settings.update stale CAS", { settings, expectedMtime: 1 });
  both(undefined);
  await put("settings.update missing CAS", { settings, expectedMtime: 5 });
  await put("settings.update missing CAS 0", { settings, expectedMtime: 0 });
  await put("settings.update extra keys", {
    settings: {
      roots: { claude: [{ label: "w", path: "~/x", extra: 1 }], bogus: [] },
      zzz: 1,
    },
  });
  await sameFiles(pair, "settings.update extra keys");
  await put("settings.update empty", {});
};

const sameFiles = async (pair: Pair, label: string) => {
  const read = (dir: string) =>
    Bun.file(join(dir, "settings.json"))
      .text()
      .catch(() => "<none>");
  const [a, b] = await Promise.all([
    tsValue(`${label} ts settings file`, () => read(pair.tsDir)),
    read(pair.rustDir),
  ]);
  if (a !== b) {
    report(
      `serve ${label}: written files differ: ${JSON.stringify(a)} vs ${JSON.stringify(b)}`
    );
  }
};

/** Read-only pair: the write is refused with the same body. */
export const readOnlyCases = async (pair: Pair, c: Counts) => {
  current = pair;
  const payload = { settings: {} };
  await compareExit(
    c,
    "settings.update read-only",
    await rpc(pair.ts.port, "settings.update", payload),
    "/api/settings",
    { method: "PUT", body: JSON.stringify(payload) }
  );
  const r = await api(pair.rust.port, "/api/settings", {
    method: "PUT",
    body: JSON.stringify(payload),
  });
  if (r.status !== 403) {
    report(
      `serve settings.update read-only: rust status ${r.status} (want 403)`
    );
  }
};

const POLL_DEADLINE_MS = 5000;

/** Both watchers see one append to a shared Claude transcript. */
export const watchCases = async (pair: Pair, c: Counts, transcript: string) => {
  current = pair;
  c.cases += 1;
  const versions = async () => {
    const t = await rpc(pair.ts.port, "watch.poll");
    const r = await api(pair.rust.port, "/api/watch");
    return {
      ts:
        t.kind === "ok"
          ? (t.value as { memory: number; sessions: number })
          : undefined,
      rust: r.json as { memory: number; sessions: number },
    };
  };
  const start = await versions();
  sameJson("watch.poll start", start.ts, start.rust);
  appendFileSync(transcript, '{"type":"system","subtype":"x"}\n');
  const deadline = Date.now() + POLL_DEADLINE_MS;
  let last = start;
  while (Date.now() < deadline) {
    last = await versions();
    if ((last.ts?.sessions ?? 0) >= 1 && last.rust.sessions >= 1) {
      break;
    }
    await Bun.sleep(100);
  }
  if (!((last.ts?.sessions ?? 0) >= 1 && last.rust.sessions >= 1)) {
    report(
      `serve watch: sessions did not bump: ts ${JSON.stringify(last.ts)} rust ${JSON.stringify(last.rust)}`
    );
  }
  if (
    last.ts?.memory !== start.ts?.memory ||
    last.rust.memory !== start.rust.memory
  ) {
    report("serve watch: memory moved on a transcript append");
  }
};

/** Real corpus: the listing, then get and analysis for picked rows. */
export const realCases = async (
  pair: Pair,
  c: Counts,
  pick: (rows: readonly Header[]) => readonly Header[],
  stampOf: (path: string) => string
) => {
  current = pair;
  const before = await rpc(pair.ts.port, "sessions.listing", {});
  const rust = await api(pair.rust.port, "/api/sessions");
  if (before.kind !== "ok") {
    report("serve real: TS listing failed");
    return;
  }
  const tsRows = (before.value as { headers: Header[] }).headers;
  const rustRows = (rust.json as { headers: Header[] }).headers;
  const stamps = new Map(tsRows.map((r) => [r.path, stampOf(r.path)]));
  const rustBy = new Map(rustRows.map((r) => [r.path, r]));
  for (const row of tsRows) {
    const other = rustBy.get(row.path);
    if (stamps.get(row.path) !== stampOf(row.path)) {
      c.live += 1;
      continue;
    }
    c.cases += 1;
    if (other === undefined) {
      report(`serve real listing: ${row.path} missing in rust`);
      continue;
    }
    sameJson(`real listing ${row.path}`, row, other);
  }
  if (rustRows.length !== tsRows.length) {
    report(
      `serve real listing: ${tsRows.length} ts rows vs ${rustRows.length} rust`
    );
  }
  for (const row of pick(tsRows)) {
    const stamp = stampOf(row.path);
    const id = row.id;
    const tsGet = await rpc(pair.ts.port, "sessions.get", { id });
    const tsAn = await rpc(pair.ts.port, "sessions.analyze", { id });
    if (stampOf(row.path) !== stamp) {
      c.live += 1;
      continue;
    }
    await compareExit(c, `real get ${id}`, tsGet, `/api/sessions/${seg(id)}`);
    await compareExit(
      c,
      `real analyze ${id}`,
      tsAn,
      `/api/sessions/${seg(id)}/analysis`
    );
    const port = pair.rust.port;
    const path = `/api/sessions/${seg(id)}`;
    const found = await pageCases({
      c,
      exit: tsAn,
      label: `real pages ${id}`,
      path,
      port,
      query: "",
    });
    if (stampOf(row.path) === stamp) {
      found.forEach(report);
    } else {
      c.live += 1;
    }
  }
};
