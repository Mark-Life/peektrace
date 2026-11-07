/** `peektrace statusline` — one-line context health for Claude Code's `statusLine`.
 *
 * Claude Code pipes a JSON blob to stdin on every refresh. We analyze the
 * transcript it names and print `ctx 42% Healthy`. Any failure (bad stdin, missing
 * session, timeout) prints the fallback `ctx --` and exits 0 so the bar never
 * shows an error. Under `--remote`, `transcript_path` only resolves when the
 * server runs on the same machine.
 */
import { Command, Options } from "@effect/cli";
import { Console, Duration, Effect, Schema } from "effect";
import { type GlobalsAccessor, withClient } from "../client";
import { finalFractionOf, verdict } from "../verdict";

export const FALLBACK = "ctx --";
const DEFAULT_TIMEOUT_MS = 1500;
const MAX_STDIN_BYTES = 1_000_000;
const MAX_PERCENT = 999;
const PERCENT = 100;

/** The subset of Claude Code's statusline payload we read; other fields are ignored. */
export const StatuslineInput = Schema.Struct({
  session_id: Schema.optional(Schema.String),
  transcript_path: Schema.optional(Schema.String),
  cwd: Schema.optional(Schema.String),
  context_window: Schema.optional(
    Schema.Struct({ context_window_size: Schema.optional(Schema.Number) })
  ),
});

const decodeInput = Schema.decodeUnknown(Schema.parseJson(StatuslineInput));

/** Format the one-line output from the current context fill. */
export const formatStatusline = (
  a: Parameters<typeof finalFractionOf>[0] & {
    readonly dumbZoneFraction: number;
  }
) => {
  const fill = finalFractionOf(a);
  const shown = Math.min(MAX_PERCENT, Math.round(fill * PERCENT));
  return `ctx ${shown}% ${verdict(fill, a.dumbZoneFraction)}`;
};

/** Read stdin up to `MAX_STDIN_BYTES`. Cancelling the fiber cancels the reader so a
 * held-open pipe cannot keep the process alive. A TTY yields an empty string. */
const readStdin = Effect.async<string>((resume) => {
  if (process.stdin.isTTY) {
    resume(Effect.succeed(""));
    return;
  }
  const reader = Bun.stdin.stream().getReader();
  const decoder = new TextDecoder();
  let text = "";
  let size = 0;
  const pump = (): void => {
    reader
      .read()
      .then(({ done, value }) => {
        if (value) {
          size += value.byteLength;
          text += decoder.decode(value, { stream: true });
        }
        if (done || size >= MAX_STDIN_BYTES) {
          reader.cancel().catch(() => undefined);
          resume(Effect.succeed(text));
          return;
        }
        pump();
      })
      .catch(() => resume(Effect.succeed(text)));
  };
  pump();
  return Effect.promise(() => reader.cancel().catch(() => undefined));
});

const timeoutOpt = Options.integer("timeout").pipe(
  Options.withDescription(
    `Give up and print "${FALLBACK}" after this many ms (default ${DEFAULT_TIMEOUT_MS})`
  ),
  Options.withDefault(DEFAULT_TIMEOUT_MS)
);

/** `statusline` — read Claude Code's statusline JSON from stdin, print one line. */
export const makeStatusline = (globals: GlobalsAccessor) =>
  Command.make("statusline", { timeout: timeoutOpt }, ({ timeout }) =>
    Effect.gen(function* () {
      const g = yield* globals();
      const input = yield* readStdin.pipe(Effect.flatMap(decodeInput));
      const id = input.transcript_path ?? input.session_id;
      if (id === undefined) {
        return FALLBACK;
      }
      const window = input.context_window?.context_window_size;
      const a = yield* withClient(g, (client) =>
        client.sessions.analyze({
          id,
          ...(window !== undefined && window > 0 ? { window } : {}),
        })
      );
      return formatStatusline(a);
    }).pipe(
      Effect.timeoutFail({
        duration: Duration.millis(Math.max(1, timeout)),
        onTimeout: () => "timeout",
      }),
      Effect.catchAllCause(() => Effect.succeed(FALLBACK)),
      Effect.flatMap(Console.log)
    )
  );
