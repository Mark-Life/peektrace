#!/usr/bin/env bun
/** `peephole` — local, loopback-only inspector for Claude Code memories & sessions.
 *
 * One binary, two execution modes per command: in-process (default; provisions the
 * core layers directly) and `--remote <url>` (HTTP client against a running
 * `peephole serve`). Global flags `--json` and `--read-only` apply to every
 * subcommand and are read from this parent command's parsed config.
 */
import { Command, Options } from "@effect/cli";
import { BunContext, BunRuntime } from "@effect/platform-bun";
import { TelemetryStoreLive, TelemetryTracerLive } from "@workspace/telemetry";
import { Console, Effect, Layer, Option } from "effect";
import type { GlobalsAccessor } from "./client";
import { makeDoctor } from "./commands/doctor";
import { makeMemoryLs, makeMemoryRm, makeMemoryShow } from "./commands/memory";
import { makeServe } from "./commands/serve";
import { makeSessionsAnalyze, makeSessionsLs } from "./commands/sessions";
import { otelEnabled, tracingLayer } from "./tracing";

/** The build version reported by the CLI and stamped on every wide event. */
export const APP_VERSION = "0.0.1";

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
  Options.withDescription("Target a running `peephole serve` over HTTP"),
  Options.optional
);
const otelOpt = Options.boolean("otel").pipe(
  Options.withDescription(
    "Log Effect spans to stderr (also enabled by PEEPHOLE_OTEL)"
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
const peephole = Command.make(
  "peephole",
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
      "Peephole — local, loopback-only inspector for Claude Code.\n" +
        "Try: peephole serve | sessions ls | memory ls"
    )
);

/** Resolve the parent command's parsed global flags inside any subcommand. */
const globals: GlobalsAccessor = () =>
  Effect.map(peephole, (config) => ({
    json: config.json,
    readOnly: config.readOnly,
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

const command = peephole.pipe(
  Command.withSubcommands([sessions, memory, makeServe(), makeDoctor()])
);

const cli = Command.run(command, {
  name: "Peephole",
  version: APP_VERSION,
});

// One tracer slot is chosen at boot from raw argv (flags are also parsed by
// @effect/cli, but the tracer layer must be picked before dispatch). Telemetry
// is ON by default and persists one wide event per invocation to local SQLite;
// `--no-telemetry` (or `PEEPHOLE_NO_TELEMETRY`) opts out. `--otel` becomes the
// stderr echo. When telemetry is off, `--otel` still drives the console tracer.
const telemetryOn =
  !process.argv.includes("--no-telemetry") &&
  process.env.PEEPHOLE_NO_TELEMETRY == null;
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

cli(process.argv).pipe(
  Effect.withSpan("cli", {
    attributes: {
      "peephole.root": true,
      "peephole.kind": "cli",
      argv: process.argv.slice(2).join(" "),
    },
  }),
  Effect.provide(BunContext.layer),
  Effect.provide(tracing),
  BunRuntime.runMain
);
