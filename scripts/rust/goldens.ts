/**
 * Golden files for the fixture parity stages: the TS side recorded once,
 * replayed against the Rust binary in CI.
 *
 *   bun scripts/rust/goldens.ts --bin target/parity/peektrace   # check (CI)
 *   bun scripts/rust/goldens.ts --write --bin target/parity/peektrace
 *   bun scripts/rust/goldens.ts --verify --bin target/parity/peektrace
 *   ... --stage base,serve   # only these stages
 *   ... --jobs 3             # stages at once (default: all)
 *
 * Each stage runs `parity.ts --fixtures-only --<stage>-only` in its own Bun
 * process, under `$PARITY_GOLDENS_ROOT/<stage>` (default
 * `/dev/shm/peektrace-goldens` on Linux, else `/tmp/peektrace-goldens`). The
 * path must not move between record and check, since fixture text holds it. `--write` is a live parity run that
 * also stores each TS result; it refuses to write a stage with differences.
 * `--verify` records into a temp dir and lists the values that changed.
 * Replay fails for a stage recorded before its TS sources, fixtures or parity
 * scripts last changed (`goldens-inputs.ts` lists them per stage).
 *
 * Goldens live in `tests/goldens/<os>-<arch>/`: TS output depends on the
 * platform (directory order, ICU zone aliases, `/tmp` real path) and the
 * Rust CLI follows it. Only `linux-x64`, the CI platform, is checked in; the
 * `record goldens` CI job makes it (scripts/rust/README.md).
 */
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { cpus } from "node:os";
import { join, relative, resolve } from "node:path";
import { deflateSync } from "node:zlib";
import {
  type DirOrder,
  probeDirOrder,
  rootProblem,
  takeLock,
} from "./goldens-host";
import { STAGES, type Stage, stageHashes } from "./goldens-inputs";
import { GOLDENS_ROOT } from "./parity-oracle";

const REPO = join(import.meta.dir, "../..");
const PLATFORM =
  process.env.PARITY_GOLDENS_PLATFORM ?? `${process.platform}-${process.arch}`;
const GOLDENS = join(REPO, "tests/goldens", PLATFORM);
const ROOT = GOLDENS_ROOT;
/** Budget for the checked-in set, zlib-compressed as git stores it. */
const MAX_PACKED_BYTES = 2 * 1024 * 1024;
const SHOWN = 20;
const argv = process.argv.slice(2);
const has = (f: string) => argv.includes(f);
const value = (f: string) => {
  const at = argv.indexOf(f);
  return at < 0 ? undefined : argv[at + 1];
};

const recordedFile = (s: string) => join(ROOT, "recorded", `${s}.jsonl`);
const goldenFile = (s: string) => join(GOLDENS, `${s}.jsonl`);

/** `<stage>.json` beside each golden file. */
interface StageMeta {
  readonly bun: string;
  readonly bytes: number;
  /** Listing order of a fresh dir under `root` when recorded. */
  readonly dirOrder?: DirOrder;
  readonly platform: string;
  readonly root: string;
  /** `tsSourceHash` of this stage's inputs when recorded. */
  readonly tsSourceHash: string;
  readonly values: number;
}

const pinnedBun = () =>
  (
    JSON.parse(readFileSync(join(REPO, "package.json"), "utf8")) as {
      packageManager: string;
    }
  ).packageManager.replace("bun@", "");

interface Outcome {
  readonly code: number;
  readonly log: string;
  readonly ms: number;
  readonly stage: Stage;
}

const runStage = async (
  stage: Stage,
  mode: "record" | "replay",
  file: string,
  bin: string,
  jobs: number
): Promise<Outcome> => {
  const started = performance.now();
  const work = join(ROOT, stage);
  const proc = Bun.spawn(
    [
      "bun",
      join(REPO, "scripts/rust/parity.ts"),
      "--fixtures-only",
      `--${stage}-only`,
      "--bin",
      bin,
      "--work",
      work,
    ],
    {
      cwd: REPO,
      env: {
        ...process.env,
        PARITY_ORACLE: mode,
        PARITY_GOLDENS: file,
        PARITY_GOLDENS_ROOT: ROOT,
        PARITY_JOBS: String(jobs),
      },
      stdout: "pipe",
      stderr: "pipe",
    }
  );
  const [out, err, code] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);
  const log = `${out}${err}`;
  mkdirSync(ROOT, { recursive: true });
  writeFileSync(join(ROOT, `${stage}.log`), log);
  return { stage, code, log, ms: performance.now() - started };
};

/** Run `f` over `items`, at most `limit` at once. */
const bounded = async <T, R>(
  items: readonly T[],
  limit: number,
  f: (t: T) => Promise<R>
) => {
  const out: R[] = [];
  let next = 0;
  const worker = async () => {
    while (next < items.length) {
      const at = next;
      next += 1;
      out[at] = await f(items[at] as T);
    }
  };
  await Promise.all(Array.from({ length: limit }, worker));
  return out;
};

const summary = (o: Outcome) => {
  const secs = (o.ms / 1000).toFixed(1);
  if (o.code === 0) {
    return `goldens ${o.stage}: OK (${secs}s)`;
  }
  const lines = o.log.split("\n");
  const shown = lines.filter((l) => l.startsWith("  DIFF ")).slice(0, SHOWN);
  const tail = shown.length > 0 ? shown : lines.slice(-SHOWN);
  return [
    `goldens ${o.stage}: FAILED, exit ${o.code} (${secs}s); log ${join(ROOT, `${o.stage}.log`)}`,
    ...tail,
    lines.findLast((l) => l.startsWith("parity: ")) ?? "",
  ].join("\n");
};

const readGolden = (file: string) =>
  existsSync(file) ? readFileSync(file, "utf8") : "";

const writeStage = (
  stage: string,
  recorded: string,
  hash: string,
  dirOrder: DirOrder
) => {
  mkdirSync(GOLDENS, { recursive: true });
  const dest = goldenFile(stage);
  copyFileSync(recorded, dest);
  const lines = readGolden(dest).split("\n").filter(Boolean);
  const meta: StageMeta = {
    bun: Bun.version,
    bytes: statSync(dest).size,
    dirOrder,
    platform: PLATFORM,
    root: ROOT,
    tsSourceHash: hash,
    values: lines.length,
  };
  writeFileSync(
    join(GOLDENS, `${stage}.json`),
    `${JSON.stringify(meta, null, 2)}\n`
  );
};

const metaOf = (stage: string): StageMeta | undefined => {
  const path = join(GOLDENS, `${stage}.json`);
  return existsSync(path)
    ? (JSON.parse(readFileSync(path, "utf8")) as StageMeta)
    : undefined;
};

/** zlib-compressed bytes of this platform's golden files: about what one
 * full set costs in a git pack. */
const packedBytes = () =>
  existsSync(GOLDENS)
    ? readdirSync(GOLDENS).reduce(
        (n, name) => n + deflateSync(readFileSync(join(GOLDENS, name))).length,
        0
      )
    : 0;

/** Fields that move from run to run and that every compare masks: clocks,
 * file times TS servers write, counts of a truncated search. Matched in
 * JSON text at any escaping depth. */
const VOLATILE =
  /(\\*"(?:generatedAt|fromIso|toIso|mtime|mtimeMs|expectedMtime|scannedSessions|unreadable)\\*":\s*)(\\*"[^"\\]*\\*"|[^,}\\\s]+)/g;
/** Polls whose count depends on timing; replay reuses the last one. */
const TIMED = /^rpc \d+ watch\.poll /;

/** Keys whose recorded value differs between two golden files, beyond the
 * volatile fields. */
export const churn = (a: string, b: string) => {
  const map = (text: string) =>
    new Map(
      text
        .split("\n")
        .filter(Boolean)
        .flatMap((l) => {
          const [k, n] = JSON.parse(l) as [string, number];
          return TIMED.test(k)
            ? []
            : [
                [
                  `${k.replace(VOLATILE, "$1<v>")}#${n}`,
                  l.replace(VOLATILE, "$1<v>"),
                ] as const,
              ];
        })
    );
  const ma = map(a);
  const mb = map(b);
  return [...new Set([...ma.keys(), ...mb.keys()])]
    .filter((k) => ma.get(k) !== mb.get(k))
    .sort();
};

const fail = (message: string, code = 2): never => {
  console.error(message);
  process.exit(code);
};

const isStage = (s: string): s is Stage =>
  (STAGES as readonly string[]).includes(s);

/** `--bin`, relative to the repo unless absolute. */
export const binPath = (arg: string | undefined, repo = REPO) =>
  resolve(repo, arg ?? "target/parity/peektrace");

/** How to fix stale goldens: CI records the checked-in platform. */
const STALE_FIX =
  PLATFORM === "linux-x64"
    ? "Re-record them in CI: label the PR `record-goldens`, then copy the `goldens-*` artifacts into tests/goldens/linux-x64 (scripts/rust/README.md)."
    : "Run: bun scripts/rust/goldens.ts --write";

/** Flags, checked: unknown stages, a missing binary, the wrong Bun to
 * record with, stale goldens or another scratch root to replay. */
const options = () => {
  const write = has("--write");
  const verify = has("--verify");
  const bin = binPath(value("--bin"));
  const asked = value("--stage")?.split(",") ?? [...STAGES];
  const unknown = asked.filter((s) => !isStage(s));
  if (unknown.length > 0) {
    fail(`unknown stage ${unknown.join(", ")}: ${STAGES.join(", ")}`);
  }
  const picked = asked.filter(isStage);
  if (!existsSync(bin)) {
    fail(`missing ${bin}: cargo build --profile parity --bins --examples`);
  }
  const recording = write || verify;
  if (recording && Bun.version !== pinnedBun() && !has("--any-bun")) {
    fail(
      `goldens are recorded with Bun ${pinnedBun()} (package.json), this is ${Bun.version}: put that Bun first on PATH, or pass --any-bun`
    );
  }
  const hashes = stageHashes(REPO);
  const missing = picked.filter((s) => metaOf(s) === undefined);
  if (!recording && missing.length > 0) {
    fail(
      `no ${PLATFORM} goldens for ${missing.join(", ")} in ${relative(REPO, GOLDENS)}: only linux-x64 is checked in. Record your own with --write, or run parity.ts.`,
      1
    );
  }
  const stale = picked.filter((s) => metaOf(s)?.tsSourceHash !== hashes[s]);
  if (!recording && stale.length > 0) {
    fail(
      `goldens are stale (${stale.join(", ")}): their TS sources, fixtures or parity scripts changed since they were recorded. ${STALE_FIX}`,
      1
    );
  }
  const moved = picked.filter((s) => (metaOf(s)?.root ?? ROOT) !== ROOT);
  if (!recording && moved.length > 0) {
    fail(
      `goldens for ${moved.join(", ")} were recorded under ${metaOf(moved[0] as Stage)?.root}, this run uses ${ROOT}: fixture text holds that path, so set PARITY_GOLDENS_ROOT to it.`,
      1
    );
  }
  const jobs = Number(value("--jobs")) || picked.length;
  return { bin, hashes, jobs, picked, recording, verify, write };
};

/** Stages whose recorded dir order is missing or differs from `order`. */
export const orderProblems = (
  recorded: readonly (readonly [string, DirOrder | undefined])[],
  order: DirOrder
) => ({
  other: recorded
    .filter(([, r]) => r !== undefined && r !== order)
    .map(([s]) => s),
  unknown: recorded.filter(([, r]) => r === undefined).map(([s]) => s),
});

/** Owns ROOT for this run and checks this host lists dirs in the order the
 * picked goldens were recorded in. */
const claimRoot = (o: ReturnType<typeof options>) => {
  const problem = rootProblem(ROOT);
  if (problem !== undefined) {
    fail(`goldens root: ${problem}`);
  }
  const { holder, release } = takeLock(ROOT);
  if (release === undefined) {
    fail(
      `goldens root ${ROOT} is in use by another run (pid ${holder}); wait for it to finish`
    );
  } else {
    process.on("exit", release);
  }
  const order = probeDirOrder(ROOT);
  if (o.recording) {
    return order;
  }
  const { other, unknown } = orderProblems(
    o.picked.map((s) => [s, metaOf(s)?.dirOrder] as const),
    order
  );
  if (unknown.length > 0) {
    fail(
      `goldens for ${unknown.join(", ")} do not record the dir order they were made under, so replay cannot tell whether this host matches. ${STALE_FIX}`,
      1
    );
  }
  if (other.length > 0) {
    fail(
      `goldens for ${other.join(", ")} were recorded where a new dir lists its entries in ${metaOf(other[0] as Stage)?.dirOrder} order; under ${ROOT} this host lists them ${order}. tmpfs order varies with the kernel: replay on a matching host, or record your own set with PARITY_GOLDENS_PLATFORM=<name> --write.`,
      1
    );
  }
  return order;
};

/** `--verify`: keys whose fresh recording differs from the checked-in one. */
const verifyReport = (picked: readonly string[], failed: number) => {
  let changed = 0;
  for (const s of picked.filter((p) => existsSync(recordedFile(p)))) {
    const keys = churn(readGolden(goldenFile(s)), readGolden(recordedFile(s)));
    changed += keys.length;
    for (const k of keys.slice(0, SHOWN)) {
      console.log(`  CHANGED ${s} ${k}`);
    }
  }
  const ok = changed === 0 && failed === 0;
  console.log(
    ok
      ? "goldens: current"
      : `goldens: ${changed} changed value(s), ${failed} failed stage(s)`
  );
  return ok;
};

/** `--write`: copy each passing stage in, with its meta. */
const writeGoldens = (
  o: ReturnType<typeof options>,
  outcomes: readonly Outcome[],
  order: DirOrder
) => {
  const allowed = has("--allow-diffs");
  const ok = outcomes.filter((x) => x.code === 0 || allowed);
  for (const x of ok) {
    writeStage(x.stage, recordedFile(x.stage), o.hashes[x.stage], order);
  }
  const packed = packedBytes();
  console.log(
    `goldens: wrote ${ok.length} stage(s) to ${relative(REPO, GOLDENS)}; ${(packed / 1024).toFixed(0)} KiB compressed`
  );
  if (packed > MAX_PACKED_BYTES) {
    console.error(
      `goldens: over the ${MAX_PACKED_BYTES / 1024 / 1024} MiB compressed budget`
    );
    return false;
  }
  return true;
};

const main = async () => {
  const o = options();
  const order = claimRoot(o);
  const perStage = Math.max(2, Math.ceil(cpus().length / Math.min(o.jobs, 3)));
  rmSync(join(ROOT, "recorded"), { recursive: true, force: true });
  const outcomes = await bounded(o.picked, o.jobs, async (s) => {
    const x = await runStage(
      s,
      o.recording ? "record" : "replay",
      o.recording ? recordedFile(s) : goldenFile(s),
      o.bin,
      perStage
    );
    console.log(summary(x));
    return x;
  });
  const failed = outcomes.filter((x) => x.code !== 0).length;
  if (o.verify) {
    process.exit(verifyReport(o.picked, failed) ? 0 : 1);
  }
  const within = o.write ? writeGoldens(o, outcomes, order) : true;
  console.log(
    failed === 0 ? "goldens: OK" : `goldens: ${failed} stage(s) failed`
  );
  process.exit(failed === 0 && within ? 0 : 1);
};

if (import.meta.main) {
  await main();
}
