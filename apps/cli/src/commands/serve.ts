/** `peephole serve` — the headline command.
 *
 * Boots a loopback-only Bun HTTP server that:
 * - mounts the Effect-RPC handler (NDJSON over HTTP) at `POST /rpc`, backed by the
 *   real core layers provisioned once at boot via `makeHandlersLayer`;
 * - serves the built inspector static assets from `apps/inspector/dist` at `/`,
 *   falling back to `index.html` for client-side routes;
 * - binds `127.0.0.1:<port>` (default 4321, auto-picking the next free port if
 *   busy) and opens the browser unless `--no-open`.
 *
 * Filesystem-driven live refresh ships via the `WatchService` baked into
 * `makeHandlersLayer`: a scoped watcher fiber runs for the server's lifetime and
 * advances the per-scope versions the inspector polls through `watch.poll`.
 */
import { spawn } from "node:child_process";
import { createServer } from "node:net";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { Command, Options } from "@effect/cli";
import {
  FileSystem,
  HttpRouter,
  HttpServer,
  HttpServerRequest,
  HttpServerResponse,
} from "@effect/platform";
import { BunHttpServer } from "@effect/platform-bun";
import { RpcSerialization, RpcServer } from "@effect/rpc";
import { makeHandlersLayer, PeepholeRpcs } from "@workspace/rpc";
import { Console, Effect, Layer } from "effect";

const HOST = "127.0.0.1";
const DEFAULT_PORT = 4321;
const PORT_SCAN_ATTEMPTS = 20;
const NOT_BUILT_STATUS = 503;
const PORT_IN_USE = -1;
const LEADING_SLASHES = /^\/+/;

const HERE = fileURLToPath(new URL(".", import.meta.url));
/** Built inspector assets: apps/cli/src/commands -> apps/inspector/dist. */
const DIST_DIR = resolve(HERE, "..", "..", "..", "inspector", "dist");

const portOpt = Options.integer("port").pipe(
  Options.withDescription(
    `Port to bind (default ${DEFAULT_PORT}, auto-picks if busy)`
  ),
  Options.withDefault(DEFAULT_PORT)
);
const openOpt = Options.boolean("open", {
  negationNames: ["no-open"],
}).pipe(
  Options.withDescription("Open the browser on start (use --no-open to skip)"),
  Options.withDefault(true)
);

/** Probe one port on the loopback interface; resolves -1 when in use. */
const tryPort = (port: number): Effect.Effect<number> =>
  Effect.async<number>((resume) => {
    const srv = createServer();
    srv.once("error", () => {
      srv.close();
      resume(Effect.succeed(-1));
    });
    srv.once("listening", () => {
      srv.close(() => resume(Effect.succeed(port)));
    });
    srv.listen(port, HOST);
  });

/** Find the first free port at or above `start`, falling back to `start`. */
const findFreePort = (start: number): Effect.Effect<number> =>
  Effect.gen(function* () {
    for (let port = start; port < start + PORT_SCAN_ATTEMPTS; port++) {
      const free = yield* tryPort(port);
      if (free !== PORT_IN_USE) {
        return free;
      }
    }
    return start;
  });

/** Platform-specific `[command, ...args]` to open a URL in the default browser. */
const browserArgv = (url: string): readonly string[] => {
  if (process.platform === "darwin") {
    return ["open", url];
  }
  if (process.platform === "win32") {
    return ["cmd", "/c", "start", "", url];
  }
  return ["xdg-open", url];
};

/** Open `url` in the default browser (best-effort, never fails the server). */
const openBrowser = (url: string): Effect.Effect<void> =>
  Effect.sync(() => {
    const [cmd, ...args] = browserArgv(url);
    if (cmd !== undefined) {
      spawn(cmd, args, { stdio: "ignore", detached: true }).unref();
    }
  }).pipe(Effect.ignore);

/**
 * Static-asset handler: serve `dist/<path>` when it resolves to a real file
 * inside the dist root, else fall back to `index.html` (SPA client routing).
 */
const staticHandler = Effect.gen(function* () {
  const req = yield* HttpServerRequest.HttpServerRequest;
  const fs = yield* FileSystem.FileSystem;
  const pathname = decodeURIComponent(
    (req.url.split("?")[0] ?? "/").replace(LEADING_SLASHES, "")
  );
  const candidate = resolve(DIST_DIR, pathname);
  const indexHtml = join(DIST_DIR, "index.html");
  const inRoot = candidate === DIST_DIR || candidate.startsWith(`${DIST_DIR}/`);
  if (inRoot && pathname !== "") {
    const isFile = yield* fs.stat(candidate).pipe(
      Effect.map((info) => info.type === "File"),
      Effect.orElseSucceed(() => false)
    );
    if (isFile) {
      return yield* HttpServerResponse.file(candidate);
    }
  }
  return yield* HttpServerResponse.file(indexHtml).pipe(
    Effect.orElse(() =>
      Effect.succeed(
        HttpServerResponse.text(
          "Inspector assets not built. Run: cd apps/inspector && bun run build",
          { status: NOT_BUILT_STATUS }
        )
      )
    )
  );
});

/** The scoped serve program: build the router, start serving, keep alive. */
const serveProgram = (args: { readonly open: boolean }) =>
  Effect.gen(function* () {
    const server = yield* HttpServer.HttpServer;
    const rpcApp = yield* RpcServer.toHttpApp(PeepholeRpcs);
    const router = HttpRouter.empty.pipe(
      HttpRouter.mountApp("/rpc", rpcApp),
      HttpRouter.get("/", staticHandler),
      HttpRouter.get("/*", staticHandler)
    );

    yield* server.serve(router);

    const address = server.address;
    const port = address._tag === "TcpAddress" ? address.port : DEFAULT_PORT;
    const url = `http://${HOST}:${port}`;
    yield* Console.log(`Peephole serving on ${url}`);
    yield* Console.log(`  RPC:    ${url}/rpc`);
    yield* Console.log(`  UI:     ${url}/  (from ${DIST_DIR})`);
    yield* Console.log(
      "  Watch:  on (filesystem-driven refresh via watch.poll)"
    );

    // WatchService is provisioned as part of `makeHandlersLayer` (see
    // packages/rpc): a scoped fiber watches the agent roots for the lifetime of
    // this server scope and feeds the `watch.poll` RPC the inspector polls.

    if (args.open) {
      yield* openBrowser(url);
    }
    yield* Effect.never;
  });

/** `serve` — start the loopback inspector server (RPC + static UI). */
export const makeServe = () =>
  Command.make("serve", { port: portOpt, open: openOpt }, ({ port, open }) =>
    Effect.gen(function* () {
      const chosen = yield* findFreePort(port);
      const serverLayer = BunHttpServer.layer({ port: chosen, hostname: HOST });
      yield* serveProgram({ open }).pipe(
        Effect.scoped,
        Effect.provide(
          Layer.mergeAll(
            serverLayer,
            RpcSerialization.layerNdjson,
            makeHandlersLayer({ rootSpans: true })
          )
        )
      );
    })
  );
