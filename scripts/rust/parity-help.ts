/** Owned help pages and the bare-command banner, byte for byte. The Rust
 * pages leave out `tui` and `doctor` until those commands exist, so their
 * TS rows are dropped before the comparison. */
import {
  compareRun,
  fixtureEnv,
  type Run,
  run,
  rustBin,
  TS_CMD,
  WORK,
} from "./parity-lib";

const MISSING_ROWS = /^ {2}(tui|doctor) {2,}.*\n/gm;
const BANNER_TUI = " tui |";

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
  stdout: r.stdout.replace(MISSING_ROWS, "").replace(BANNER_TUI, ""),
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
