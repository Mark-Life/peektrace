/** Serve-stage HTTP cases: guards, token auth, static files, startup lines
 * and the `--remote` CLI, sent the same way to both servers. */
import { createHash } from "node:crypto";
import { readdirSync } from "node:fs";
import { join } from "node:path";
import { compareRun, report, run, rustBin, TS_CMD } from "./parity-lib";
import { bind } from "./parity-oracle";
import type { Counts, Pair } from "./parity-serve-cases";
import { type RawAnswer, raw, type Server } from "./parity-serve-lib";

const RPC_BODY = `${JSON.stringify({ _tag: "Request", id: "1", tag: "server.info", payload: {}, headers: [] })}\n`;

/** One request shape: `{P}` is the server's port, `{T}` the token. The TS
 * `/rpc` POST stands for the Rust `/api/server` GET. */
interface Probe {
  readonly headers?: readonly string[];
  readonly http10?: boolean;
  readonly label: string;
  readonly method?: string;
  readonly noHost?: boolean;
  readonly path: string;
}

const API = "<api>";

const request = (s: Server, p: Probe, token: string) => {
  const fill = (t: string) =>
    t.replaceAll("{P}", String(s.port)).replaceAll("{T}", token);
  const isApi = p.path === API;
  const isTs = s.label === "ts";
  const apiPath = isTs ? "/rpc" : "/api/server";
  const apiMethod = isTs ? "POST" : "GET";
  const path = isApi ? apiPath : fill(p.path);
  const method = isApi ? apiMethod : (p.method ?? "GET");
  const body = isApi && s.label === "ts" ? RPC_BODY : "";
  const lines = [
    `${method} ${path} HTTP/${p.http10 ? "1.0" : "1.1"}`,
    ...(p.noHost || p.headers?.some((h) => h.toLowerCase().startsWith("host:"))
      ? []
      : [`Host: 127.0.0.1:${s.port}`]),
    ...(p.headers ?? []).map(fill),
    "Connection: close",
    ...(body === ""
      ? []
      : ["content-type: application/ndjson", `content-length: ${body.length}`]),
  ];
  return `${lines.join("\r\n")}\r\n\r\n${body}`;
};

/** The parts of an answer both servers must agree on. */
const shape = (a: RawAnswer, s: Server, p: Probe) => {
  const port = (v: string | undefined) => v?.replaceAll(String(s.port), "{P}");
  const ok = a.status >= 200 && a.status < 300;
  return {
    status: a.status,
    location: port(a.headers.location),
    cookie: port(a.headers["set-cookie"]),
    referrer: a.headers["referrer-policy"],
    cache: a.status === 401 ? a.headers["cache-control"] : undefined,
    body: ok && p.path === API ? "<api body>" : a.body,
  };
};

const probeBoth = async (
  pair: Pair,
  c: Counts,
  probes: readonly Probe[],
  token = ""
) => {
  for (const p of probes) {
    c.cases += 1;
    const [t, r] = await Promise.all([
      raw(pair.ts.port, request(pair.ts, p, token)),
      raw(pair.rust.port, request(pair.rust, p, token)),
    ]);
    const a = shape(t, pair.ts, p);
    const b = shape(r, pair.rust, p);
    for (const k of Object.keys(a) as (keyof typeof a)[]) {
      if (a[k] !== b[k]) {
        report(
          `serve http ${p.label}: ${k} ts=${JSON.stringify(a[k])} rust=${JSON.stringify(b[k])}`
        );
      }
    }
  }
};

/** Host and Origin guards on a loopback bind. */
export const loopbackHttp = (pair: Pair, c: Counts) =>
  probeBoth(pair, c, [
    { label: "health", path: "/health" },
    {
      label: "health evil origin",
      path: "/health",
      headers: ["Origin: http://evil.com"],
    },
    { label: "no host 1.0", path: "/health", http10: true, noHost: true },
    { label: "host evil", path: "/health", headers: ["Host: evil.com:{P}"] },
    { label: "host upper", path: "/health", headers: ["Host: LOCALHOST:{P}"] },
    { label: "host no port", path: "/health", headers: ["Host: 127.0.0.1"] },
    {
      label: "host other port",
      path: "/health",
      headers: ["Host: 127.0.0.1:1"],
    },
    { label: "api", path: API },
    {
      label: "api localhost host",
      path: API,
      headers: ["Host: localhost:{P}"],
    },
    { label: "api ipv6 host", path: API, headers: ["Host: [::1]:{P}"] },
    {
      label: "api own origin",
      path: API,
      headers: ["Origin: http://127.0.0.1:{P}"],
    },
    {
      label: "api evil origin",
      path: API,
      headers: ["Origin: http://evil.com"],
    },
    {
      label: "api https origin",
      path: API,
      headers: ["Origin: https://127.0.0.1:{P}"],
    },
  ]);

/** Token auth on a non-loopback bind. */
export const authHttp = (pair: Pair, c: Counts, token: string) => {
  const cookie = "peektrace_token_{P}";
  return probeBoth(
    pair,
    c,
    [
      { label: "auth health", path: "/health" },
      { label: "auth api", path: API },
      { label: "auth root", path: "/" },
      {
        label: "auth bearer",
        path: "/health",
        headers: ["Authorization: Bearer {T}"],
      },
      {
        label: "auth bearer lower",
        path: "/health",
        headers: ["Authorization: bearer   {T}"],
      },
      {
        label: "auth basic",
        path: "/health",
        headers: ["Authorization: Basic {T}"],
      },
      {
        label: "auth wrong bearer",
        path: "/health",
        headers: ["Authorization: Bearer nope"],
      },
      {
        label: "auth cookie bad first",
        path: "/health",
        headers: [`Cookie: ${cookie}=bad; ${cookie}={T}`],
      },
      {
        label: "auth cookie good first",
        path: "/health",
        headers: [`Cookie: ${cookie}={T}; ${cookie}=bad`],
      },
      {
        label: "auth cookie quoted",
        path: "/health",
        headers: [`Cookie: ${cookie}="{T}"`],
      },
      { label: "auth query", path: "/?token={T}" },
      { label: "auth query wrong", path: "/?token=nope" },
      { label: "auth query good bad", path: "/?token={T}&token=bad" },
      { label: "auth query bad good", path: "/?token=bad&token={T}" },
      {
        label: "auth query keeps params",
        path: "/a%20b/?x=a+b&token={T}&y=%41",
      },
      { label: "auth query protocol-relative", path: "//evil.com?token={T}" },
      { label: "auth query dot slashes", path: "/.//evil.example/x?token={T}" },
      { label: "auth query head", method: "HEAD", path: "/?token={T}" },
      { label: "auth query post", method: "POST", path: "/?token={T}" },
      {
        label: "auth cookie plus query",
        path: "/x?token={T}",
        headers: [`Cookie: ${cookie}={T}`],
      },
      {
        label: "auth lan host",
        path: "/health",
        headers: ["Host: evil.com", "Authorization: Bearer {T}"],
      },
      {
        label: "auth no host",
        path: "/health",
        http10: true,
        noHost: true,
        headers: ["Authorization: Bearer {T}"],
      },
      {
        label: "auth api https origin",
        path: API,
        headers: ["Origin: https://127.0.0.1:{P}", "Authorization: Bearer {T}"],
      },
      {
        label: "auth api evil origin",
        path: API,
        headers: ["Origin: http://evil.com", "Authorization: Bearer {T}"],
      },
    ],
    token
  );
};

const sha = (s: string) =>
  createHash("sha256").update(s, "latin1").digest("hex");

/** Static files, SPA fallback and traversal attempts. */
export const staticHttp = async (pair: Pair, c: Counts, dist: string) => {
  const asset =
    readdirSync(join(dist, "assets")).find((f) => f.endsWith(".js")) ?? "";
  const paths = [
    "/",
    "/index.html",
    `/assets/${asset}`,
    "/assets/missing.js",
    "/sessions/abc",
    "/../secret.txt",
    "/%2e%2e/secret.txt",
    "/..%2Fsecret.txt",
    "/assets/..%2F..%2Fsecret.txt",
    `/assets%2F${asset}`,
    "/%E0%A4%A",
  ];
  for (const path of paths) {
    c.cases += 1;
    const [t, r] = await Promise.all(
      [pair.ts, pair.rust].map((s) =>
        raw(
          s.port,
          `GET ${path} HTTP/1.1\r\nHost: 127.0.0.1:${s.port}\r\nConnection: close\r\n\r\n`
        )
      )
    );
    const media = (a: RawAnswer) => a.headers["content-type"]?.split(";")[0];
    if (
      t.status !== r.status ||
      media(t) !== media(r) ||
      sha(t.body) !== sha(r.body)
    ) {
      report(
        `serve static ${path}: ts ${t.status} ${media(t)} vs rust ${r.status} ${media(r)}${sha(t.body) === sha(r.body) ? "" : " (body differs)"}`
      );
    }
  }
  for (const [method, path] of [
    ["POST", "/"],
    ["PUT", "/x"],
  ] as const) {
    c.cases += 1;
    const [t, r] = await Promise.all(
      [pair.ts, pair.rust].map((s) =>
        raw(
          s.port,
          `${method} ${path} HTTP/1.1\r\nHost: 127.0.0.1:${s.port}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n`
        )
      )
    );
    if (t.status !== r.status) {
      report(
        `serve static ${method} ${path}: ts ${t.status} vs rust ${r.status}`
      );
    }
  }
};

/** Startup lines, after mapping the RPC wording to the API wording. */
export const startupLines = (pair: Pair, c: Counts) => {
  c.cases += 1;
  const lines = (s: Server) =>
    s.out.stdout
      .replaceAll(String(s.port), "{P}")
      .replace(
        "  RPC:    http://127.0.0.1:{P}/rpc",
        "  API:    http://127.0.0.1:{P}/api"
      )
      .replace("via watch.poll", "via /api/events")
      .split("\n")
      .filter((l) => l !== "");
  const a = lines(pair.ts);
  const b = lines(pair.rust);
  if (a.join("\n") !== b.join("\n")) {
    report(`serve startup: ${JSON.stringify(a)} vs ${JSON.stringify(b)}`);
  }
  if (pair.ts.out.stderr.trim() !== pair.rust.out.stderr.trim()) {
    report(
      `serve startup stderr: ${JSON.stringify(pair.ts.out.stderr)} vs ${JSON.stringify(pair.rust.out.stderr)}`
    );
  }
};

/** `--remote` against the Rust server must print what TS prints locally. */
export const remoteCli = (
  pair: Pair,
  c: Counts,
  env: Record<string, string>,
  ids: readonly string[]
) => {
  const url = `http://127.0.0.1:${pair.rust.port}`;
  const cases: (readonly string[])[] = [
    ["sessions", "ls", "--limit", "0"],
    ["--json", "sessions", "ls", "--limit", "0"],
    ["sessions", "ls", "--agent", "codex"],
    ["sessions", "ls", "--cwd", "/tmp/demo", "--json"],
    ["sessions", "analyze", "--latest"],
    ["sessions", "analyze", "nope"],
    ["sessions", "export", "--latest"],
    ...ids.flatMap((id) => [
      ["sessions", "analyze", id],
      ["--json", "sessions", "analyze", id],
      ...["md", "html", "json"].map((f) => [
        "sessions",
        "export",
        id,
        "--format",
        f,
      ]),
      ["sessions", "export", id, "--format", "json", "--no-redact"],
    ]),
  ];
  // `exportedAt` is the clock of each run.
  const today = new Date().toISOString().slice(0, "2026-01-01".length);
  const stamp = new RegExp(`${today}T\\d\\d:\\d\\d:\\d\\d\\.\\d{3}Z`, "g");
  const mask = (r: ReturnType<typeof run>) => ({
    ...r,
    stdout: r.stdout.replace(stamp, "<now>"),
  });
  for (const args of cases) {
    c.cases += 1;
    const ts = run([...TS_CMD, "--no-telemetry", ...args], env, {
      canon: mask,
    });
    const rust = run([rustBin, "--remote", url, ...args], env);
    compareRun(`serve remote ${args.join(" ")}`, mask(ts), mask(rust));
  }
};

/** Transport failures of `--remote`: TS and Rust clients print the same. */
export const remoteErrors = (
  env: Record<string, string>,
  closedPort: number
) => {
  bind("<CLOSED>", `127.0.0.1:${closedPort}`);
  const urls = [`http://127.0.0.1:${closedPort}`, "", "ftp://x", "127.0.0.1:1"];
  for (const url of urls) {
    const args = ["sessions", "ls"];
    const ts = run(
      [...TS_CMD, "--no-telemetry", "--remote", url, ...args],
      env
    );
    const rust = run([rustBin, "--remote", url, ...args], env);
    // Bun before 1.4 names a refused connection `ConnectionRefused`.
    const tsErr = ts.stderr.replace("(ConnectionRefused)", "(ECONNREFUSED)");
    compareRun(
      `serve remote error ${JSON.stringify(url)}`,
      { ...ts, stderr: tsErr },
      rust
    );
  }
};

/** `--remote` to the token server: missing token, token URL, env token. */
export const remoteAuth = (
  pair: Pair,
  env: Record<string, string>,
  token: string
) => {
  const cases: [string, string, Record<string, string>][] = [
    ["no token", "", {}],
    ["token url", `/?token=${token}`, {}],
    ["env token", "", { PEEKTRACE_TOKEN: token }],
  ];
  for (const [label, suffix, extra] of cases) {
    const args = ["sessions", "ls", "--limit", "3"];
    const at = (s: Server) => `http://127.0.0.1:${s.port}${suffix}`;
    const ts = run(
      [...TS_CMD, "--no-telemetry", "--remote", at(pair.ts), ...args],
      { ...env, ...extra }
    );
    const rust = run([rustBin, "--remote", at(pair.rust), ...args], {
      ...env,
      ...extra,
    });
    const map = (s: string) =>
      s
        .replaceAll(String(pair.ts.port), "{P}")
        .replaceAll(String(pair.rust.port), "{P}");
    compareRun(
      `serve remote auth ${label}`,
      { ...ts, stderr: map(ts.stderr) },
      { ...rust, stderr: map(rust.stderr) }
    );
  }
};
