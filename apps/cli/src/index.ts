#!/usr/bin/env bun
/** `peektrace` — local, loopback-only inspector for Claude Code memories & sessions.
 *
 * One binary, two execution modes per command: in-process (default; provisions the
 * core layers directly) and `--remote <url>` (HTTP client against a running
 * `peektrace serve`). Global flags `--json` and `--read-only` apply to every
 * subcommand and are read from this parent command's parsed config.
 */
import { Command, Options, ValidationError } from "@effect/cli";
import { BunContext, BunRuntime } from "@effect/platform-bun";
import { TelemetryStoreLive, TelemetryTracerLive } from "@workspace/telemetry";
import { Console, Effect, Layer, Option } from "effect";
import type { GlobalsAccessor } from "./client";
import { makeDoctor } from "./commands/doctor";
import { makeMemoryLs, makeMemoryRm, makeMemoryShow } from "./commands/memory";
import { makeServe } from "./commands/serve";
import { makeSessionsAnalyze, makeSessionsLs } from "./commands/sessions";
import { formatCliError } from "./errors";
import { otelEnabled, tracingLayer } from "./tracing";

// Injected at compile time by `src/build.ts` (Bun `define`); a bare undeclared
// global when running from source, so `typeof` guards against a ReferenceError.
declare const PEEKTRACE_VERSION: string | undefined;

/** The build version reported by the CLI and stamped on every wide event. */
export const APP_VERSION =
  (typeof PEEKTRACE_VERSION === "string" ? PEEKTRACE_VERSION : undefined) ??
  "0.0.0-dev";

const jsonOpt = Options.boolean("json").pipe(
  Options.withDescription("Emit raw JSON instead of rendered tables")
);
const readOnlyOpt = Options.boolean("read-only").pipe(
  Options.withDescription("Refuse any mutating command (safe mode)")
);
const prettyOpt = Options.boolean("pretty").pipe(
  Options.withDescription(
    "Render aligned tables instead of the default compact tab-separated output"
  )
);
const remoteOpt = Options.text("remote").pipe(
  Options.withDescription("Target a running `peektrace serve` over HTTP"),
  Options.optional
);
const otelOpt = Options.boolean("otel").pipe(
  Options.withDescription(
    "Log Effect spans to stderr (also enabled by PEEKTRACE_OTEL)"
  )
);
const telemetryOpt = Options.boolean("telemetry", {
  negationNames: ["no-telemetry"],
}).pipe(
  Options.withDescription(
    "Persist local wide-event telemetry (default on; --no-telemetry to disable)"
  ),
  Options.withDefault(true)
);

/** Root command — carries the global flags; prints a banner when run bare. */
const peektrace = Command.make(
  "peektrace",
  {
    json: jsonOpt,
    readOnly: readOnlyOpt,
    remote: remoteOpt,
    otel: otelOpt,
    telemetry: telemetryOpt,
    pretty: prettyOpt,
  },
  () =>
    Console.log(
      "Peektrace — local, loopback-only inspector for Claude Code.\n" +
        "Try: peektrace serve | sessions ls | memory ls"
    )
);

/** Resolve the parent command's parsed global flags inside any subcommand,
 * OR-ing subcommand-local `--json`/`--read-only` onto the parent values. */
const globals: GlobalsAccessor = (local) =>
  Effect.map(peektrace, (config) => ({
    json: config.json || (local?.json ?? false),
    readOnly: config.readOnly || (local?.readOnly ?? false),
    remote: Option.getOrUndefined(config.remote),
    compact: !config.pretty,
  }));

const sessions = Command.make("sessions").pipe(
  Command.withSubcommands([
    makeSessionsLs(globals),
    makeSessionsAnalyze(globals),
  ])
);

const memory = Command.make("memory").pipe(
  Command.withSubcommands([
    makeMemoryLs(globals),
    makeMemoryShow(globals),
    makeMemoryRm(globals),
  ])
);

const command = peektrace.pipe(
  Command.withSubcommands([sessions, memory, makeServe(globals), makeDoctor()])
);

const cli = Command.run(command, {
  name: "Peektrace",
  version: APP_VERSION,
});

// One tracer slot is chosen at boot from raw argv (flags are also parsed by
// @effect/cli, but the tracer layer must be picked before dispatch). Telemetry
// is ON by default and persists one wide event per invocation to local SQLite;
// `--no-telemetry` (or `PEEKTRACE_NO_TELEMETRY`) opts out. `--otel` becomes the
// stderr echo. When telemetry is off, `--otel` still drives the console tracer.
const telemetryOn =
  !process.argv.includes("--no-telemetry") &&
  process.env.PEEKTRACE_NO_TELEMETRY == null;
const echo = otelEnabled(process.argv.includes("--otel"));

/** Pick the single tracer layer: local telemetry when on, else the echo/no-op. */
const selectTracing = () => {
  if (telemetryOn) {
    return TelemetryTracerLive({ echo, appVersion: APP_VERSION }).pipe(
      Layer.provide(TelemetryStoreLive)
    );
  }
  return tracingLayer(echo);
};
const tracing = selectTracing();

// Convenience alias: `-v` behaves like `--version` (@effect/cli has no built-in
// short alias for it). No subcommand defines `-v`, so the rewrite is unambiguous.
const argv = process.argv.map((arg) => (arg === "-v" ? "--version" : arg));

/**
 * Error boundary for expected user-error paths: @effect/cli `ValidationError`
 * (already rendered by the cli itself — help/version exit 0, the rest exit 1) and
 * typed domain failures surfaced from RPC or the commands (rendered as one clean
 * line to stderr, exit 1). Unexpected defects are NOT caught here, so genuine bugs
 * still surface — only known user errors are made clean.
 */
const handleCliError = (error: unknown) =>
  Effect.sync(() => {
    if (ValidationError.isValidationError(error)) {
      if (!ValidationError.isHelpRequested(error)) {
        process.exitCode = 1;
      }
      return;
    }
    process.stderr.write(`${formatCliError(error)}\n`);
    process.exitCode = 1;
  });

cli(argv).pipe(
  Effect.withSpan("cli", {
    attributes: {
      "peektrace.root": true,
      "peektrace.kind": "cli",
      argv: argv.slice(2).join(" "),
    },
  }),
  Effect.provide(BunContext.layer),
  Effect.provide(tracing),
  Effect.catchAll(handleCliError),
  BunRuntime.runMain
);
