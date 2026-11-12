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
import { join } from "node:path";

const ROOT = join(import.meta.dir, "..", "..");
const dryRun = process.argv.includes("--dry-run");

const pinnedRev = () => {
  const toml = readFileSync(join(ROOT, "Cargo.toml"), "utf8");
  const rev = toml.match(/anomalyco\/rune", rev = "([0-9a-f]{40})"/)?.[1];
  if (!rev) {
    throw new Error("no anomalyco/rune rev in Cargo.toml");
  }
  return rev;
};

/** name -> versions from the registry, per lock. */
const registryVersions = (lock: string) => {
  const out = new Map<string, string[]>();
  for (const block of lock.split("[[package]]").slice(1)) {
    const name = block.match(/^name = "([^"]+)"/m)?.[1];
    const version = block.match(/^version = "([^"]+)"/m)?.[1];
    const source = block.match(/^source = "([^"]+)"/m)?.[1] ?? "";
    if (name && version && source.startsWith("registry+")) {
      out.set(name, [...(out.get(name) ?? []), version]);
    }
  }
  return out;
};

const runeLock = (rev: string) => {
  const base = join(process.env.CARGO_HOME ?? join(homedir(), ".cargo"), "git", "checkouts");
  const short = rev.slice(0, 7);
  for (const dir of readdirSync(base).filter((d) => d.startsWith("rune-"))) {
    const path = join(base, dir, short, "Cargo.lock");
    try {
      return readFileSync(path, "utf8");
    } catch {
      // not this checkout
    }
  }
  throw new Error(`Rune checkout ${short} not found under ${base}; run cargo fetch`);
};

/** Crate names the non-Rune members build, on every target. */
const nonRuneCrates = () => {
  const r = Bun.spawnSync(
    ["cargo", "tree", "--workspace", "--exclude", "peektrace-ui", "--exclude", "peektrace-app",
      "--target", "all", "-e", "normal,build,dev", "--prefix", "none", "-f", "{p}"],
    { cwd: ROOT, stderr: "inherit" }
  );
  if (r.exitCode !== 0) {
    throw new Error("cargo tree failed");
  }
  return new Set(r.stdout.toString().split("\n").map((l) => l.split(" ")[0] ?? ""));
};

const rev = pinnedRev();
const shared = nonRuneCrates();
const ours = registryVersions(readFileSync(join(ROOT, "Cargo.lock"), "utf8"));
const theirs = registryVersions(runeLock(rev));
const moves = [...ours]
  .flatMap(([name, vs]) => {
    const t = theirs.get(name);
    return !shared.has(name) && vs.length === 1 && t?.length === 1 && vs[0] !== t[0]
      ? [{ name, from: vs[0] as string, to: t[0] as string }]
      : [];
  })
  .sort((a, b) => a.name.localeCompare(b.name));

let failed = 0;
for (const m of moves) {
  const args = ["update", "-p", `${m.name}@${m.from}`, "--precise", m.to];
  console.log(`${m.name} ${m.from} -> ${m.to}`);
  if (dryRun) {
    continue;
  }
  const r = Bun.spawnSync(["cargo", ...args], { cwd: ROOT, stderr: "pipe" });
  if (r.exitCode !== 0) {
    failed++;
    console.log(`  skipped: ${r.stderr.toString().trim().split("\n").at(-1)}`);
  }
}
console.log(`${moves.length} differ, ${moves.length - failed} pinned, ${failed} skipped`);
