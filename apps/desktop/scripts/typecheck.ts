#!/usr/bin/env bun
/**
 * Type-check the desktop app, gating only on diagnostics in our own source.
 *
 * electrobun ships raw `.ts` (not `.d.ts`), so `tsc` descends into its internals
 * and reports type errors in electrobun's own source. Those are electrobun's to
 * fix and don't affect us — Electrobun apps compile with the Bun bundler, which
 * does not type-check, not with `tsc`. Our code is still checked under full
 * `strict`; any error in `src/`, `scripts/`, or the config fails the gate.
 */
import { spawnSync } from "node:child_process";

const TSC_ERROR = /error TS/;
const NEWLINE = /\r?\n/;

const result = spawnSync("bunx", ["tsc", "--noEmit", "--pretty", "false"], {
  encoding: "utf8",
});

const output = `${result.stdout ?? ""}${result.stderr ?? ""}`;
const ourErrors = output
  .split(NEWLINE)
  .filter((line) => TSC_ERROR.test(line) && !line.includes("node_modules/"));

if (ourErrors.length > 0) {
  console.error(ourErrors.join("\n"));
  console.error(`\n${ourErrors.length} type error(s) in app source.`);
  process.exit(1);
}

console.log("Type-check clean (electrobun-internal .ts diagnostics ignored).");
