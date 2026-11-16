/**
 * Pins our Cargo.lock to the versions Rune's own lock tested, for every crate
 * both locks hold once and only the Rune crates use. Crates the CLI, server or
 * TUI also build keep their version, so the CLI output never moves. Run after adding or bumping the Rune git deps:
 *
 *   bun scripts/rust/rune-lock-align.ts [--dry-run]
 *
 * Rune's lock comes from the cargo git checkout of the pinned rev, so run a
 * build first (`cargo fetch` is enough).
 */
import { readdirSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import nodePath from "node:path";

const ROOT = nodePath.join(import.meta.dir, "..", "..");
const PIN = /anomalyco\/rune", rev = "(?<rev>[0-9a-f]{40})"/u;
const NAME = /^name = "(?<name>[^"]+)"/mu;
const VERSION = /^version = "(?<version>[^"]+)"/mu;
const SOURCE = /^source = "(?<source>[^"]+)"/mu;

/** Length of the short rev cargo names a checkout dir after. */
const SHORT_REV = 7;
const dryRun = process.argv.includes("--dry-run");

const pinnedRev = () => {
  const toml = readFileSync(nodePath.join(ROOT, "Cargo.toml"), "utf-8");
  const rev = PIN.exec(toml)?.groups?.rev;

  if (rev === undefined || rev === "") {
    throw new Error("no anomalyco/rune rev in Cargo.toml");
  }

  return rev;
};

/** name -> versions from the registry, per lock. */
const registryVersions = (lock: string) => {
  const out = new Map<string, string[]>();

  for (const block of lock.split("[[package]]").slice(1)) {
    const name = NAME.exec(block)?.groups?.name;
    const version = VERSION.exec(block)?.groups?.version;
    const source = SOURCE.exec(block)?.groups?.source ?? "";

    if (
      name !== undefined &&
      name !== "" &&
      version !== undefined &&
      version !== "" &&
      source.startsWith("registry+")
    ) {
      out.set(name, [...(out.get(name) ?? []), version]);
    }
  }

  return out;
};

const runeLock = (rev: string) => {
  const base = nodePath.join(
    process.env.CARGO_HOME ?? nodePath.join(homedir(), ".cargo"),
    "git",
    "checkouts"
  );

  const short = rev.slice(0, SHORT_REV);

  for (const dir of readdirSync(base).filter((d) => d.startsWith("rune-"))) {
    const path = nodePath.join(base, dir, short, "Cargo.lock");

    try {
      return readFileSync(path, "utf-8");
    } catch {
      // not this checkout
    }
  }

  throw new Error(
    `Rune checkout ${short} not found under ${base}; run cargo fetch`
  );
};

/** Crate names the non-Rune members build, on every target. */
const nonRuneCrates = () => {
  const r = Bun.spawnSync(
    [
      "cargo",
      "tree",
      "--workspace",
      "--exclude",
      "peektrace-ui",
      "--exclude",
      "peektrace-app",
      "--target",
      "all",
      "-e",
      "normal,build,dev",
      "--prefix",
      "none",
      "-f",
      "{p}",
    ],
    { cwd: ROOT, stderr: "inherit" }
  );

  if (r.exitCode !== 0) {
    throw new Error("cargo tree failed");
  }

  return new Set(
    r.stdout
      .toString()
      .split("\n")
      .map((l) => l.split(" ")[0] ?? "")
  );
};

const rev = pinnedRev();

const shared = nonRuneCrates();

const ours = registryVersions(
  readFileSync(nodePath.join(ROOT, "Cargo.lock"), "utf-8")
);

const theirs = registryVersions(runeLock(rev));

const differing = [...ours].flatMap(([name, vs]) => {
  const [from, extraOurs] = vs;
  const [to, extraTheirs] = theirs.get(name) ?? [];

  return !shared.has(name) &&
    from !== undefined &&
    extraOurs === undefined &&
    to !== undefined &&
    extraTheirs === undefined &&
    from !== to
    ? [{ from, name, to }]
    : [];
});

const moves = differing.toSorted((a, b) => a.name.localeCompare(b.name));

let failed = 0;

for (const m of moves) {
  const args = ["update", "-p", `${m.name}@${m.from}`, "--precise", m.to];
  console.log(`${m.name} ${m.from} -> ${m.to}`);

  if (dryRun) {
    continue;
  }

  const r = Bun.spawnSync(["cargo", ...args], { cwd: ROOT, stderr: "pipe" });

  if (r.exitCode !== 0) {
    failed += 1;
    console.log(`  skipped: ${r.stderr.toString().trim().split("\n").at(-1)}`);
  }
}

console.log(
  `${moves.length} differ, ${moves.length - failed} pinned, ${failed} skipped`
);
