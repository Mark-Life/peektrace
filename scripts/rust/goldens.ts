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
 * `/tmp/peektrace-goldens`). The path must not move between record and
 * check, since fixture text holds it. `--write` is a live parity run that
 * also stores each TS result; it refuses to write a stage with differences.
 * `--verify` records into a temp dir and lists the values that changed.
 * Replay fails for a stage recorded before the TS sources, fixtures or parity
 * scripts last changed: regenerate it with `--write`.
 */
import { createHash } from "node:crypto";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { cpus } from "node:os";
import { join, relative } from "node:path";
import { gunzipSync } from "node:zlib";
import { indexLines } from "./parity-oracle";

const REPO = join(import.meta.dir, "../..");
const GOLDENS = join(REPO, "tests/goldens");
const ROOT = process.env.PARITY_GOLDENS_ROOT ?? "/tmp/peektrace-goldens";
const STAGES = ["base", "search", "memory", "stats", "serve", "cli", "tui"];
const MAX_BYTES = 10 * 1024 * 1024;
const SHOWN = 20;
/** Tracked inputs whose change makes the goldens stale. */
const SOURCES = [
  "apps/cli/src",
  "apps/inspector",
  "packages",
  "bun.lock",
  "package.json",
  "scripts/rust",
];
const SOURCE_FILE =
  /^(apps\/cli\/src\/|apps\/inspector\/(?!dist\/)|packages\/[^/]+\/(src|package\.json)|packages\/core\/test\/fixtures\/|bun\.lock$|package\.json$|scripts\/rust\/parity[^/]*(?<!\.test)\.ts$)/;

const argv = process.argv.slice(2);
const has = (f: string) => argv.includes(f);
const value = (f: string) => {
  const at = argv.indexOf(f);
  return at < 0 ? undefined : argv[at + 1];
};

interface Manifest {
  readonly bun: string;
  readonly platform: string;
  readonly root: string;
  readonly schema: 1;
  readonly stages: Record<string, StageEntry>;
}

interface StageEntry {
  readonly gzBytes: number;
  /** `tsSourceHash` when this stage was recorded. */
  readonly tsSourceHash: string;
  readonly values: number;
}

/** sha256 over every tracked input file, path and bytes. */
export const tsSourceHash = (repo = REPO) => {
  const listed = Bun.spawnSync(["git", "ls-files", "-z", "--", ...SOURCES], {
    cwd: repo,
  });
  const files = listed.stdout
    .toString()
    .split("\0")
    .filter((f) => SOURCE_FILE.test(f) && existsSync(join(repo, f)))
    .sort();
  const h = createHash("sha256");
  for (const f of files) {
    h.update(`${f}\0`);
    h.update(readFileSync(join(repo, f)));
    h.update("\0");
  }
  return h.digest("hex");
};

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
  readonly stage: string;
}

const runStage = async (
  stage: string,
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

const goldenText = (file: string) =>
  existsSync(file) ? gunzipSync(readFileSync(file)).toString("utf8") : "";

const writeStage = (
  stage: string,
  recorded: string,
  hash: string
): StageEntry => {
  mkdirSync(GOLDENS, { recursive: true });
  const dest = join(GOLDENS, `${stage}.jsonl.gz`);
  copyFileSync(recorded, dest);
  const lines = goldenText(dest).split("\n").filter(Boolean);
  writeFileSync(
    join(GOLDENS, `${stage}.index`),
    `${indexLines(lines).join("\n")}\n`
  );
  return {
    values: lines.length,
    gzBytes: statSync(dest).size,
    tsSourceHash: hash,
  };
};

const readManifest = (): Manifest | undefined => {
  const path = join(GOLDENS, "manifest.json");
  return existsSync(path)
    ? (JSON.parse(readFileSync(path, "utf8")) as Manifest)
    : undefined;
};

/** Keys whose recorded value differs between two golden files. */
const churn = (a: string, b: string) => {
  const map = (text: string) =>
    new Map(
      text
        .split("\n")
        .filter(Boolean)
        .map((l) => {
          const [k, n] = JSON.parse(l) as [string, number];
          return [`${k}#${n}`, l] as const;
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

/** Flags, checked: unknown stages, a missing binary, the wrong Bun to
 * record with, stale goldens to replay. */
const options = () => {
  const write = has("--write");
  const verify = has("--verify");
  const bin = join(REPO, value("--bin") ?? "target/parity/peektrace");
  const picked = value("--stage")?.split(",") ?? STAGES;
  const unknown = picked.filter((s) => !STAGES.includes(s));
  if (unknown.length > 0) {
    fail(`unknown stage ${unknown.join(", ")}: ${STAGES.join(", ")}`);
  }
  if (!existsSync(bin)) {
    fail(`missing ${bin}: cargo build --profile parity --bins --examples`);
  }
  const recording = write || verify;
  if (recording && Bun.version !== pinnedBun() && !has("--any-bun")) {
    fail(
      `goldens are recorded with Bun ${pinnedBun()} (package.json), this is ${Bun.version}: put that Bun first on PATH, or pass --any-bun`
    );
  }
  const hash = tsSourceHash();
  const manifest = readManifest();
  const stale = picked.filter(
    (s) => manifest?.stages[s]?.tsSourceHash !== hash
  );
  if (!recording && stale.length > 0) {
    fail(
      `goldens are stale (${stale.join(", ")}): the TS sources, fixtures or parity scripts changed since they were recorded. Run: bun scripts/rust/goldens.ts --write`,
      1
    );
  }
  const jobs = Number(value("--jobs")) || picked.length;
  return { bin, hash, jobs, manifest, picked, recording, verify, write };
};

const recordedFile = (s: string) => join(ROOT, "recorded", `${s}.jsonl.gz`);
const goldenFile = (s: string) => join(GOLDENS, `${s}.jsonl.gz`);

/** `--verify`: keys whose fresh recording differs from the checked-in one. */
const verifyReport = (picked: readonly string[], failed: number) => {
  let changed = 0;
  for (const s of picked.filter((p) => existsSync(recordedFile(p)))) {
    const keys = churn(goldenText(goldenFile(s)), goldenText(recordedFile(s)));
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

/** `--write`: copy each passing stage in, then the index and manifest. */
const writeGoldens = (
  o: ReturnType<typeof options>,
  outcomes: readonly Outcome[]
) => {
  const allowed = has("--allow-diffs");
  const ok = outcomes.filter((x) => x.code === 0 || allowed);
  const stages = { ...o.manifest?.stages };
  for (const x of ok) {
    stages[x.stage] = writeStage(x.stage, recordedFile(x.stage), o.hash);
  }
  const total = Object.values(stages).reduce((n, s) => n + s.gzBytes, 0);
  const next: Manifest = {
    schema: 1,
    bun: Bun.version,
    platform: `${process.platform}-${process.arch}`,
    root: ROOT,
    stages,
  };
  writeFileSync(
    join(GOLDENS, "manifest.json"),
    `${JSON.stringify(next, null, 2)}\n`
  );
  console.log(
    `goldens: wrote ${ok.length} stage(s), ${(total / 1024).toFixed(0)} KiB in ${relative(REPO, GOLDENS)}`
  );
  if (total > MAX_BYTES) {
    console.error(`goldens: over the ${MAX_BYTES / 1024 / 1024} MiB budget`);
    return false;
  }
  return true;
};

const main = async () => {
  const o = options();
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
  const within = o.write ? writeGoldens(o, outcomes) : true;
  console.log(
    failed === 0 ? "goldens: OK" : `goldens: ${failed} stage(s) failed`
  );
  process.exit(failed === 0 && within ? 0 : 1);
};

if (import.meta.main) {
  await main();
}
