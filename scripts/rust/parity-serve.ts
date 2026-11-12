/** Serve stage: the TS server (`/rpc`) and the Rust server (`/api`) run
 * side by side on the same data and UI, and every equivalent call, guard,
 * static path and `--remote` command is compared. */
import {
  cpSync,
  existsSync,
  mkdirSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { createServer } from "node:net";
import { join } from "node:path";
import { buildPiRoot } from "./parity-agents-fixtures";
import { fixtureEnv, newestAndLargest, REPO, run, WORK } from "./parity-lib";
import { buildOpencodeData } from "./parity-opencode-fixtures";
import {
  type Counts,
  imageCases,
  infoCases,
  type Pair,
  readOnlyCases,
  realCases,
  sessionCases,
  settingsCases,
  watchCases,
} from "./parity-serve-cases";
import {
  authHttp,
  loopbackHttp,
  remoteAuth,
  remoteCli,
  remoteErrors,
  startupLines,
  staticHttp,
} from "./parity-serve-http";
import { startServer, stopServer } from "./parity-serve-lib";

const DIST = join(REPO, "apps/inspector/dist");
const TOKEN = "parity-token-0123456789abcdefghijklmn";
const PNG =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

/** A free port far from the others in use. */
const freePort = () =>
  new Promise<number>((done) => {
    const srv = createServer();
    srv.listen(0, "127.0.0.1", () => {
      const address = srv.address();
      const port = typeof address === "object" && address ? address.port : 0;
      srv.close(() => done(port));
    });
  });

/** The UI both servers serve: a copy of the built dist, with a file beside
 * it that no traversal may reach. */
const buildUi = (root: string) => {
  if (!existsSync(join(DIST, "index.html"))) {
    run(["bun", "run", "--filter=inspector", "build"], {
      PATH: process.env.PATH ?? "",
      HOME: process.env.HOME ?? "",
    });
  }
  const dist = join(root, "dist");
  cpSync(DIST, dist, { recursive: true });
  writeFileSync(join(root, "secret.txt"), "secret\n");
  return dist;
};

/** A Claude transcript with an image block and a tool result image. */
const imageTranscript = (projects: string) => {
  const id = "img-0000-4000-8000-000000000001";
  const dir = join(projects, "-tmp-img");
  mkdirSync(dir, { recursive: true });
  const source = { type: "base64", media_type: "image/png", data: PNG };
  const at = (n: number) => `2026-06-02T10:00:0${n}.000Z`;
  const base = { sessionId: id, cwd: "/tmp/img", version: "2.0.0" };
  const lines = [
    {
      ...base,
      type: "user",
      uuid: "u1",
      timestamp: at(0),
      message: {
        role: "user",
        content: [
          { type: "text", text: "look" },
          { type: "image", source },
        ],
      },
    },
    {
      ...base,
      type: "assistant",
      uuid: "a1",
      parentUuid: "u1",
      timestamp: at(1),
      message: {
        id: "m1",
        role: "assistant",
        model: "claude-sonnet-4-5",
        content: [
          {
            type: "tool_use",
            id: "t1",
            name: "Read",
            input: { file_path: "/tmp/img/a.png" },
          },
        ],
        usage: { input_tokens: 10, output_tokens: 5 },
      },
    },
    {
      ...base,
      type: "user",
      uuid: "u2",
      parentUuid: "a1",
      timestamp: at(2),
      message: {
        role: "user",
        content: [
          {
            type: "tool_result",
            tool_use_id: "t1",
            content: [{ type: "image", source }],
          },
        ],
      },
    },
  ];
  writeFileSync(
    join(dir, `${id}.jsonl`),
    `${lines.map((l) => JSON.stringify(l)).join("\n")}\n`
  );
  return { id, path: join(dir, `${id}.jsonl`) };
};

const stampOf = (path: string) => {
  try {
    const s = statSync(path);
    return `${s.size}:${s.mtimeMs}`;
  } catch {
    return "gone";
  }
};

/** Start a TS and a Rust server with the same flags; each gets its own
 * `PEEKTRACE_DIR`. */
const startPair = async (
  name: string,
  env: Record<string, string>,
  flags: readonly string[]
): Promise<Pair> => {
  const tsDir = join(WORK, "serve", name, "ts-dir");
  const rustDir = join(WORK, "serve", name, "rust-dir");
  mkdirSync(tsDir, { recursive: true });
  mkdirSync(rustDir, { recursive: true });
  const ts = await startServer(
    "ts",
    ["--port", String(await freePort()), ...flags],
    {
      ...env,
      PEEKTRACE_DIR: tsDir,
    }
  );
  try {
    const rust = await startServer(
      "rust",
      ["--port", String(await freePort()), ...flags],
      {
        ...env,
        PEEKTRACE_DIR: rustDir,
      }
    );
    return { rust, rustDir, ts, tsDir };
  } catch (e) {
    await stopServer(ts);
    throw e;
  }
};

const stopPair = async (p: Pair | undefined) => {
  await stopServer(p?.ts);
  await stopServer(p?.rust);
};

const withPair = async (
  name: string,
  env: Record<string, string>,
  flags: readonly string[],
  use: (p: Pair) => Promise<void>
) => {
  const pair = await startPair(name, env, flags);
  try {
    await use(pair);
  } finally {
    await stopPair(pair);
  }
};

const fixtureStage = async (c: Counts) => {
  const root = join(WORK, "serve");
  const fixtures = join(REPO, "packages/core/test/fixtures");
  const claude = join(root, "claude");
  cpSync(join(fixtures, "sessions/projects"), claude, { recursive: true });
  const image = imageTranscript(claude);
  const oc = buildOpencodeData(
    join(root, "oc"),
    join(fixtures, "sessions/projects")
  );
  const dist = buildUi(join(root, "ui"));
  const home = join(root, "home");
  mkdirSync(home, { recursive: true });
  const env = fixtureEnv(home, {
    PEEKTRACE_CLAUDE_PROJECTS: claude,
    PEEKTRACE_CODEX_SESSIONS: join(fixtures, "sessions-codex"),
    PEEKTRACE_PI_SESSIONS: buildPiRoot(fixtures, join(root, "pi-root")),
    PEEKTRACE_OPENCODE_DATA: oc.main,
    PEEKTRACE_CLIENT_DIR: dist,
  });
  try {
    await withPair("fixtures", env, [], async (pair) => {
      startupLines(pair, c);
      await infoCases(pair, c);
      await sessionCases(pair, c);
      await imageCases(pair, c, image.id);
      await settingsCases(pair, c);
      await loopbackHttp(pair, c);
      await staticHttp(pair, c, dist);
      const ids = run(
        [
          ...["bun", join(REPO, "apps/cli/src/index.ts")],
          "--json",
          "sessions",
          "ls",
          "--limit",
          "0",
        ],
        env
      );
      const rows = JSON.parse(ids.stdout || "[]") as { id: string }[];
      remoteCli(
        pair,
        c,
        env,
        rows.map((r) => r.id)
      );
      remoteErrors(env, await freePort());
      const transcript = join(
        claude,
        "-Users-demo-proj",
        "11111111-1111-4111-8111-111111111111.jsonl"
      );
      await watchCases(
        pair,
        c,
        existsSync(transcript) ? transcript : image.path
      );
    });
    await withPair("read-only", env, ["--read-only"], (pair) =>
      readOnlyCases(pair, c)
    );
    await withPair(
      "auth",
      { ...env, PEEKTRACE_TOKEN: TOKEN },
      ["--host", "0.0.0.0"],
      async (pair) => {
        startupLines(pair, c);
        await authHttp(pair, c, TOKEN);
        remoteAuth(pair, env, TOKEN);
      }
    );
  } finally {
    oc.close();
  }
};

const REAL_NEWEST = 5;
const REAL_LARGEST = 3;

const realStage = async (c: Counts, realAll: boolean) => {
  const env = {
    PATH: process.env.PATH ?? "",
    HOME: process.env.HOME ?? "",
    PEEKTRACE_NO_TELEMETRY: "1",
    PEEKTRACE_CLIENT_DIR: buildUi(join(WORK, "serve", "real-ui")),
  };
  await withPair("real", env, ["--read-only"], (pair) =>
    realCases(
      pair,
      c,
      (rows) =>
        realAll ? rows : newestAndLargest(rows, REAL_NEWEST, REAL_LARGEST),
      stampOf
    )
  );
};

export const runServeStage = async (args: {
  readonly fixtures: boolean;
  readonly real: boolean;
  readonly realAll: boolean;
}) => {
  const c: Counts = { cases: 0, live: 0 };
  if (args.fixtures) {
    await fixtureStage(c);
  }
  if (args.real) {
    await realStage(c, args.realAll);
  }
  console.log(`serve stage: ${c.cases} cases, ${c.live} live`);
};
