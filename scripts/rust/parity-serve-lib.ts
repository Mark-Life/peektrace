/** Serve-stage plumbing: start a TS and a Rust server, call TS `/rpc` and
 * Rust `/api`, send raw HTTP, and diff JSON values key-order-free. */
import { connect } from "node:net";
import type { Subprocess } from "bun";
import { REPO, report, rustBin, TS_CMD } from "./parity-lib";

const READY = /PEEKTRACE_READY:(\d+)/;
const START_TIMEOUT_MS = 30_000;
const MAX_PATHS = 10;

export interface Server {
  readonly label: "ts" | "rust";
  /** Everything printed so far. */
  readonly out: { stdout: string; stderr: string };
  readonly port: number;
  readonly proc: Subprocess<"ignore", "pipe", "pipe">;
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

export const stopServer = async (s: Server | undefined) => {
  if (s) {
    s.proc.kill();
    await s.proc.exited;
  }
};

/** What a TS `/rpc` call ended with. */
export type RpcExit =
  | { readonly kind: "ok"; readonly value: unknown }
  | { readonly kind: "fail"; readonly error: unknown }
  | { readonly kind: "die"; readonly cause: unknown };

/** One TS RPC call over NDJSON. */
export const rpc = async (
  port: number,
  tag: string,
  payload: unknown = {}
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

/** Send `request` as written over a plain socket; parse the answer. */
export const raw = (port: number, request: string) =>
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
