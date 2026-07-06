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
import { Console, Effect, Option } from "effect";
import type { GlobalsAccessor } from "./client";
import { makeMemoryLs, makeMemoryRm, makeMemoryShow } from "./commands/memory";
import { makeServe } from "./commands/serve";
import { makeSessionsAnalyze, makeSessionsLs } from "./commands/sessions";
import { otelEnabled, tracingLayer } from "./tracing";

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

/** Root command — carries the global flags; prints a banner when run bare. */
const peephole = Command.make(
  "peephole",
  {
    json: jsonOpt,
    readOnly: readOnlyOpt,
    remote: remoteOpt,
    otel: otelOpt,
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
  Command.withSubcommands([sessions, memory, makeServe()])
);

const cli = Command.run(command, {
  name: "Peephole",
  version: "0.0.1",
});

// The tracer is installed at the runtime boundary so spans from every command
// (and the long-lived `serve` fibers) are exported. The `--otel` flag is read
// from argv here purely to pick the layer; it is also declared as a real option
// above so CLI parsing accepts it. Off by default → default no-op tracer.
const tracing = tracingLayer(otelEnabled(process.argv.includes("--otel")));

cli(process.argv).pipe(
  Effect.provide(BunContext.layer),
  Effect.provide(tracing),
  BunRuntime.runMain
);
