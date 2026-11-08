/** The command tree. Kept free of boot side effects so tests can import it. */
import { Command, Options } from "@effect/cli";
import { Console, Effect, Option } from "effect";
import type { GlobalsAccessor } from "./client";
import { makeDoctor } from "./commands/doctor";
import { makeMemoryLs, makeMemoryRm, makeMemoryShow } from "./commands/memory";
import { makeServe } from "./commands/serve";
import { makeSessionsAnalyze, makeSessionsLs } from "./commands/sessions";
import { makeSessionsExport } from "./commands/sessions-export";
import { makeStats } from "./commands/stats";
import { makeStatusline } from "./commands/statusline";
import { makeTui } from "./commands/tui";
import { makeUpgrade } from "./commands/upgrade";
import { DESCRIPTION } from "./description";
import { ROOT_FLAGS } from "./help";
import { MEMORY_SUMMARY, SESSIONS_SUMMARY } from "./summaries";

const jsonOpt = Options.boolean("json").pipe(
  Options.withDescription(ROOT_FLAGS.json.description)
);
const readOnlyOpt = Options.boolean("read-only").pipe(
  Options.withDescription(ROOT_FLAGS.readOnly.description)
);
const prettyOpt = Options.boolean("pretty").pipe(
  Options.withDescription(ROOT_FLAGS.pretty.description)
);
const remoteOpt = Options.text("remote").pipe(
  Options.withDescription(ROOT_FLAGS.remote.description),
  Options.optional
);
const otelOpt = Options.boolean("otel").pipe(
  Options.withDescription(ROOT_FLAGS.otel.description)
);
const telemetryOpt = Options.boolean("telemetry", {
  negationNames: ["no-telemetry"],
}).pipe(
  Options.withDescription(ROOT_FLAGS.telemetry.description),
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
      `Peektrace — ${DESCRIPTION}\n` +
        "Try: peektrace serve | tui | sessions ls | memory ls | stats | --help"
    )
).pipe(Command.withDescription(DESCRIPTION));

/** Resolve the parent command's parsed global flags inside any subcommand,
 * OR-ing subcommand-local `--json`/`--read-only` onto the parent values. */
const globals: GlobalsAccessor = (local) =>
  Effect.map(peektrace, (config) => ({
    json: config.json || (local?.json ?? false),
    readOnly: config.readOnly || (local?.readOnly ?? false),
    remote: Option.getOrUndefined(config.remote),
    compact: !config.pretty,
  }));

export const sessions = Command.make("sessions").pipe(
  Command.withDescription(SESSIONS_SUMMARY),
  Command.withSubcommands([
    makeSessionsLs(globals),
    makeSessionsAnalyze(globals),
    makeSessionsExport(globals),
  ])
);

export const memory = Command.make("memory").pipe(
  Command.withDescription(MEMORY_SUMMARY),
  Command.withSubcommands([
    makeMemoryLs(globals),
    makeMemoryShow(globals),
    makeMemoryRm(globals),
  ])
);

export const stats = makeStats(globals);

export const command = peektrace.pipe(
  Command.withSubcommands([
    sessions,
    memory,
    stats,
    makeStatusline(globals),
    makeServe(globals),
    makeTui(globals),
    makeDoctor(),
    makeUpgrade(),
  ])
);
