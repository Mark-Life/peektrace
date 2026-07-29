/** `peektrace serve` — the headline command.
 *
 * Boots a loopback-only Bun HTTP server that:
 * - mounts the Effect-RPC handler (NDJSON over HTTP) at `POST /rpc`, backed by the
 *   real core layers provisioned once at boot via `makeHandlersLayer`;
 * - serves the built inspector static assets from `apps/inspector/dist` at `/`,
 *   falling back to `index.html` for client-side routes;
 * - binds `127.0.0.1:<port>` (default 4321, auto-picking the next free port if
 *   busy) and opens the browser unless `--no-open`. `--host 0.0.0.0` exposes it
 *   on the network (no auth — warned at startup); the default stays loopback.
 *
 * The HTTP surface (router, guards, static-asset resolution, port scan) lives in
 * `serve-core` and is shared with `peektrace tui`. Filesystem-driven live refresh
 * ships via the `WatchService` baked into `makeHandlersLayer`: a scoped watcher
 * fiber runs for the server's lifetime and advances the per-scope versions the
 * inspector polls through `watch.poll`.
 */
import { Command, Options } from "@effect/cli";
import { HttpServer } from "@effect/platform";
import { BunHttpServer } from "@effect/platform-bun";
import { RpcSerialization } from "@effect/rpc";
import { makeHandlersLayer } from "@workspace/rpc";
import { Console, Effect, Layer } from "effect";
import { type GlobalsAccessor, localReadOnlyOpt } from "../client";
import { CliUserError } from "../errors";
import {
  buildServeApp,
  DEFAULT_HOST,
  DEFAULT_PORT,
  findFreePort,
  isLoopbackHost,
  openBrowser,
} from "../serve-core";
import { startupUpdateNotice } from "../upgrade/update-check";
import { APP_VERSION } from "../version";

const MIN_PORT = 1;
const MAX_PORT = 65_535;

const portOpt = Options.integer("port").pipe(
  Options.withDescription(
    `Port to bind (default ${DEFAULT_PORT}, auto-picks if busy)`
  ),
  Options.withDefault(DEFAULT_PORT)
);
const hostOpt = Options.text("host").pipe(
  Options.withDescription(
    `Interface to bind (default ${DEFAULT_HOST} loopback; --host 0.0.0.0 exposes it on the network — no auth, firewall yourself)`
  ),
  Options.withDefault(DEFAULT_HOST)
);
const openOpt = Options.boolean("open", {
  negationNames: ["no-open"],
}).pipe(
  Options.withDescription("Open the browser on start (use --no-open to skip)"),
  Options.withDefault(true)
);

/** The scoped serve program: build the router, start serving, keep alive. */
const serveProgram = (args: {
  readonly open: boolean;
  readonly host: string;
  readonly port: number;
}) =>
  Effect.gen(function* () {
    const server = yield* HttpServer.HttpServer;
    const { app, uiSource } = yield* buildServeApp(args.host, args.port);

    yield* server.serve(app);

    const address = server.address;
    const port = address._tag === "TcpAddress" ? address.port : DEFAULT_PORT;
    // Only the browser-openable URL uses a dialable host; a wildcard bind
    // (0.0.0.0 / ::) is not itself connectable.
    const displayHost =
      args.host === "0.0.0.0" || args.host === "::" ? "127.0.0.1" : args.host;
    const url = `http://${displayHost}:${port}`;
    yield* Console.log(`Peektrace serving on ${url}`);
    yield* Console.log(`  RPC:    ${url}/rpc`);
    yield* Console.log(`  UI:     ${url}/  (from ${uiSource})`);
    yield* Console.log(
      "  Watch:  on (filesystem-driven refresh via watch.poll)"
    );

    if (!isLoopbackHost(args.host)) {
      yield* Console.warn(
        `  WARNING: bound to ${args.host} (not loopback). Peektrace has no auth — ` +
          "anyone who can reach this port can read your Claude Code data. " +
          "Restrict access with a firewall or use --read-only."
      );
    }

    // Machine-readable readiness line for a supervising desktop shell.
    if (process.env.PEEKTRACE_CLIENT === "desktop") {
      yield* Console.log(`PEEKTRACE_READY:${port}`);
    }

    if (args.open) {
      yield* openBrowser(url);
    }

    // Best-effort, non-blocking release check: forked so it never delays
    // startup, self-timing-out and error-swallowing, and gated/cached inside
    // `startupUpdateNotice`. Prints one line if a newer release exists.
    yield* Effect.forkDaemon(startupUpdateNotice(APP_VERSION));

    yield* Effect.never;
  });

/** `serve` — start the inspector server (RPC + static UI); loopback by default. */
export const makeServe = (globals: GlobalsAccessor) =>
  Command.make(
    "serve",
    { port: portOpt, open: openOpt, host: hostOpt, readOnly: localReadOnlyOpt },
    ({ port, open, host, readOnly }) =>
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
        yield* serveProgram({ open, host, port: chosen }).pipe(
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
