/** `peektrace tui` — the terminal UI, with the web app hosted alongside it.
 *
 * Boots the same HTTP surface (loopback by default) as `serve` (RPC + static inspector), then
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
import { allowWriteOpt, resolveServeAuth, tokenUrl } from "../serve-auth";
import {
  buildServeApp,
  DEFAULT_HOST,
  DEFAULT_PORT,
  findFreePort,
  openBrowser,
} from "../serve-core";
import { TUI_SUMMARY } from "../summaries";
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
    `Interface to bind the web server (default ${DEFAULT_HOST} loopback; any other host needs a token and is read-only unless --allow-write)`
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
  readonly token: string | undefined;
}) =>
  Effect.gen(function* () {
    const server = yield* HttpServer.HttpServer;
    const { app } = yield* buildServeApp(args.host, args.port, {
      token: args.token,
    });
    yield* server.serve(app);

    const address = server.address;
    const boundPort = address._tag === "TcpAddress" ? address.port : args.port;
    const displayHost =
      args.host === "0.0.0.0" || args.host === "::" ? "127.0.0.1" : args.host;
    const baseUrl = `http://${displayHost}:${boundPort}`;
    const serverUrl =
      args.token === undefined ? baseUrl : tokenUrl(baseUrl, args.token);

    // Same in-process client the CLI commands use, bound to the provided runtime
    // so React can drive it as promises — sharing this layer's read model with
    // the HTTP server mounted just above.
    const client = yield* makeInProcessClient();
    const runtime = yield* Effect.runtime<TuiEnv>();
    const bridge = makeTuiBridge(
      client,
      runtime,
      serverUrl,
      args.token !== undefined
    );

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
    {
      port: portOpt,
      host: hostOpt,
      open: openOpt,
      readOnly: localReadOnlyOpt,
      allowWrite: allowWriteOpt,
    },
    ({ port, host, open, readOnly, allowWrite }) =>
      Effect.gen(function* () {
        const g = yield* globals({ readOnly });
        if (!Number.isInteger(port) || port < MIN_PORT || port > MAX_PORT) {
          return yield* new CliUserError({
            message: `Invalid --port ${port}: must be an integer between ${MIN_PORT} and ${MAX_PORT}.`,
          });
        }
        const posture = yield* resolveServeAuth({
          host,
          readOnly: g.readOnly,
          allowWrite,
          env: process.env,
        });
        const chosen = yield* findFreePort(port, host);
        const serverLayer = BunHttpServer.layer({
          port: chosen,
          hostname: host,
        });
        yield* tuiProgram({
          host,
          port: chosen,
          open,
          token: posture.token,
        }).pipe(
          Effect.scoped,
          Effect.provide(
            Layer.mergeAll(
              serverLayer,
              RpcSerialization.layerNdjson,
              makeHandlersLayer({ rootSpans: true, readOnly: posture.readOnly })
            )
          )
        );
      })
  ).pipe(Command.withDescription(TUI_SUMMARY));
