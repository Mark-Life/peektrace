/** Feature-case plumbing for the serve stage: one TS `/rpc` call next to
 * the Rust `/api` route that replaces it, compared after a normaliser.
 *
 * - TS success: Rust must answer 200 with the same value.
 * - TS typed failure: Rust must answer the same error with the status its
 *   tag maps to (`STATUS`; a read-only refusal is 403).
 * - TS defect (`Die`, a bad payload): Rust must answer 400.
 */
import { report } from "./parity-lib";
import type { Counts } from "./parity-serve-cases";
import {
  type ApiAnswer,
  api,
  jsonDiff,
  type Pair,
  type RpcExit,
  rpc,
} from "./parity-serve-lib";

const OK = 200;
const BAD_REQUEST = 400;
const FORBIDDEN = 403;

const STATUS: Record<string, number> = {
  BadRequestError: BAD_REQUEST,
  CapabilityUnsupportedError: FORBIDDEN,
  FileChangedError: 409,
  MemoryNotFoundError: 404,
  MemoryValidationError: 422,
  PathOutsideRootError: FORBIDDEN,
  SessionNotFoundError: 404,
  StatsRowNotFoundError: 404,
  StatsScanError: 500,
};

export const READ_ONLY_REASON = "read-only mode is enabled; refusing to write";

/** The status a TS typed failure maps to on `/api`. */
const statusOf = (error: unknown) => {
  const e = error as { _tag?: string; reason?: string };
  if (e._tag === "MemoryValidationError" && e.reason === READ_ONLY_REASON) {
    return FORBIDDEN;
  }
  return STATUS[e._tag ?? ""];
};

export type Norm = (v: unknown) => unknown;
const same: Norm = (v) => v;

/** One TS call and its Rust route. */
export interface Call {
  readonly init?: RequestInit;
  readonly path: string;
  readonly payload?: unknown;
  readonly tag: string;
}

/** What differs between a TS exit and a Rust answer. */
export const exitDiff = (exit: RpcExit, r: ApiAnswer, norm: Norm = same) => {
  const brief = JSON.stringify(r.json)?.slice(0, 160);
  if (exit.kind === "ok") {
    return r.status === OK
      ? jsonDiff(norm(exit.value), norm(r.json))
      : [`ts ok, rust ${r.status} ${brief}`];
  }
  if (exit.kind === "die") {
    return r.status === BAD_REQUEST
      ? []
      : [`ts die, rust ${r.status} (want 400) ${brief}`];
  }
  const error = (r.json as { error?: unknown } | undefined)?.error;
  if (error === undefined) {
    return [`ts fail ${JSON.stringify(exit.error)}, rust ${r.status} ${brief}`];
  }
  const want = statusOf(exit.error);
  const status = r.status === want ? [] : [`status ${r.status}, want ${want}`];
  return [...status, ...jsonDiff(norm(exit.error), norm(error))];
};

/** An exit with `norm` applied to its value. */
export const normExit =
  (norm: Norm) =>
  (exit: RpcExit): RpcExit =>
    exit.kind === "ok" ? { kind: "ok", value: norm(exit.value) } : exit;

export interface CompareOpts {
  /** Record the TS answer through `norm`, so the golden keeps only what is
   * compared. `norm` must give the same result run twice. */
  readonly keepNormed?: boolean;
  readonly norm?: Norm;
  readonly retry?: boolean;
}

export interface Compared {
  readonly exit: RpcExit;
  readonly rust: ApiAnswer;
}

/** Run `call` on TS then on Rust, never at once, and report the diffs.
 * With `retry`, a mismatch runs both again once and counts as live. */
export const compareCall = async (
  c: Counts,
  pair: Pair,
  label: string,
  call: Call,
  opts: CompareOpts = {}
): Promise<Compared> => {
  c.cases += 1;
  c.features += 1;
  const canon =
    opts.keepNormed === true && opts.norm !== undefined
      ? normExit(opts.norm)
      : undefined;
  const once = async () => {
    const exit = await rpc(pair.ts.port, call.tag, call.payload ?? {}, canon);
    const rust = await api(pair.rust.port, call.path, call.init);
    return { exit, rust, diffs: exitDiff(exit, rust, opts.norm) };
  };
  let got = await once();
  if (got.diffs.length > 0 && opts.retry === true) {
    c.live += 1;
    got = await once();
  }
  for (const d of got.diffs) {
    report(`serve ${label}: ${d}`);
  }
  return { exit: got.exit, rust: got.rust };
};

/** A TS defect where Rust answers a typed error on purpose (a known
 * difference): TS must die and Rust must answer `status` with `tag`. */
export const compareKnown = async (
  c: Counts,
  pair: Pair,
  label: string,
  call: Call,
  want: { readonly status: number; readonly tag: string }
) => {
  c.cases += 1;
  c.features += 1;
  const exit = await rpc(pair.ts.port, call.tag, call.payload ?? {});
  const r = await api(pair.rust.port, call.path, call.init);
  const tag = (r.json as { error?: { _tag?: string } } | undefined)?.error
    ?._tag;
  if (exit.kind !== "die") {
    report(`serve ${label}: ts ${exit.kind}, want die (known difference)`);
  }
  if (r.status !== want.status || tag !== want.tag) {
    report(
      `serve ${label}: rust ${r.status} ${tag}, want ${want.status} ${want.tag}`
    );
  }
};

/** A query string; arrays repeat their key, `undefined` is left out. */
export const query = (
  q: Record<string, string | number | boolean | readonly string[] | undefined>
) => {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(q)) {
    if (Array.isArray(v)) {
      for (const x of v) {
        p.append(k, x);
      }
    } else if (v !== undefined) {
      p.append(k, String(v));
    }
  }
  const s = p.toString();
  return s === "" ? "" : `?${s}`;
};

export const seg = encodeURIComponent;

/** A JSON body with `method`. */
export const send = (method: string, body?: unknown): RequestInit => ({
  method,
  headers: { "content-type": "application/json" },
  ...(body === undefined ? {} : { body: JSON.stringify(body) }),
});

/** `v` without the listed top-level keys and `window.fromIso`/`toIso`:
 * the fields each run reads from its own clock. */
export const dropClock: Norm = (v) => {
  if (v === null || typeof v !== "object" || Array.isArray(v)) {
    return v;
  }
  const { generatedAt: _g, window, ...rest } = v as Record<string, unknown>;
  if (window === undefined) {
    return rest;
  }
  const { fromIso: _f, toIso: _t, ...w } = window as Record<string, unknown>;
  return { ...rest, window: w };
};

const WINDOW_KEYS = new Set(["schemaVersion", "by", "tz", "window"]);

/** Only how a windowed answer resolved its window, without the clock. Its
 * rows count fixture data with fixed dates against each run's clock, so a
 * recorded answer would drift from a later one as days pass. */
export const windowOnly: Norm = (v) => {
  const d = dropClock(v);
  if (d === null || typeof d !== "object" || !("window" in d)) {
    return d;
  }
  return Object.fromEntries(
    Object.entries(d).filter(([k]) => WINDOW_KEYS.has(k))
  );
};
