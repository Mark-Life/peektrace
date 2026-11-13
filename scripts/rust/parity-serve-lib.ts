/** Serve-stage plumbing: start a TS and a Rust server, call TS `/rpc` and
 * Rust `/api`, send raw HTTP, and diff JSON values key-order-free. */
import { mkdirSync } from "node:fs";
import { connect, createServer } from "node:net";
import { join } from "node:path";
import type { Subprocess } from "bun";
import { REPO, report, rustBin, TS_CMD, WORK } from "./parity-lib";
import { ORACLE_MODE, PINNED, tsValue } from "./parity-oracle";

const READY = /PEEKTRACE_READY:(\d+)/;
const START_TIMEOUT_MS = 30_000;
const MAX_PATHS = 10;
/** TS servers listen on fixed ports when goldens are recorded or replayed,
 * so the keys and the recorded text do not move from run to run. */
const TS_PORT_BASE = 47_310;
const TS_PORT_SPAN = 100;
let tsServers = 0;
const tsPorts = new Set<number>();

export interface Server {
  readonly label: "ts" | "rust";
  /** Everything printed so far. */
  readonly out: { stdout: string; stderr: string };
  readonly port: number;
  /** Absent for a replayed TS server: nothing runs. */
  readonly proc?: Subprocess<"ignore", "pipe", "pipe">;
}

const drain = async (
  stream: ReadableStream<Uint8Array>,
  sink: (s: string) => void
) => {
  const decoder = new TextDecoder();
  for await (const chunk of stream) {
    sink(decoder.decode(chunk, { stream: true }));
  }
};

const running = new Set<Subprocess>();
const stopAll = () => {
  for (const proc of running) {
    proc.kill();
  }
};
process.on("exit", stopAll);
for (const signal of ["SIGINT", "SIGTERM"] as const) {
  process.on(signal, () => {
    stopAll();
    process.exit(130);
  });
}

/** Start `serve` and wait for its `PEEKTRACE_READY:<port>` line. */
export const startServer = async (
  label: "ts" | "rust",
  args: readonly string[],
  env: Record<string, string>
): Promise<Server> => {
  const cmd = label === "ts" ? [...TS_CMD, "serve"] : [rustBin, "serve"];
  const proc = Bun.spawn([...cmd, ...args, "--no-open"], {
    env: {
      ...env,
      PEEKTRACE_CLIENT: "desktop",
      PEEKTRACE_NO_UPDATE_CHECK: "1",
    },
    cwd: REPO,
    stdin: "ignore",
    stdout: "pipe",
    stderr: "pipe",
  });
  running.add(proc);
  const out = { stdout: "", stderr: "" };
  drain(proc.stdout, (s) => {
    out.stdout += s;
  });
  drain(proc.stderr, (s) => {
    out.stderr += s;
  });
  const deadline = Date.now() + START_TIMEOUT_MS;
  while (Date.now() < deadline) {
    const m = READY.exec(out.stdout);
    if (m) {
      return { label, port: Number(m[1]), proc, out };
    }
    if (proc.exitCode !== null) {
      break;
    }
    await Bun.sleep(50);
  }
  proc.kill();
  throw new Error(`${label} serve did not start: ${out.stdout}${out.stderr}`);
};

/** A TS and a Rust server over the same data. */
export interface Pair {
  readonly rust: Server;
  readonly rustDir: string;
  readonly ts: Server;
  readonly tsDir: string;
}

const anyPort = () =>
  new Promise<number>((done) => {
    const srv = createServer();
    srv.listen(0, "127.0.0.1", () => {
      const address = srv.address();
      const port = typeof address === "object" && address ? address.port : 0;
      srv.close(() => done(port));
    });
  });

const isTsPort = (port: number) =>
  port >= TS_PORT_BASE && port < TS_PORT_BASE + TS_PORT_SPAN;

/** A free port far from the others in use, never a fixed TS one. */
export const freePort = async (): Promise<number> => {
  const port = await anyPort();
  return PINNED && isTsPort(port) ? freePort() : port;
};

const tsPort = () => {
  if (!PINNED) {
    return freePort();
  }
  const port = TS_PORT_BASE + tsServers;
  tsServers += 1;
  return Promise.resolve(port);
};

/** The TS server of a pair: started, or in replay a stub with the startup
 * output it printed when recorded. */
const startTs = async (
  name: string,
  args: readonly string[],
  env: Record<string, string>
): Promise<Server> => {
  const port = await tsPort();
  tsPorts.add(port);
  const key = `serve ${name} startup`;
  if (ORACLE_MODE === "replay") {
    const out = await tsValue(key, () => ({ stdout: "", stderr: "" }));
    return { label: "ts", port, out };
  }
  const server = await startServer(
    "ts",
    ["--port", String(port), ...args],
    env
  );
  if (PINNED && server.port !== port) {
    await stopServer(server);
    throw new Error(
      `TS serve took port ${server.port}, not ${port}: free ${TS_PORT_BASE}-${TS_PORT_BASE + TS_PORT_SPAN - 1}`
    );
  }
  await tsValue(key, () => ({ ...server.out }));
  return server;
};

/** Start a TS and a Rust server with the same flags. Each gets its own
 * `PEEKTRACE_DIR`, or both get `shared` (TS caches stats in `stats/`, Rust
 * in `stats-rs/`). */
export type Side = "ts" | "rust";
export type PairEnv =
  | Record<string, string>
  | ((side: Side) => Record<string, string>);

export const startPair = async (
  name: string,
  pairEnv: PairEnv,
  flags: readonly string[],
  shared?: string
): Promise<Pair> => {
  const env = (side: Side) =>
    typeof pairEnv === "function" ? pairEnv(side) : pairEnv;
  const tsDir = shared ?? join(WORK, "serve", name, "ts-dir");
  const rustDir = shared ?? join(WORK, "serve", name, "rust-dir");
  mkdirSync(tsDir, { recursive: true });
  mkdirSync(rustDir, { recursive: true });
  const ts = await startTs(name, flags, {
    ...env("ts"),
    PEEKTRACE_DIR: tsDir,
  });
  try {
    const rust = await startServer(
      "rust",
      ["--port", String(await freePort()), ...flags],
      { ...env("rust"), PEEKTRACE_DIR: rustDir }
    );
    return { rust, rustDir, ts, tsDir };
  } catch (e) {
    await stopServer(ts);
    throw e;
  }
};

export const withPair = async (
  name: string,
  env: PairEnv,
  flags: readonly string[],
  use: (p: Pair) => Promise<void>,
  shared?: string
) => {
  const pair = await startPair(name, env, flags, shared);
  try {
    await use(pair);
  } finally {
    await stopServer(pair.ts);
    await stopServer(pair.rust);
  }
};

export const stopServer = async (s: Server | undefined) => {
  if (s?.proc) {
    running.delete(s.proc);
    s.proc.kill();
    await s.proc.exited;
  }
};

/** What a TS `/rpc` call ended with. */
export type RpcExit =
  | { readonly kind: "ok"; readonly value: unknown }
  | { readonly kind: "fail"; readonly error: unknown }
  | { readonly kind: "die"; readonly cause: unknown };

/** One TS RPC call over NDJSON; recorded (through `canon`, if given) and
 * replayed as a TS result. */
export const rpc = (
  port: number,
  tag: string,
  payload: unknown = {},
  canon?: (exit: RpcExit) => RpcExit
): Promise<RpcExit> =>
  tsValue(
    `rpc ${port} ${tag} ${JSON.stringify(payload)}`,
    () => rpcLive(port, tag, payload),
    canon === undefined ? {} : { canon }
  );

const rpcLive = async (
  port: number,
  tag: string,
  payload: unknown
): Promise<RpcExit> => {
  const body = `${JSON.stringify({ _tag: "Request", id: "1", tag, payload, headers: [] })}\n`;
  const res = await fetch(`http://127.0.0.1:${port}/rpc`, {
    method: "POST",
    body,
    headers: { "content-type": "application/ndjson" },
  });
  const lines = (await res.text())
    .split("\n")
    .filter(Boolean)
    .map((l) => JSON.parse(l));
  const exit = lines.find((l) => l._tag === "Exit")?.exit;
  if (exit?._tag === "Success") {
    return { kind: "ok", value: exit.value };
  }
  const cause = exit?.cause;
  return cause?._tag === "Fail"
    ? { kind: "fail", error: cause.error }
    : { kind: "die", cause: cause ?? lines };
};

export interface ApiAnswer {
  readonly body: Uint8Array;
  readonly json: unknown;
  readonly status: number;
  readonly type: string;
}

/** One Rust `/api` call. */
export const api = async (
  port: number,
  path: string,
  init: RequestInit = {}
): Promise<ApiAnswer> => {
  const res = await fetch(`http://127.0.0.1:${port}${path}`, init);
  const body = new Uint8Array(await res.arrayBuffer());
  const type = res.headers.get("content-type") ?? "";
  let json: unknown;
  if (type.startsWith("application/json")) {
    json = JSON.parse(new TextDecoder().decode(body));
  }
  return { body, json, status: res.status, type };
};

export interface RawAnswer {
  readonly body: string;
  readonly headers: Record<string, string>;
  readonly status: number;
}

/** Send `request` as written over a plain socket; parse the answer. A TS
 * server's answer is recorded and replayed. */
export const raw = (port: number, request: string) =>
  tsPorts.has(port)
    ? tsValue(`raw ${request}`, () => rawLive(port, request), {
        canon: comparedHeaders,
      })
    : rawLive(port, request);

/** Headers the serve stage compares; the rest (date, etag) move per run. */
const COMPARED_HEADERS = new Set([
  "cache-control",
  "content-type",
  "location",
  "referrer-policy",
  "set-cookie",
]);

const comparedHeaders = (a: RawAnswer): RawAnswer => ({
  ...a,
  headers: Object.fromEntries(
    Object.entries(a.headers).filter(([k]) => COMPARED_HEADERS.has(k))
  ),
});

const rawLive = (port: number, request: string) =>
  new Promise<RawAnswer>((resolveAnswer, reject) => {
    const socket = connect(port, "127.0.0.1");
    const chunks: Buffer[] = [];
    socket.on("data", (c) => chunks.push(c));
    socket.on("error", reject);
    socket.on("end", () => {
      const text = Buffer.concat(chunks).toString("latin1");
      const split = text.indexOf("\r\n\r\n");
      const head = text.slice(0, split).split("\r\n");
      const headers: Record<string, string> = {};
      for (const line of head.slice(1)) {
        const at = line.indexOf(":");
        headers[line.slice(0, at).toLowerCase()] = line.slice(at + 1).trim();
      }
      let body = text.slice(split + 4);
      if (headers["transfer-encoding"] === "chunked") {
        body = unchunk(body);
      }
      resolveAnswer({
        body,
        headers,
        status: Number(head[0]?.split(" ")[1]),
      });
    });
    socket.write(request);
  });

const unchunk = (s: string) => {
  let out = "";
  let rest = s;
  for (;;) {
    const at = rest.indexOf("\r\n");
    const size = Number.parseInt(rest.slice(0, at), 16);
    if (!(size > 0)) {
      return out;
    }
    out += rest.slice(at + 2, at + 2 + size);
    rest = rest.slice(at + 2 + size + 2);
  }
};

const kindOf = (v: unknown) => {
  if (v === null) {
    return "null";
  }
  return Array.isArray(v) ? "array" : typeof v;
};

const brief = (v: unknown) => JSON.stringify(v)?.slice(0, 80);

/** Up to 10 paths where two JSON values differ; key order is ignored and
 * numbers compare with `===`. */
export const jsonDiff = (a: unknown, b: unknown, path = "$"): string[] => {
  const out: string[] = [];
  const walk = (x: unknown, y: unknown, p: string): void => {
    if (out.length >= MAX_PATHS) {
      return;
    }
    if (kindOf(x) !== kindOf(y)) {
      out.push(`${p}: ${brief(x)} vs ${brief(y)}`);
    } else if (Array.isArray(x) && Array.isArray(y)) {
      walkArray(x, y, p);
    } else if (kindOf(x) === "object") {
      walkObject(x as Record<string, unknown>, y as Record<string, unknown>, p);
    } else if (x !== y) {
      out.push(`${p}: ${brief(x)} vs ${brief(y)}`);
    }
  };
  const walkArray = (x: unknown[], y: unknown[], p: string) => {
    if (x.length !== y.length) {
      out.push(`${p}: length ${x.length} vs ${y.length}`);
    }
    for (const [i, v] of x.slice(0, y.length).entries()) {
      walk(v, y[i], `${p}[${i}]`);
    }
  };
  const walkObject = (
    x: Record<string, unknown>,
    y: Record<string, unknown>,
    p: string
  ) => {
    for (const k of new Set([...Object.keys(x), ...Object.keys(y)])) {
      if (k in x && k in y) {
        walk(x[k], y[k], `${p}.${k}`);
      } else {
        out.push(`${p}.${k}: missing in ${k in x ? "rust" : "ts"}`);
      }
    }
  };
  walk(a, b, path);
  return out;
};

/** Report every difference of one compared value; true when equal. */
export const sameJson = (label: string, ts: unknown, rust: unknown) => {
  const d = jsonDiff(ts, rust);
  for (const line of d) {
    report(`serve ${label}: ${line}`);
  }
  return d.length === 0;
};

/** Replace every occurrence of `from` in string leaves with `to`. */
export const replaceText = (v: unknown, from: string, to: string): unknown => {
  if (typeof v === "string") {
    return v.split(from).join(to);
  }
  if (Array.isArray(v)) {
    return v.map((x) => replaceText(x, from, to));
  }
  if (v && typeof v === "object") {
    return Object.fromEntries(
      Object.entries(v).map(([k, x]) => [k, replaceText(x, from, to)])
    );
  }
  return v;
};
