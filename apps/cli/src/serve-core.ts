/** Shared HTTP-serving core for `peektrace serve` and `peektrace tui`.
 *
 * Both commands stand up the same loopback HTTP surface — the Effect-RPC handler
 * at `POST /rpc` plus the static inspector assets — guarded by the same
 * DNS-rebinding + CSRF checks. This module owns that construction so the two
 * commands share one source of truth: `serve` wraps it with human-readable
 * startup logging; `tui` mounts it quietly beside the terminal UI, so the web
 * app and the CLI UI read one and the same in-process backend.
 */

import { spawn } from "node:child_process";
import { createServer } from "node:net";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  FileSystem,
  HttpRouter,
  HttpServerRequest,
  HttpServerResponse,
} from "@effect/platform";
import { RpcServer } from "@effect/rpc";
import { PeektraceRpcs } from "@workspace/rpc";
import { Effect } from "effect";
import embeddedUI from "./embedded-ui.gen";
import { CliUserError } from "./errors";

export const DEFAULT_HOST = "127.0.0.1";
export const DEFAULT_PORT = 4321;
const PORT_SCAN_ATTEMPTS = 20;
const NOT_BUILT_STATUS = 503;
const FORBIDDEN_STATUS = 403;
const LEADING_SLASHES = /^\/+/;

const HERE = fileURLToPath(new URL(".", import.meta.url));
/** Built inspector assets: apps/cli/src -> apps/inspector/dist. */
const DIST_DIR = resolve(HERE, "..", "..", "inspector", "dist");

/** True when `host` is a loopback interface (no off-box exposure). */
export const isLoopbackHost = (host: string) =>
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
 * `CliUserError`) on EACCES, an unexpected bind error, or exhausting the scan
 * window — instead of returning a busy port or surfacing a Node stack trace.
 */
export const findFreePort = (
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
export const openBrowser = (url: string): Effect.Effect<void> =>
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
export const resolveStaticHandler = () => {
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
export interface RpcAllowlist {
  readonly hosts: ReadonlySet<string>;
  readonly origins: ReadonlySet<string>;
}

/**
 * Build the `/rpc` allowlist from the actually-bound host + port. Loopback binds
 * accept the three loopback authorities; an explicit non-loopback `--host`
 * additionally accepts that configured authority.
 */
export const buildRpcAllowlist = (host: string, port: number): RpcAllowlist => {
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
 * DNS-rebinding guard: the `Host` header must name an allowed authority. Returns
 * a human reason when the request must be refused, else `undefined`.
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
 * `Origin` must exactly equal the server's own origin; the `Host` header must be
 * an allowed authority. Returns a human reason when refused, else `undefined`.
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

/**
 * Build the fully-guarded HTTP app that both commands serve: the RPC handler
 * (front-guarded against CSRF + DNS rebinding) mounted at `/rpc`, a `/health`
 * probe, and the static inspector UI at `/` — all behind an outer DNS-rebinding
 * guard applied to every route. Requires the RPC serialization + handlers layer
 * in context (the caller provides them); yields the UI source label alongside.
 */
export const buildServeApp = (host: string, port: number) =>
  Effect.gen(function* () {
    const rpcApp = yield* RpcServer.toHttpApp(PeektraceRpcs);
    const { handler: staticHandler, source: uiSource } = resolveStaticHandler();
    const allow = buildRpcAllowlist(host, port);
    // Guard the RPC surface: the embedded same-origin UI still reaches it, but a
    // cross-origin `fetch` or a spoofed Host is 403'd.
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
    // DNS-rebinding guard in front of *every* route (static `/` included): a
    // request whose `Host` is not allowed is 403'd before dispatch.
    const guardedRouter = Effect.gen(function* () {
      const req = yield* HttpServerRequest.HttpServerRequest;
      const reason = hostForbiddenReason(req.headers, allow);
      if (reason !== undefined) {
        return HttpServerResponse.text(reason, { status: FORBIDDEN_STATUS });
      }
      return yield* router;
    });
    return { app: guardedRouter, uiSource };
  });
