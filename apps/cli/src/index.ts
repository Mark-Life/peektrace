#!/usr/bin/env bun
/** `peektrace` — local, loopback-only inspector for Claude Code, Codex, Pi and OpenCode sessions.
 * Context-budget forensics, stats and Claude memory.
 *
 * One binary, two execution modes per command: in-process (default; provisions the
 * core layers directly) and `--remote <url>` (HTTP client against a running
 * `peektrace serve`). The command tree lives in `cli-tree.ts`.
 */
import { CliConfig, Command, HelpDoc, ValidationError } from "@effect/cli";
import { BunContext, BunRuntime } from "@effect/platform-bun";
import { TelemetryStoreLive, TelemetryTracerLive } from "@workspace/telemetry";
import { Console, Effect, Layer } from "effect";
import {
  beforeSeparator,
  hasFreeText,
  helpGroup,
  isRootHelp,
  loggableArgv,
  skipsTelemetry,
} from "./argv";
import { command } from "./cli-tree";
import { formatCliError, UsageError } from "./errors";
import { GROUP_PATHS, groupHelpDoc, rootHelpDoc } from "./help";
import { otelEnabled, tracingLayer } from "./tracing";
import { APP_VERSION } from "./version";

export { APP_VERSION } from "./version";

const cli = Command.run(command, {
  name: "Peektrace",
  version: APP_VERSION,
});

const printRootHelp = Console.log(HelpDoc.toAnsiText(rootHelpDoc(APP_VERSION)));

// One tracer slot is chosen at boot from raw argv (flags are also parsed by
// @effect/cli, but the tracer layer must be picked before dispatch). Telemetry
// is ON by default and persists one wide event per invocation to local SQLite;
// `--no-telemetry` (or `PEEKTRACE_NO_TELEMETRY`) opts out; `statusline` never
// records (it runs on every status-bar refresh). `--otel` becomes the
// stderr echo. When telemetry is off, `--otel` still drives the console tracer.
const flagArgs = beforeSeparator(process.argv);
const telemetryOn =
  !(flagArgs.includes("--no-telemetry") || skipsTelemetry(process.argv)) &&
  process.env.PEEKTRACE_NO_TELEMETRY == null;
const echo = otelEnabled(flagArgs.includes("--otel"));

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
const argv = process.argv.map((arg, i) =>
  arg === "-v" && i < flagArgs.length ? "--version" : arg
);

/**
 * Error boundary for expected user-error paths: @effect/cli `ValidationError`
 * (already rendered by the cli itself — help/version exit 0, the rest exit 1) and
 * typed domain failures surfaced from RPC or the commands (rendered as one clean
 * line to stderr, exit 1). Unexpected defects are NOT caught here, so genuine bugs
 * still surface — only known user errors are made clean.
 */
const reportCliError = (error: unknown) => {
  if (ValidationError.isValidationError(error)) {
    if (!ValidationError.isHelpRequested(error)) {
      process.exitCode = 1;
    }
    return;
  }
  process.stderr.write(`${formatCliError(error)}\n`);
  process.exitCode = 1;
};

/** `UsageError` was already reported by `scrubUsage`. */
const handleCliError = (error: unknown) =>
  Effect.sync(() => {
    if (!(error instanceof UsageError)) {
      reportCliError(error);
    }
  });

/**
 * On free-text commands, report the failure here and hand telemetry a
 * text-free `UsageError`: validation and user errors quote the user's words.
 */
const scrubUsage = <A, E, R>(
  effect: Effect.Effect<A, E, R>
): Effect.Effect<A, E | UsageError, R> =>
  hasFreeText(argv)
    ? effect.pipe(
        Effect.tapError((error) => Effect.sync(() => reportCliError(error))),
        Effect.mapError(
          () => new UsageError({ message: "invalid command line" })
        )
      )
    : effect;

const selectProgram = () => {
  if (isRootHelp(argv)) {
    return printRootHelp;
  }
  const group = helpGroup(argv, GROUP_PATHS);
  return group === undefined
    ? cli(argv)
    : Console.log(HelpDoc.toAnsiText(groupHelpDoc(group, APP_VERSION)));
};

const program = selectProgram();

scrubUsage(program).pipe(
  Effect.withSpan("cli", {
    attributes: {
      "peektrace.root": true,
      "peektrace.kind": "cli",
      argv: loggableArgv(argv),
    },
  }),
  Effect.provide(BunContext.layer),
  Effect.provide(CliConfig.layer({ showBuiltIns: false })),
  Effect.provide(tracing),
  Effect.catchAll(handleCliError),
  BunRuntime.runMain
);
