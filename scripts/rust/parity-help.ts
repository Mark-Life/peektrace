/** Owned help pages and the bare-command banner, byte for byte. The Rust
 * root page leaves out `doctor` until it exists, so its TS row is dropped
 * before the comparison. */
import {
  compareRun,
  fixtureEnv,
  type Run,
  run,
  rustBin,
  TS_CMD,
  WORK,
} from "./parity-lib";

const MISSING_ROWS = /^ {2}doctor {2,}.*\n/gm;

const CASES: readonly (readonly string[])[] = [
  [],
  ["--json"],
  ["--help"],
  ["-h"],
  ["--json", "--help"],
  ["--help", "sessions"],
  ["--remote", "http://x", "--help"],
  ["sessions", "--help"],
  ["sessions", "-h", "--json"],
  ["memory", "--help"],
  ["--telemetry", "--help"],
];

const dropMissing = (r: Run): Run => ({
  ...r,
  stdout: r.stdout.replace(MISSING_ROWS, ""),
});

export const runHelpStage = () => {
  const env = fixtureEnv(WORK);
  for (const args of CASES) {
    const ts = run([...TS_CMD, ...args], env);
    const rust = run([rustBin, ...args], env);
    compareRun(`help ${args.join(" ") || "(bare)"}`, dropMissing(ts), rust);
  }
  console.log(`suite help: ${CASES.length} cases`);
};
