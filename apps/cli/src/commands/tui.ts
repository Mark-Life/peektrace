/** `peektrace tui` — the terminal UI, with the web app hosted alongside it.
 *
 * Boots the same loopback HTTP surface as `serve` (RPC + static inspector), then
 * renders an OpenTUI terminal UI over the *same* in-process backend. Both are
 * live at once: browse in the terminal or open the URL in a browser, one shared
 * `makeHandlersLayer` (one filesystem watcher, one read model) behind both.
 *
 * The HTTP surface is built by `serve-core` (identical guards/routing to
 * `serve`) but mounted quietly — no stdout logging that would corrupt the
 * alternate-screen renderer; the bound URL is surfaced in the TUI footer.
 */
import { Command, Options } from "@effect/cli";
import { HttpServer } from "@effect/platform";
import { BunHttpServer } from "@effect/platform-bun";
import { RpcSerialization } from "@effect/rpc";
import { makeHandlersLayer, makeInProcessClient } from "@workspace/rpc";
import { Effect, Layer } from "effect";
import { type GlobalsAccessor, localReadOnlyOpt } from "../client";
import { CliUserError } from "../errors";
import {
  buildServeApp,
  DEFAULT_HOST,
  DEFAULT_PORT,
  findFreePort,
  openBrowser,
} from "../serve-core";
import { runTui } from "../tui/index";
import { makeTuiBridge, type TuiEnv } from "../tui/runtime";

const MIN_PORT = 1;
const MAX_PORT = 65_535;

const portOpt = Options.integer("port").pipe(
  Options.withDescription(
    `Port for the sibling web server (default ${DEFAULT_PORT}, auto-picks if busy)`
  ),
  Options.withDefault(DEFAULT_PORT)
);
const hostOpt = Options.text("host").pipe(
  Options.withDescription(
    `Interface to bind the web server (default ${DEFAULT_HOST} loopback)`
  ),
  Options.withDefault(DEFAULT_HOST)
);
const openOpt = Options.boolean("open", {
  negationNames: ["no-open"],
}).pipe(
  Options.withDescription(
    "Also open the web app in a browser on start (use --no-open to skip)"
  ),
  Options.withDefault(true)
);

/** Scoped program: start the quiet web server, then run the terminal UI. */
const tuiProgram = (args: {
  readonly host: string;
  readonly port: number;
  readonly open: boolean;
}) =>
  Effect.gen(function* () {
    const server = yield* HttpServer.HttpServer;
    const { app } = yield* buildServeApp(args.host, args.port);
    yield* server.serve(app);

    const address = server.address;
    const boundPort = address._tag === "TcpAddress" ? address.port : args.port;
    const displayHost =
      args.host === "0.0.0.0" || args.host === "::" ? "127.0.0.1" : args.host;
    const serverUrl = `http://${displayHost}:${boundPort}`;

    // Same in-process client the CLI commands use, bound to the provided runtime
    // so React can drive it as promises — sharing this layer's read model with
    // the HTTP server mounted just above.
    const client = yield* makeInProcessClient();
    const runtime = yield* Effect.runtime<TuiEnv>();
    const bridge = makeTuiBridge(client, runtime, serverUrl);

    if (args.open) {
      yield* openBrowser(serverUrl);
    }

    // Blocks the fiber (keeping the scope — and the server — alive) until the
    // user quits the renderer.
    yield* Effect.tryPromise({
      try: () => runTui(bridge),
      catch: (error) =>
        new CliUserError({
          message: `TUI failed: ${error instanceof Error ? error.message : String(error)}`,
        }),
    });
  });

/** `tui` — launch the terminal UI with the web app served alongside it. */
export const makeTui = (globals: GlobalsAccessor) =>
  Command.make(
    "tui",
    { port: portOpt, host: hostOpt, open: openOpt, readOnly: localReadOnlyOpt },
    ({ port, host, open, readOnly }) =>
      Effect.gen(function* () {
        const g = yield* globals({ readOnly });
        if (!Number.isInteger(port) || port < MIN_PORT || port > MAX_PORT) {
          return yield* new CliUserError({
            message: `Invalid --port ${port}: must be an integer between ${MIN_PORT} and ${MAX_PORT}.`,
          });
        }
        const chosen = yield* findFreePort(port, host);
        const serverLayer = BunHttpServer.layer({
          port: chosen,
          hostname: host,
        });
        yield* tuiProgram({ host, port: chosen, open }).pipe(
          Effect.scoped,
          Effect.provide(
            Layer.mergeAll(
              serverLayer,
              RpcSerialization.layerNdjson,
              makeHandlersLayer({ rootSpans: true, readOnly: g.readOnly })
            )
          )
        );
      })
  );
