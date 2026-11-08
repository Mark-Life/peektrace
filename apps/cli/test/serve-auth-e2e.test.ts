/** Spawned `peektrace serve --host 0.0.0.0` checks: token printing, auth, and
 * the read-only default. Uses a synthetic HOME and memory vault. */
import {
  afterAll,
  beforeAll,
  describe,
  expect,
  setDefaultTimeout,
  test,
} from "bun:test";
import { existsSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createPeektraceClient } from "@workspace/rpc";
import { Effect } from "effect";
import { findFreePort } from "../src/serve-core";

const ENTRY = join(import.meta.dirname, "../src/index.ts");
const PROJECT = "-synthetic-proj";
const URL_LINE = /Open:\s+(http:\/\/\S+)/;
const LOOPBACK_LINE = /serving on (http:\/\/127\.0\.0\.1:\d+)/;
const LONG_TOKEN = "k".repeat(40);

setDefaultTimeout(45_000);

let home = "";
const memoryDir = () => join(home, ".claude", "projects", PROJECT, "memory");

interface Server {
  readonly base: string;
  readonly kill: () => void;
  readonly token: string;
}

const env = (extra: Record<string, string> = {}) => ({
  PATH: process.env.PATH ?? "",
  HOME: home,
  CLAUDE_CONFIG_DIR: join(home, ".claude"),
  CODEX_HOME: join(home, ".codex"),
  XDG_DATA_HOME: join(home, ".local", "share"),
  PEEKTRACE_DIR: join(home, ".peektrace"),
  PEEKTRACE_NO_TELEMETRY: "1",
  PEEKTRACE_NO_UPDATE_CHECK: "1",
  ...extra,
});

const serveArgs = (port: number, host: string) => [
  "bun",
  ENTRY,
  "serve",
  "--host",
  host,
  "--port",
  String(port),
  "--no-open",
];

/**
 * Drain `proc.stdout` for the life of the process and resolve with the first
 * match of `re`. Draining keeps serve's later writes off a closed pipe. Rejects
 * with the captured output if the stream ends first (serve exited).
 */
const waitForLine = (
  proc: Bun.Subprocess<"ignore", "pipe", "ignore">,
  re: RegExp
) =>
  new Promise<RegExpExecArray>((resolve, reject) => {
    const decoder = new TextDecoder();
    let out = "";
    let matched = false;
    (async () => {
      for await (const chunk of proc.stdout) {
        out += decoder.decode(chunk, { stream: true });
        const match = matched ? null : re.exec(out);
        if (match !== null) {
          matched = true;
          resolve(match);
        }
      }
      if (!matched) {
        reject(new Error(`serve exited before printing ${re}:\n${out}`));
      }
    })().catch(reject);
  });

/** Start `serve` on all interfaces and wait for its printed token URL. */
const startServe = async (
  flags: readonly string[],
  extra: Record<string, string> = {}
): Promise<Server> => {
  // Serve picks its own free port from 42000; the printed URL carries it.
  const proc = Bun.spawn([...serveArgs(42_000, "0.0.0.0"), ...flags], {
    env: env(extra),
    stdout: "pipe",
    stderr: "ignore",
  });
  const [, href = ""] = await waitForLine(proc, URL_LINE).catch((e) => {
    proc.kill();
    throw e;
  });
  const url = new URL(href);
  return {
    base: url.origin,
    kill: () => proc.kill(),
    token: url.searchParams.get("token") ?? "",
  };
};

const createMemory = (server: Server) =>
  Effect.gen(function* () {
    const client = yield* createPeektraceClient(server.base, {
      token: server.token,
    });
    return yield* client.memory.create({
      project: PROJECT,
      name: "gamma",
      description: "Synthetic",
      type: "reference",
      body: "Body.\n",
    });
  }).pipe(Effect.scoped);

const withServer = async (
  flags: readonly string[],
  use: (server: Server) => Promise<void> | void,
  extra: Record<string, string> = {}
) => {
  const server = await startServe(flags, extra);
  try {
    await use(server);
  } finally {
    server.kill();
  }
};

beforeAll(() => {
  home = mkdtempSync(join(tmpdir(), "peektrace-serve-auth-"));
  mkdirSync(memoryDir(), { recursive: true });
});

afterAll(() => {
  rmSync(home, { recursive: true, force: true });
});

describe("serve on a non-loopback host", () => {
  test("requires the token and refuses writes by default", () =>
    withServer([], async (server) => {
      expect(server.token.length).toBe(43);
      expect((await fetch(`${server.base}/health`)).status).toBe(401);
      const ok = await fetch(`${server.base}/health`, {
        headers: { authorization: `Bearer ${server.token}` },
      });
      expect(ok.status).toBe(200);
      const failure = await Effect.runPromise(
        createMemory(server).pipe(Effect.flip)
      );
      expect(JSON.stringify(failure)).toContain("read-only");
      expect(existsSync(join(memoryDir(), "gamma.md"))).toBe(false);
    }));

  test("--allow-write permits memory.create", () =>
    withServer(["--allow-write"], async (server) => {
      const entry = await Effect.runPromise(createMemory(server));
      expect(entry.slug).toBe("gamma");
      expect(existsSync(join(memoryDir(), "gamma.md"))).toBe(true);
      rmSync(join(memoryDir(), "gamma.md"), { force: true });
    }));

  test("--read-only wins over --allow-write", () =>
    withServer(["--allow-write", "--read-only"], async (server) => {
      const failure = await Effect.runPromise(
        createMemory(server).pipe(Effect.flip)
      );
      expect(JSON.stringify(failure)).toContain("read-only");
    }));

  test("PEEKTRACE_TOKEN supplies the token", () =>
    withServer(
      [],
      (server) => {
        expect(server.token).toBe(LONG_TOKEN);
      },
      { PEEKTRACE_TOKEN: LONG_TOKEN }
    ));
});

describe("serve token handling", () => {
  test("a short PEEKTRACE_TOKEN fails before binding", async () => {
    const port = await Effect.runPromise(findFreePort(43_000, "0.0.0.0"));
    const proc = Bun.spawn(serveArgs(port, "0.0.0.0"), {
      env: env({ PEEKTRACE_TOKEN: "short" }),
      stdout: "pipe",
      stderr: "pipe",
    });
    const [err, code] = await Promise.all([
      new Response(proc.stderr).text(),
      proc.exited,
    ]);
    expect(code).not.toBe(0);
    expect(err).toContain("PEEKTRACE_TOKEN");
    await expect(
      fetch(`http://127.0.0.1:${port}/health`)
    ).rejects.toBeDefined();
  });

  test("loopback ignores PEEKTRACE_TOKEN and needs no credentials", async () => {
    const proc = Bun.spawn(serveArgs(43_100, "127.0.0.1"), {
      env: env({ PEEKTRACE_TOKEN: LONG_TOKEN }),
      stdout: "pipe",
      stderr: "ignore",
    });
    try {
      // Serve picks its own free port from the start one, so use the one it prints.
      const [, base] = await waitForLine(proc, LOOPBACK_LINE);
      expect((await fetch(`${base}/health`)).status).toBe(200);
    } finally {
      proc.kill();
    }
  });
});

describe("--remote against an authed server", () => {
  const remote = (args: readonly string[], extra: Record<string, string>) => {
    const proc = Bun.spawnSync(["bun", ENTRY, ...args], { env: env(extra) });
    return { code: proc.exitCode, err: proc.stderr.toString() };
  };

  test("token URL, env token, and the missing-token message", () =>
    withServer([], (server) => {
      const ls = ["--json", "sessions", "ls"];
      const viaUrl = remote(
        ["--remote", `${server.base}/?token=${server.token}`, ...ls],
        {}
      );
      expect(viaUrl.code).toBe(0);
      const viaEnv = remote(["--remote", server.base, ...ls], {
        PEEKTRACE_TOKEN: server.token,
      });
      expect(viaEnv.code).toBe(0);
      const none = remote(["--remote", server.base, ...ls], {});
      expect(none.code).toBe(1);
      expect(none.err).toContain("HTTP 401");
      expect(none.err).toContain("PEEKTRACE_TOKEN");
      expect(none.err).not.toContain(server.token);
    }));
});
