/**
 * Record/replay seam for every TS-side result of the parity harness.
 *
 * - live (default): each TS call runs; `parity.ts` behaves as without this.
 * - record (`PARITY_ORACLE=record`): each TS call runs and its result is kept,
 *   then written to `PARITY_GOLDENS` (gzipped JSON lines) by `finishOracle`.
 * - replay (`PARITY_ORACLE=replay`): each TS call returns the kept result, so
 *   only the Rust side runs.
 *
 * A key names one TS call. Repeats of a key get `#n` in call order; past the
 * last kept repeat, replay returns the last one. Stored text has the stage
 * root, its real path and the repo replaced by `<WORK>`, `<RWORK>` and
 * `<REPO>`, plus whatever `bind` adds, so goldens hold no machine paths.
 */
import { createHash } from "node:crypto";
import {
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  realpathSync,
  statSync,
} from "node:fs";
import { homedir, tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { gunzipSync, gzipSync } from "node:zlib";

export type OracleMode = "live" | "record" | "replay";

const modeOf = (v: string | undefined): OracleMode =>
  v === "record" || v === "replay" ? v : "live";

export const ORACLE_MODE = modeOf(process.env.PARITY_ORACLE);
/** True when results are recorded or replayed, not only run. */
export const PINNED = ORACLE_MODE !== "live";
const FILE = process.env.PARITY_GOLDENS ?? "";
/** Root of the fixed scratch dirs goldens are recorded and replayed in.
 * Linux uses tmpfs: its directory order follows creation order, while ext4
 * order differs from machine to machine, and listings of symlinked
 * duplicates follow that order. */
export const GOLDENS_ROOT =
  process.env.PARITY_GOLDENS_ROOT ??
  (process.platform === "linux"
    ? "/dev/shm/peektrace-goldens"
    : "/tmp/peektrace-goldens");
/** Placeholder exit code of a replayed run that has no golden. */
export const NO_GOLDEN = -999;
const GZIP_OS_BYTE = 9;
const GZIP_OS_UNIX = 3;

interface Roots {
  readonly repo: string;
  readonly report: (line: string) => void;
  readonly work: string;
}

let roots: Roots | undefined;
const binds: [string, string][] = [];
/** Recorded values per key, encoded; in replay, loaded from the file. */
const store = new Map<string, unknown[]>();
const calls = new Map<string, number>();

const real = (path: string) => {
  try {
    return realpathSync(path);
  } catch {
    return path;
  }
};

/** Placeholder pairs, longest actual text first so nested paths win. */
const pairs = (): [string, string][] => {
  if (!roots) {
    return [];
  }
  const all: [string, string][] = [
    ["<WORK>", roots.work],
    ["<REPO>", roots.repo],
    ...binds,
  ];
  // Always present, so a value recorded where the real path differs (macOS
  // `/tmp`) decodes where it does not. Ties keep `<WORK>` first.
  all.push(["<RWORK>", real(roots.work)], ["<RREPO>", real(roots.repo)]);
  return all.sort((a, b) => b[1].length - a[1].length);
};

let shield: string[] | undefined;

/** Fixture text that spells a placeholder path (a fixture recorded in the
 * repo dir): kept as written, since the fixture says the same on any host. */
const shielded = () => {
  shield ??= fixturePaths(pairs().map(([, actual]) => actual));
  return shield;
};

const SHIELD = "\u0001";

export const encodeText = (s: string) => {
  const keep = shielded().filter((k) => s.includes(k));
  const hidden = keep.reduce(
    (acc, k, i) => acc.replaceAll(k, `${SHIELD}${i}${SHIELD}`),
    s
  );
  const done = pairs().reduce(
    (acc, [token, actual]) => acc.replaceAll(actual, token),
    hidden
  );
  return keep.reduce(
    (acc, k, i) => acc.replaceAll(`${SHIELD}${i}${SHIELD}`, k),
    done
  );
};

export const decodeText = (s: string) =>
  [...pairs()]
    .reverse()
    .reduce((acc, [token, actual]) => acc.replaceAll(token, actual), s);

const roundTrip = (v: unknown, f: (s: string) => string): unknown =>
  v === undefined ? undefined : JSON.parse(f(JSON.stringify(v)));

/** The stage root, the repo and the diff sink; `parity-lib` sets them. */
export const setOracleRoots = (r: Roots) => {
  roots = r;
  shield = undefined;
  if (ORACLE_MODE === "replay") {
    load();
  }
};

/** Store `actual` as `token` and read it back as the current value. Bind
 * before the first call whose key or result holds `actual`. */
export const bind = (token: string, actual: string) => {
  const at = binds.findIndex(([t]) => t === token);
  if (at >= 0) {
    binds.splice(at, 1);
  }
  binds.push([token, actual]);
};

const reportLine = (line: string) => {
  if (roots) {
    roots.report(line);
  } else {
    console.error(line);
  }
};

const load = () => {
  if (FILE === "" || !existsSync(FILE)) {
    reportLine(
      `no goldens at ${FILE || "(PARITY_GOLDENS unset)"}: run bun scripts/rust/goldens.ts --write`
    );
    return;
  }
  const text = gunzipSync(readFileSync(FILE)).toString("utf8");
  for (const line of text.split("\n")) {
    if (line === "") {
      continue;
    }
    const [key, n, value] = JSON.parse(line) as [string, number, unknown];
    const list = store.get(key) ?? [];
    list[n] = value;
    store.set(key, list);
  }
};

interface Thrown {
  readonly __thrown: string;
}

const isThrown = (v: unknown): v is Thrown =>
  typeof v === "object" && v !== null && "__thrown" in v;

const nextIndex = (key: string) => {
  const n = calls.get(key) ?? 0;
  calls.set(key, n + 1);
  return n;
};

const keep = (key: string, n: number, value: unknown) => {
  const list = store.get(key) ?? [];
  list[n] = roundTrip(value, encodeText);
  store.set(key, list);
};

const MISSING = Symbol("missing");

/** The kept value for call `n` of `key`, or the last one past the end. */
const recall = (key: string, n: number): unknown => {
  const list = store.get(key);
  if (list === undefined || list.length === 0) {
    reportLine(`no golden for ${key}: run bun scripts/rust/goldens.ts --write`);
    return MISSING;
  }
  return roundTrip(list[Math.min(n, list.length - 1)], decodeText);
};

const settle = <T>(v: unknown): T => {
  if (isThrown(v)) {
    throw new Error(v.__thrown);
  }
  return v as T;
};

export interface ValueOpts<T> {
  /** Record time only: make a volatile value stable. It must keep the shape,
   * so the stage's compare-time masks give the same result on it. */
  readonly canon?: (v: T) => T;
  /** Replay value of a missing key; without it a missing key throws. */
  readonly missing?: T;
}

/** A TS-side value: computed live, kept in record, recalled in replay. */
export const tsValue = async <T>(
  key: string,
  thunk: () => T | Promise<T>,
  opts: ValueOpts<T> = {}
): Promise<T> => {
  if (ORACLE_MODE === "live") {
    return await thunk();
  }
  const k = encodeText(key);
  const n = nextIndex(k);
  if (ORACLE_MODE === "replay") {
    return replayed(k, n, opts);
  }
  try {
    const v = await thunk();
    keep(k, n, opts.canon ? opts.canon(v) : v);
    return v;
  } catch (e) {
    keep(k, n, { __thrown: String((e as Error)?.message ?? e) });
    throw e;
  }
};

/** `tsValue` for synchronous thunks. */
export const tsValueSync = <T>(
  key: string,
  thunk: () => T,
  opts: ValueOpts<T> = {}
): T => {
  if (ORACLE_MODE === "live") {
    return thunk();
  }
  const k = encodeText(key);
  const n = nextIndex(k);
  if (ORACLE_MODE === "replay") {
    return replayed(k, n, opts);
  }
  try {
    const v = thunk();
    keep(k, n, opts.canon ? opts.canon(v) : v);
    return v;
  } catch (e) {
    keep(k, n, { __thrown: String((e as Error)?.message ?? e) });
    throw e;
  }
};

const replayed = <T>(k: string, n: number, opts: ValueOpts<T>): T => {
  const v = recall(k, n);
  if (v === MISSING) {
    if ("missing" in opts) {
      return opts.missing as T;
    }
    throw new Error(`no golden for ${k}`);
  }
  return settle<T>(v);
};

const sha1 = (s: string) => createHash("sha1").update(s).digest("hex");

/** A short, stable digest of the parts of a call that are not its argv. */
export const digest = (parts: unknown) =>
  sha1(encodeText(JSON.stringify(parts))).slice(0, 10);

/** The sorted golden lines: `[key, n, value]`. */
export const goldenLines = (m: ReadonlyMap<string, readonly unknown[]>) =>
  [...m.keys()]
    .sort()
    .flatMap((key) =>
      (m.get(key) ?? []).map((v, n) => JSON.stringify([key, n, v]))
    );

/** Gzip with a fixed header, so the same lines give the same bytes. */
export const gzipLines = (lines: readonly string[]) => {
  const out = gzipSync(Buffer.from(lines.map((l) => `${l}\n`).join("")), {
    level: 9,
  });
  out[GZIP_OS_BYTE] = GZIP_OS_UNIX;
  return out;
};

/** The `key#n<TAB>sha1` index that sits next to each golden file. */
export const indexLines = (lines: readonly string[]) =>
  lines.map((line) => {
    const [key, n, v] = JSON.parse(line) as [string, number, unknown];
    return `${key}#${n}\t${sha1(JSON.stringify(v)).slice(0, 12)}`;
  });

/** Text that must never reach a golden: the caller's home, temp dir, repo.
 * Paths written in the checked-in fixtures are fixture text, not leaks. */
export const leaks = (
  text: string,
  forbidden: readonly string[],
  allowed: readonly string[] = []
) => {
  const rest = allowed.reduce((acc, a) => acc.replaceAll(a, ""), text);
  return forbidden.filter((f) => f.length > 1 && rest.includes(f));
};

const forbidden = () =>
  [
    homedir(),
    real(homedir()),
    tmpdir(),
    real(tmpdir()),
    roots?.repo ?? "",
    real(roots?.repo ?? ""),
  ].filter((f) => f !== "" && f !== "/" && f !== "/tmp");

const PATH_TAIL = /^[^\s"'<>\\]*/;

/** Every forbidden path as the fixture files spell it, longest first. */
const fixturePaths = (bad: readonly string[]) => {
  const found = new Set<string>();
  const walk = (dir: string) => {
    for (const name of readdirSync(dir)) {
      const path = join(dir, name);
      if (statSync(path).isDirectory()) {
        walk(path);
        continue;
      }
      const text = readFileSync(path, "utf8");
      for (const f of bad) {
        for (const part of text.split(f).slice(1)) {
          found.add(f + (PATH_TAIL.exec(part)?.[0] ?? ""));
        }
      }
    }
  };
  const fixtures = join(roots?.repo ?? "", "packages/core/test/fixtures");
  if (PINNED && existsSync(fixtures)) {
    walk(fixtures);
  }
  return [...found].sort((a, b) => b.length - a.length);
};

const save = async () => {
  const lines = goldenLines(store);
  const bad = forbidden();
  const allowed = fixturePaths(bad);
  for (const line of lines) {
    const hits = leaks(line, bad, allowed);
    if (hits.length > 0) {
      reportLine(`golden leaks ${hits.join(", ")}: ${line.slice(0, 160)}`);
    }
  }
  mkdirSync(dirname(FILE), { recursive: true });
  await Bun.write(FILE, gzipLines(lines));
};

/** End of a run: record writes the goldens, replay reports keys no call
 * asked for (a case list that shrank). */
export const finishOracle = async () => {
  if (ORACLE_MODE === "record") {
    await save();
  } else if (ORACLE_MODE === "replay") {
    for (const key of store.keys()) {
      if (!calls.has(key)) {
        reportLine(`golden not used: ${key}`);
      }
    }
  }
};
