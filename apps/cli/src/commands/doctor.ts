/** `peephole doctor` — write a redacted local telemetry bundle for support.
 *
 * Reads recent wide events from the local {@link TelemetryStore}, recursively
 * redacts every string through the core `redactText` rules, and writes a JSON
 * bundle to `PEEPHOLE_DIR` (or `~/.peephole`). Nothing leaves the machine until
 * the user emails the file to 108@mark-life.com.
 */
import { homedir } from "node:os";
import { join } from "node:path";
import { Command, Options } from "@effect/cli";
import { FileSystem } from "@effect/platform";
import { redactText } from "@workspace/core";
import { TelemetryStore, TelemetryStoreLive } from "@workspace/telemetry";
import { Console, Effect, Option } from "effect";

const DEFAULT_LAST = 200;
const MIN_ENTROPY = 3.5;
const ENTROPY_RADIX = 2;
// Attribute keys whose values may carry a raw, non-provider-format secret (e.g.
// the `argv` command line, or a future `authToken` attribute). `redactText`'s
// generic high-entropy fallbacks only fire adjacent to a credential keyword, so
// a bare high-entropy token in these values would otherwise survive into the
// emailed bundle; here we sweep them unconditionally.
const CRED_KEY = /key|token|secret|password|passwd|pwd|auth|cred|argv/i;
const HIGH_ENTROPY_TOKEN = /[A-Za-z0-9+/_-]{20,}={0,2}/g;
const HAS_DIGIT = /[0-9]/;
const HAS_ALPHA = /[A-Za-z]/;

/** Shannon entropy (bits/char) over a token's own characters. */
const entropy = (s: string): number => {
  const freq = new Map<string, number>();
  for (const ch of s) {
    freq.set(ch, (freq.get(ch) ?? 0) + 1);
  }
  let h = 0;
  for (const n of freq.values()) {
    const p = n / s.length;
    h -= (p * Math.log(p)) / Math.log(ENTROPY_RADIX);
  }
  return h;
};

/**
 * Redact any bare high-entropy token in a value from a credential-ish attribute.
 * Applied on top of `redactText`, so provider-format secrets are already gone;
 * this closes the gap where a long mixed-alnum token sits with no cred keyword.
 */
const redactHighEntropyTokens = (s: string): string =>
  s.replace(HIGH_ENTROPY_TOKEN, (m) =>
    HAS_DIGIT.test(m) &&
    HAS_ALPHA.test(m) &&
    !m.startsWith("[REDACTED") &&
    entropy(m) >= MIN_ENTROPY
      ? "[REDACTED:high-entropy]"
      : m
  );

const lastOpt = Options.integer("last").pipe(
  Options.withDescription("Max events to include"),
  Options.withDefault(DEFAULT_LAST)
);
const interestingOpt = Options.boolean("interesting-only").pipe(
  Options.withDescription("Only errors/slow events"),
  Options.withDefault(false)
);
const outOpt = Options.text("out").pipe(
  Options.withDescription("Output path for the bundle"),
  Options.optional
);

/** Recursively redact every string within a JSON-like value (key-aware). */
const redactJson = (v: unknown, key?: string): unknown => {
  if (typeof v === "string") {
    const base = redactText(v);
    return key !== undefined && CRED_KEY.test(key)
      ? redactHighEntropyTokens(base)
      : base;
  }
  if (Array.isArray(v)) {
    return v.map((item) => redactJson(item, key));
  }
  if (v && typeof v === "object") {
    return Object.fromEntries(
      Object.entries(v).map(([k, val]) => [k, redactJson(val, k)])
    );
  }
  return v;
};

/** `doctor` — write a redacted telemetry bundle to disk for support. */
export const makeDoctor = () =>
  Command.make(
    "doctor",
    { last: lastOpt, interestingOnly: interestingOpt, out: outOpt },
    ({ last, interestingOnly, out }) =>
      Effect.gen(function* () {
        const store = yield* TelemetryStore;
        const fs = yield* FileSystem.FileSystem;
        const events = yield* store.recent({ limit: last, interestingOnly });
        const bundle = {
          schema: "peephole-report/v1",
          generatedAt: Date.now(),
          count: events.length,
          events: events.map((event) => redactJson(event)),
        };
        const dir = process.env.PEEPHOLE_DIR ?? join(homedir(), ".peephole");
        yield* fs.makeDirectory(dir, { recursive: true }).pipe(Effect.ignore);
        const path = Option.getOrElse(out, () =>
          join(dir, `peephole-report-${bundle.count}.json`)
        );
        yield* fs.writeFileString(path, JSON.stringify(bundle, null, 2));
        yield* Console.log(`Wrote ${bundle.count} events → ${path}`);
        yield* Console.log("Email this file to 108@mark-life.com");
      }).pipe(Effect.provide(TelemetryStoreLive))
  );
