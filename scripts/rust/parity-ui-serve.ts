/** `serve --ui rune` static answers (Rust only: TS has no such flag), on a
 * fake bundle: types, brotli twins, ETags, the page and its fallback, the
 * 404 for twins asked by name, `/api` untouched, and the 503 when the
 * bundle is missing. */
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fixtureEnv, report, WORK } from "./parity-lib";
import { freePort, startServer, stopServer } from "./parity-serve-lib";

interface Expect {
  readonly body?: string;
  readonly headers?: Record<string, string | null>;
  readonly path: string;
  readonly req?: Record<string, string>;
  readonly status: number;
}

const bundle = () => {
  const dir = join(WORK, "rune-dist");
  mkdirSync(join(dir, "snippets"), { recursive: true });
  writeFileSync(join(dir, "index.html"), "<p>rune</p>");
  writeFileSync(join(dir, "app.js"), "export default () => {};");
  writeFileSync(join(dir, "app_bg.wasm"), "\0asm-plain");
  writeFileSync(join(dir, "app_bg.wasm.br"), "br-bytes");
  writeFileSync(join(dir, "snippets", "x.js"), "export const x = 1;");
  return dir;
};

const CASES: readonly Expect[] = [
  {
    path: "/",
    status: 200,
    headers: {
      "content-type": "text/html;charset=utf-8",
      "cache-control": "no-store",
    },
    body: "<p>rune</p>",
  },
  {
    path: "/sessions/abc",
    status: 200,
    headers: { "content-type": "text/html;charset=utf-8" },
    body: "<p>rune</p>",
  },
  {
    path: "/app.js",
    status: 200,
    headers: {
      "content-type": "text/javascript;charset=utf-8",
      "cache-control": "no-cache",
      vary: "accept-encoding",
    },
  },
  {
    path: "/app_bg.wasm",
    req: { "accept-encoding": "identity" },
    status: 200,
    headers: { "content-type": "application/wasm", "content-encoding": null },
    body: "\0asm-plain",
  },
  {
    path: "/app_bg.wasm",
    req: { "accept-encoding": "gzip, br" },
    status: 200,
    headers: {
      "content-type": "application/wasm",
      "content-encoding": "br",
      vary: "accept-encoding",
    },
    body: "br-bytes",
  },
  { path: "/app_bg.wasm.br", status: 404 },
  {
    path: "/snippets/x.js",
    status: 200,
    headers: { "content-type": "text/javascript;charset=utf-8" },
  },
];

const fetchRaw = (
  port: number,
  path: string,
  headers: Record<string, string> = {}
) =>
  fetch(`http://127.0.0.1:${port}${path}`, {
    headers,
    decompress: false,
  } as RequestInit);

const checkCase = async (port: number, c: Expect) => {
  const res = await fetchRaw(port, c.path, c.req);
  const label = `ui serve ${c.path} ${JSON.stringify(c.req ?? {})}`;
  if (res.status !== c.status) {
    report(`${label}: status ${res.status}, want ${c.status}`);
  }
  for (const [k, v] of Object.entries(c.headers ?? {})) {
    if (res.headers.get(k) !== v) {
      report(`${label}: ${k}=${res.headers.get(k)}, want ${v}`);
    }
  }
  const body = Buffer.from(await res.arrayBuffer()).toString("latin1");
  if (c.body !== undefined && body !== c.body) {
    report(`${label}: body ${JSON.stringify(body.slice(0, 40))}`);
  }
};

/** Number of checks run. */
export const runRuneServeChecks = async () => {
  const home = join(WORK, "rune-home");
  mkdirSync(home, { recursive: true });
  let checks = 0;
  const port = await freePort();
  const server = await startServer(
    "rust",
    ["--ui", "rune", "--port", String(port)],
    {
      ...fixtureEnv(home),
      PEEKTRACE_RUNE_DIR: bundle(),
    }
  );
  try {
    for (const c of CASES) {
      checks++;
      await checkCase(server.port, c);
    }
    const first = await fetchRaw(server.port, "/app_bg.wasm", {
      "accept-encoding": "br",
    });
    const etag = first.headers.get("etag") ?? "";
    const again = await fetchRaw(server.port, "/app_bg.wasm", {
      "accept-encoding": "br",
      "if-none-match": etag,
    });
    checks++;
    if (!etag.endsWith('-br"') || again.status !== 304) {
      report(`ui serve etag: ${etag} -> ${again.status}`);
    }
    const head = await fetch(`http://127.0.0.1:${server.port}/app.js`, {
      method: "HEAD",
    });
    checks++;
    if (head.status !== 200 || (await head.text()) !== "") {
      report(`ui serve HEAD /app.js: ${head.status}`);
    }
    const api = await fetchRaw(server.port, "/api/sessions");
    checks++;
    if (
      api.status !== 200 ||
      !(api.headers.get("content-type") ?? "").startsWith("application/json")
    ) {
      report(
        `ui serve /api/sessions: ${api.status} ${api.headers.get("content-type")}`
      );
    }
  } finally {
    await stopServer(server);
  }
  const emptyPort = await freePort();
  const empty = await startServer(
    "rust",
    ["--ui", "rune", "--port", String(emptyPort)],
    {
      ...fixtureEnv(home),
      PEEKTRACE_RUNE_DIR: join(WORK, "rune-missing"),
    }
  );
  try {
    const res = await fetchRaw(empty.port, "/");
    checks++;
    const text = await res.text();
    if (res.status !== 503 || !text.startsWith("Rune UI not built")) {
      report(`ui serve missing bundle: ${res.status} ${text.slice(0, 60)}`);
    }
  } finally {
    await stopServer(empty);
  }
  return checks;
};
