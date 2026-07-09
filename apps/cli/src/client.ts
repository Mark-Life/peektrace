/** Execution-mode plumbing for the `peektrace` CLI.
 *
 * Every one-shot command runs against the same fully-typed RPC client surface,
 * regardless of transport. Two modes share one contract:
 * - in-process: drive the group directly through `makeInProcessClient` over an
 *   in-process `makeHandlersLayer` (no network) — the default.
 * - remote: a `createPeektraceClient(url)` HTTP client against a running
 *   `peektrace serve` (`--remote <url>`).
 *
 * Both yield the identical client shape, so commands are written once against
 * `PeektraceClient` and the in-process client (error channel `never`) is assignable
 * to the HTTP client type (error channel `RpcClientError`).
 */
import { type Command, Options } from "@effect/cli";
import {
  createPeektraceClient,
  makeHandlersLayer,
  makeInProcessClient,
} from "@workspace/rpc";
import { Effect } from "effect";

/**
 * Per-subcommand `--json` twin of the root flag. Declaring it locally means a
 * leading `--json` after the subcommand is parsed as a flag rather than swallowed
 * by an optional positional (e.g. `memory ls --json`). Merged with the parent
 * flag in `GlobalsAccessor`.
 */
export const localJsonOpt = Options.boolean("json").pipe(
  Options.withDescription("Emit raw JSON instead of rendered tables")
);
/** Per-subcommand `--read-only` twin of the root flag (see `localJsonOpt`). */
export const localReadOnlyOpt = Options.boolean("read-only").pipe(
  Options.withDescription("Refuse any mutating command (safe mode)")
);

/** The typed client surface every command is written against (HTTP variant). */
export type PeektraceClient = Effect.Effect.Success<
  ReturnType<typeof createPeektraceClient>
>;

/** Resolved global flags that pick the transport + safety posture. */
export interface Globals {
  /** Render token-lean tab-separated tables (the default; off under `--pretty`). */
  readonly compact: boolean;
  /** Emit raw JSON instead of rendered tables. */
  readonly json: boolean;
  /** Refuse any mutating command (compile-time-ish safe mode). */
  readonly readOnly: boolean;
  /** When set, hit a running `peektrace serve` over HTTP instead of in-process. */
  readonly remote: string | undefined;
}

/** Subcommand-local overrides OR-ed onto the parent flags (see `localJsonOpt`). */
export interface LocalGlobals {
  readonly json: boolean;
  readonly readOnly: boolean;
}

/**
 * Accessor that reads the parent `peektrace` command's parsed global flags,
 * merging any subcommand-local `--json`/`--read-only` (OR semantics). Its effect
 * requires the parent command context, which `withSubcommands` supplies.
 */
export type GlobalsAccessor = (
  local?: Partial<LocalGlobals>
) => Effect.Effect<Globals, never, Command.Command.Context<"peektrace">>;

/**
 * Run `use` with a live client for the chosen transport. The in-process path
 * provisions the real core layers once; the remote path opens an HTTP client.
 * Both are scoped — the client is torn down when the effect completes.
 */
export const withClient = <A, E>(
  globals: Globals,
  use: (client: PeektraceClient) => Effect.Effect<A, E>
) => {
  if (globals.remote !== undefined) {
    return createPeektraceClient(globals.remote).pipe(
      Effect.flatMap(use),
      Effect.scoped
    );
  }
  return makeInProcessClient().pipe(
    Effect.flatMap(use),
    Effect.scoped,
    Effect.provide(makeHandlersLayer())
  );
};
