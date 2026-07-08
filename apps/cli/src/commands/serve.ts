/** `peephole serve` — the headline command.
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
import embeddedUI from "../embedded-ui.gen";

const DEFAULT_HOST = "127.0.0.1";
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

/** True when `host` is a loopback interface (no off-box exposure). */
const isLoopbackHost = (host: string) =>
  host === "127.0.0.1" || host === "localhost" || host === "::1";

/** Probe one port on `host`; resolves -1 when in use. */
const tryPort = (port: number, host: string): Effect.Effect<number> =>
  Effect.async<number>((resume) => {
    const srv = createServer();
    srv.once("error", () => {
      srv.close();
      resume(Effect.succeed(-1));
    });
    srv.once("listening", () => {
      srv.close(() => resume(Effect.succeed(port)));
    });
    srv.listen(port, host);
  });

/** Find the first free port at or above `start` on `host`, falling back to `start`. */
const findFreePort = (start: number, host: string): Effect.Effect<number> =>
  Effect.gen(function* () {
    for (let port = start; port < start + PORT_SCAN_ATTEMPTS; port++) {
      const free = yield* tryPort(port, host);
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

/** Request pathname without the query string or leading slashes. */
const requestPathname = (url: string) =>
  decodeURIComponent((url.split("?")[0] ?? "/").replace(LEADING_SLASHES, ""));

/**
 * Filesystem static-asset handler: serve `<clientDir>/<path>` when it resolves
 * to a real file inside the root, else fall back to `index.html` (SPA client
 * routing), else a 503 when the UI was never built.
 */
const fileSystemStaticHandler = (clientDir: string) =>
  Effect.gen(function* () {
    const req = yield* HttpServerRequest.HttpServerRequest;
    const fs = yield* FileSystem.FileSystem;
    const pathname = requestPathname(req.url);
    const candidate = resolve(clientDir, pathname);
    const indexHtml = join(clientDir, "index.html");
    const inRoot =
      candidate === clientDir || candidate.startsWith(`${clientDir}/`);
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

/** Build a response for one embedded (bunfs) asset, with SPA-friendly caching. */
const embeddedFileResponse = (bunfsPath: string, isIndex: boolean) => {
  const file = Bun.file(bunfsPath);
  const headers: Record<string, string> = {
    "content-type": file.type || "application/octet-stream",
  };
  if (isIndex) {
    headers["cache-control"] = "no-store";
  }
  return HttpServerResponse.raw(new Response(file, { headers }));
};

/**
 * Embedded static-asset handler (compiled binary): resolve the request path in
 * the baked-in manifest, falling back to the embedded `index.html` for
 * client-side routes.
 */
const embeddedStaticHandler = (manifest: Record<string, string>) =>
  Effect.gen(function* () {
    const req = yield* HttpServerRequest.HttpServerRequest;
    const pathname = requestPathname(req.url);
    const key = pathname === "" ? "/index.html" : `/${pathname}`;
    const indexPath = manifest["/index.html"];
    const target = manifest[key] ?? indexPath;
    if (target === undefined) {
      return HttpServerResponse.text("Inspector assets not embedded.", {
        status: NOT_BUILT_STATUS,
      });
    }
    return embeddedFileResponse(target, target === indexPath);
  });

/** Resolve the static handler + a human-readable UI source label.
 *
 * Order: `PEEPHOLE_CLIENT_DIR` dev override, then the embedded manifest baked
 * into a compiled binary, then the on-disk inspector `dist/` (source runs).
 */
const resolveStaticHandler = () => {
  const override = process.env.PEEPHOLE_CLIENT_DIR;
  if (override) {
    const dir = resolve(override);
    return { handler: fileSystemStaticHandler(dir), source: dir };
  }
  if (embeddedUI) {
    return { handler: embeddedStaticHandler(embeddedUI), source: "embedded" };
  }
  return { handler: fileSystemStaticHandler(DIST_DIR), source: DIST_DIR };
};

/** The scoped serve program: build the router, start serving, keep alive. */
const serveProgram = (args: {
  readonly open: boolean;
  readonly host: string;
}) =>
  Effect.gen(function* () {
    const server = yield* HttpServer.HttpServer;
    const rpcApp = yield* RpcServer.toHttpApp(PeepholeRpcs);
    const { handler: staticHandler, source: uiSource } = resolveStaticHandler();
    const router = HttpRouter.empty.pipe(
      HttpRouter.mountApp("/rpc", rpcApp),
      HttpRouter.get("/health", Effect.succeed(HttpServerResponse.text("ok"))),
      HttpRouter.get("/", staticHandler),
      HttpRouter.get("/*", staticHandler)
    );

    yield* server.serve(router);

    const address = server.address;
    const port = address._tag === "TcpAddress" ? address.port : DEFAULT_PORT;
    // Only the browser-openable URL uses a dialable host; a wildcard bind
    // (0.0.0.0 / ::) is not itself connectable.
    const displayHost =
      args.host === "0.0.0.0" || args.host === "::" ? "127.0.0.1" : args.host;
    const url = `http://${displayHost}:${port}`;
    yield* Console.log(`Peephole serving on ${url}`);
    yield* Console.log(`  RPC:    ${url}/rpc`);
    yield* Console.log(`  UI:     ${url}/  (from ${uiSource})`);
    yield* Console.log(
      "  Watch:  on (filesystem-driven refresh via watch.poll)"
    );

    if (!isLoopbackHost(args.host)) {
      yield* Console.warn(
        `  WARNING: bound to ${args.host} (not loopback). Peephole has no auth — ` +
          "anyone who can reach this port can read your Claude Code data. " +
          "Restrict access with a firewall or use --read-only."
      );
    }

    // Machine-readable readiness line for a supervising desktop shell.
    if (process.env.PEEPHOLE_CLIENT === "desktop") {
      yield* Console.log(`PEEPHOLE_READY:${port}`);
    }

    // WatchService is provisioned as part of `makeHandlersLayer` (see
    // packages/rpc): a scoped fiber watches the agent roots for the lifetime of
    // this server scope and feeds the `watch.poll` RPC the inspector polls.

    if (args.open) {
      yield* openBrowser(url);
    }
    yield* Effect.never;
  });

/** `serve` — start the inspector server (RPC + static UI); loopback by default. */
export const makeServe = () =>
  Command.make(
    "serve",
    { port: portOpt, open: openOpt, host: hostOpt },
    ({ port, open, host }) =>
      Effect.gen(function* () {
        const chosen = yield* findFreePort(port, host);
        const serverLayer = BunHttpServer.layer({
          port: chosen,
          hostname: host,
        });
        yield* serveProgram({ open, host }).pipe(
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
