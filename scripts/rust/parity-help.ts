/** Owned help pages and the bare-command banner, byte for byte. */
import {
  compareRun,
  fixtureEnv,
  run,
  rustBin,
  TS_CMD,
  WORK,
} from "./parity-lib";

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

export const runHelpStage = () => {
  const env = fixtureEnv(WORK);
  for (const args of CASES) {
    const ts = run([...TS_CMD, ...args], env);
    const rust = run([rustBin, ...args], env);
    compareRun(`help ${args.join(" ") || "(bare)"}`, ts, rust);
  }
  console.log(`suite help: ${CASES.length} cases`);
};
