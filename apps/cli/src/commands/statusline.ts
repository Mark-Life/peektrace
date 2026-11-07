/** `peektrace statusline` — one-line context health for Claude Code's `statusLine`.
 *
 * Claude Code pipes a JSON blob to stdin on every refresh. We analyze the
 * transcript it names and print `ctx 42% Healthy`. Any failure (bad stdin, missing
 * session, timeout) prints the fallback `ctx --` and exits 0 so the bar never
 * shows an error. Under `--remote`, `transcript_path` only resolves when the
 * server runs on the same machine.
 */
import { Command, Options } from "@effect/cli";
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import { Console, Duration, Effect, Option, Schema, Stream } from "effect";
import { type GlobalsAccessor, withClient } from "../client";
import { finalFractionOf, verdict } from "../verdict";

export const FALLBACK = "ctx --";
const DEFAULT_TIMEOUT_MS = 1500;
const MAX_STDIN_BYTES = 1_000_000;
const MAX_PERCENT = 999;
const PERCENT = 100;

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** The subset of Claude Code's statusline payload we read; other fields are ignored.
 * JSON `null` counts as absent so one null field cannot sink the whole decode.
 * `context_window` stays unknown here and is decoded on its own, so a wrong
 * type there loses only the window, not the transcript. */
const StatuslineInput = Schema.Struct({
  session_id: Schema.optionalWith(Schema.String, { nullable: true }),
  transcript_path: Schema.optionalWith(Schema.String, { nullable: true }),
  context_window: Schema.optional(Schema.Unknown),
});

const ContextWindow = Schema.Struct({
  context_window_size: Schema.Number.pipe(Schema.positive()),
});

const decodeInput = Schema.decodeUnknown(Schema.parseJson(StatuslineInput));
const decodeWindow = Schema.decodeUnknownOption(ContextWindow);

/** Format the one-line output from the current context fill. */
const formatStatusline = (
  a: Parameters<typeof finalFractionOf>[0] &
    Pick<AnalyzedSession, "dumbZoneFraction">
) => {
  const fill = finalFractionOf(a);
  const shown = Math.min(MAX_PERCENT, Math.round(fill * PERCENT));
  return `ctx ${shown}% ${verdict(fill, a.dumbZoneFraction)}`;
};

/** Read stdin up to `MAX_STDIN_BYTES`. Interrupting the fiber cancels the reader, so a
 * held-open pipe cannot keep the process alive. A TTY yields an empty string. */
const readStdin = process.stdin.isTTY
  ? Effect.succeed("")
  : Stream.fromReadableStream(
      () => Bun.stdin.stream(),
      (e) => e
    ).pipe(
      Stream.mapAccum(0, (size, chunk) => [
        size + chunk.byteLength,
        { chunk, size: size + chunk.byteLength },
      ]),
      Stream.takeUntil(({ size }) => size >= MAX_STDIN_BYTES),
      Stream.map(({ chunk }) => chunk),
      Stream.decodeText(),
      Stream.mkString,
      Effect.catchAll(() => Effect.succeed(""))
    );

const timeoutOpt = Options.integer("timeout").pipe(
  Options.withDescription(
    `Give up and print "${FALLBACK}" after this many ms (default ${DEFAULT_TIMEOUT_MS}). Covers stdin, lookup and remote calls, not parsing a large transcript`
  ),
  Options.withDefault(DEFAULT_TIMEOUT_MS)
);

/** `statusline` — read Claude Code's statusline JSON from stdin, print one line. */
export const makeStatusline = (globals: GlobalsAccessor) =>
  Command.make("statusline", { timeout: timeoutOpt }, ({ timeout }) =>
    Effect.gen(function* () {
      const g = yield* globals();
      const input = yield* readStdin.pipe(Effect.flatMap(decodeInput));
      const sessionId =
        input.session_id && UUID.test(input.session_id)
          ? input.session_id
          : undefined;
      const path = input.transcript_path || undefined;
      const window = Option.getOrUndefined(
        decodeWindow(input.context_window)
      )?.context_window_size;
      const analyze = (id: string) =>
        withClient(g, (client) =>
          client.sessions.analyze({
            id,
            ...(window === undefined ? {} : { window }),
          })
        );
      const first = path ?? sessionId;
      if (first === undefined) {
        return FALLBACK;
      }
      const a = yield* analyze(first).pipe(
        Effect.catchTag("SessionNotFoundError", (e) =>
          path !== undefined && sessionId !== undefined
            ? analyze(sessionId)
            : Effect.fail(e)
        )
      );
      return formatStatusline(a);
    }).pipe(
      Effect.timeout(Duration.millis(Math.max(1, timeout))),
      Effect.catchAllCause(() => Effect.succeed(FALLBACK)),
      Effect.flatMap(Console.log)
    )
  );
