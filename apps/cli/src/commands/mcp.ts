/** `peektrace mcp` — a stdio MCP server exposing read-only peektrace tools.
 *
 * stdout carries only JSON-RPC lines; logs go to stderr. One in-process client
 * lives for the whole process, so caches stay warm across calls, and
 * `rootSpans` records one wide telemetry event per RPC, as `serve` does.
 * Local only: `--remote` is refused.
 */
import { createInterface } from "node:readline";
import { Command } from "@effect/cli";
import { makeHandlersLayer, makeInProcessClient } from "@workspace/rpc";
import { Effect, Logger, Stream } from "effect";
import type { GlobalsAccessor } from "../client";
import { CliUserError } from "../errors";
import { handleLine } from "../mcp/protocol";
import { TOOLS } from "../mcp/tools";
import { MCP_SUMMARY } from "../summaries";

const stdinLines = () =>
  Stream.fromAsyncIterable(
    createInterface({
      input: process.stdin,
      crlfDelay: Number.POSITIVE_INFINITY,
    }),
    (cause) => new CliUserError({ message: `stdin failed: ${String(cause)}` })
  );

/** One reply per write, resolved once the bytes are handed to the pipe. */
const writeLine = (line: string) =>
  Effect.async<void>((resume) => {
    process.stdout.write(`${line}\n`, () => resume(Effect.void));
  });

/** Serve until stdin closes, then let in-flight calls finish. */
const serveStdio = Effect.gen(function* () {
  const client = yield* makeInProcessClient();
  const handle = handleLine({ tools: TOOLS, client });
  yield* stdinLines().pipe(
    Stream.mapEffect(handle, { concurrency: "unbounded", unordered: true }),
    Stream.filter((line) => line !== undefined),
    Stream.runForEach(writeLine)
  );
}).pipe(
  Effect.scoped,
  Effect.provide(makeHandlersLayer({ rootSpans: true, readOnly: true })),
  Effect.provide(
    Logger.replace(
      Logger.defaultLogger,
      Logger.withConsoleError(Logger.defaultLogger)
    )
  )
);

export const makeMcp = (globals: GlobalsAccessor) =>
  Command.make("mcp", {}, () =>
    Effect.gen(function* () {
      const g = yield* globals();
      if (g.remote !== undefined) {
        return yield* new CliUserError({
          message: "mcp runs locally only; remote is not supported yet",
        });
      }
      return yield* serveStdio;
    })
  ).pipe(Command.withDescription(MCP_SUMMARY));
