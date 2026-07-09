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
import { makeHandlersLayer, PeektraceRpcs } from "@workspace/rpc";
import { Console, Effect, Layer } from "effect";
import { type GlobalsAccessor, localReadOnlyOpt } from "../client";
import embeddedUI from "../embedded-ui.gen";
import { CliUserError } from "../errors";

const DEFAULT_HOST = "127.0.0.1";
const DEFAULT_PORT = 4321;
const PORT_SCAN_ATTEMPTS = 20;
const NOT_BUILT_STATUS = 503;
const FORBIDDEN_STATUS = 403;
const MIN_PORT = 1;
const MAX_PORT = 65_535;
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

/** Outcome of probing one port: free, taken, or unbindable (with a reason). */
type PortProbe =
  | { readonly _tag: "free"; readonly port: number }
  | { readonly _tag: "inUse" }
  | { readonly _tag: "denied" }
  | { readonly _tag: "error"; readonly message: string };

/** Probe one port on `host`, classifying the bind outcome. */
const tryPort = (port: number, host: string): Effect.Effect<PortProbe> =>
  Effect.async<PortProbe>((resume) => {
    const srv = createServer();
    srv.once("error", (err: NodeJS.ErrnoException) => {
      srv.close();
      if (err.code === "EADDRINUSE") {
        resume(Effect.succeed({ _tag: "inUse" }));
      } else if (err.code === "EACCES") {
        resume(Effect.succeed({ _tag: "denied" }));
      } else {
        resume(
          Effect.succeed({
            _tag: "error",
            message: err.message ?? String(err),
          })
        );
      }
    });
    srv.once("listening", () => {
      srv.close(() => resume(Effect.succeed({ _tag: "free", port })));
    });
    srv.listen(port, host);
  });

/**
 * Find the first free port at or above `start` on `host`. Fails cleanly (typed
 * `CliUserError`, rendered by the boundary) on EACCES (privileged port), an
 * unexpected bind error, or exhausting the scan window — instead of returning a
 * busy port or surfacing a Node stack trace.
 */
const findFreePort = (
  start: number,
  host: string
): Effect.Effect<number, CliUserError> =>
  Effect.gen(function* () {
    const end = start + PORT_SCAN_ATTEMPTS;
    for (let port = start; port < end; port++) {
      const probe = yield* tryPort(port, host);
      if (probe._tag === "free") {
        return probe.port;
      }
      if (probe._tag === "denied") {
        return yield* new CliUserError({
          message: `Permission denied binding port ${port} (privileged port; try a port >= 1024).`,
        });
      }
      if (probe._tag === "error") {
        return yield* new CliUserError({
          message: `Failed to bind port ${port}: ${probe.message}`,
        });
      }
    }
    return yield* new CliUserError({
      message: `No free port in range ${start}..${end}; pass --port to choose another.`,
    });
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
 * Order: `PEEKTRACE_CLIENT_DIR` dev override, then the embedded manifest baked
 * into a compiled binary, then the on-disk inspector `dist/` (source runs).
 */
const resolveStaticHandler = () => {
  const override = process.env.PEEKTRACE_CLIENT_DIR;
  if (override) {
    const dir = resolve(override);
    return { handler: fileSystemStaticHandler(dir), source: dir };
  }
  if (embeddedUI) {
    return { handler: embeddedStaticHandler(embeddedUI), source: "embedded" };
  }
  return { handler: fileSystemStaticHandler(DIST_DIR), source: DIST_DIR };
};

/** Origins + Host authorities that legitimately reach this server's `/rpc`. */
interface RpcAllowlist {
  readonly hosts: ReadonlySet<string>;
  readonly origins: ReadonlySet<string>;
}

/**
 * Build the `/rpc` allowlist from the actually-bound host + port. Loopback binds
 * accept the three loopback authorities (`127.0.0.1` / `localhost` / `[::1]`); an
 * explicit non-loopback `--host` additionally accepts that configured authority
 * (the user opted into network exposure — the Origin check still guards them).
 */
const buildRpcAllowlist = (host: string, port: number): RpcAllowlist => {
  const authorities = [
    `127.0.0.1:${port}`,
    `localhost:${port}`,
    `[::1]:${port}`,
  ];
  if (!isLoopbackHost(host)) {
    authorities.push(`${host}:${port}`);
  }
  return {
    hosts: new Set(authorities),
    origins: new Set(authorities.map((a) => `http://${a}`)),
  };
};

const HOST_FORBIDDEN_REASON = "request rejected (Host header not allowed)";

/**
 * DNS-rebinding guard applied to every route: the `Host` header must name an
 * allowed authority. A missing or spoofed `Host` (e.g. `evil.example.com`,
 * pointed at the loopback IP by a malicious DNS answer) is refused. Returns a
 * human reason when the request must be refused, else `undefined`.
 */
const hostForbiddenReason = (
  headers: Record<string, string | undefined>,
  allow: RpcAllowlist
): string | undefined => {
  const host = headers.host;
  if (host === undefined || !allow.hosts.has(host)) {
    return HOST_FORBIDDEN_REASON;
  }
  return;
};

/**
 * Refuse cross-origin (CSRF) and DNS-rebinding requests to `/rpc`. A present
 * `Origin` must exactly equal the server's own origin (defeats a malicious page's
 * `fetch`); the `Host` header must be an allowed authority (defeats DNS
 * rebinding). No `Origin` (curl, same-origin GET) passes the Origin gate. Returns
 * a human reason when the request must be refused, else `undefined`.
 */
const rpcForbiddenReason = (
  headers: Record<string, string | undefined>,
  allow: RpcAllowlist
): string | undefined => {
  const origin = headers.origin;
  if (origin !== undefined && !allow.origins.has(origin)) {
    return "cross-origin request rejected (Origin not allowed)";
  }
  return hostForbiddenReason(headers, allow);
};

/** The scoped serve program: build the router, start serving, keep alive. */
const serveProgram = (args: {
  readonly open: boolean;
  readonly host: string;
  readonly port: number;
}) =>
  Effect.gen(function* () {
    const server = yield* HttpServer.HttpServer;
    const rpcApp = yield* RpcServer.toHttpApp(PeektraceRpcs);
    const { handler: staticHandler, source: uiSource } = resolveStaticHandler();
    const allow = buildRpcAllowlist(args.host, args.port);
    // Guard the RPC surface in front of the mounted app: the embedded same-origin
    // UI still reaches it, but a cross-origin `fetch` or a spoofed Host is 403'd.
    const guardedRpcApp = Effect.gen(function* () {
      const req = yield* HttpServerRequest.HttpServerRequest;
      const reason = rpcForbiddenReason(req.headers, allow);
      if (reason !== undefined) {
        return HttpServerResponse.text(reason, { status: FORBIDDEN_STATUS });
      }
      return yield* rpcApp;
    });
    const router = HttpRouter.empty.pipe(
      HttpRouter.mountApp("/rpc", guardedRpcApp),
      HttpRouter.get("/health", Effect.succeed(HttpServerResponse.text("ok"))),
      HttpRouter.get("/", staticHandler),
      HttpRouter.get("/*", staticHandler)
    );
    // DNS-rebinding guard in front of *every* route (static `/` included, not
    // just `/rpc`): a request whose `Host` is not an allowed authority is 403'd
    // before dispatch. The stricter Origin (CSRF) check stays scoped to `/rpc`.
    const guardedRouter = Effect.gen(function* () {
      const req = yield* HttpServerRequest.HttpServerRequest;
      const reason = hostForbiddenReason(req.headers, allow);
      if (reason !== undefined) {
        return HttpServerResponse.text(reason, { status: FORBIDDEN_STATUS });
      }
      return yield* router;
    });

    yield* server.serve(guardedRouter);

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

    // WatchService is provisioned as part of `makeHandlersLayer` (see
    // packages/rpc): a scoped fiber watches the agent roots for the lifetime of
    // this server scope and feeds the `watch.poll` RPC the inspector polls.

    if (args.open) {
      yield* openBrowser(url);
    }
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
