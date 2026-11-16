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
import { join } from "node:path";
import { buildPiRoot } from "./parity-agents-fixtures";
import {
  COPY,
  fixtureEnv,
  fixturesDir,
  newestAndLargest,
  REPO,
  run,
  WORK,
} from "./parity-lib";
import { buildOpencodeData } from "./parity-opencode-fixtures";
import {
  type Counts,
  imageCases,
  infoCases,
  readOnlyCases,
  realCases,
  sessionCases,
  settingsCases,
  watchCases,
} from "./parity-serve-cases";
import { fixtureFeatures, realFeatures } from "./parity-serve-features";
import {
  authHttp,
  loopbackHttp,
  remoteAuth,
  remoteCli,
  remoteErrors,
  startupLines,
  staticHttp,
} from "./parity-serve-http";
import { freePort, withPair } from "./parity-serve-lib";
import { remoteStatusline } from "./parity-statusline";

const DIST = join(REPO, "apps/inspector/dist");
const TOKEN = "parity-token-0123456789abcdefghijklmn";
const PNG =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

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

const fixtureStage = async (c: Counts) => {
  const root = join(WORK, "serve");
  const fixtures = fixturesDir();
  const claude = join(root, "claude");
  cpSync(join(fixtures, "sessions/projects"), claude, COPY);
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
      const rows = JSON.parse(ids.stdout || "[]") as {
        id: string;
        path: string;
      }[];
      remoteCli(
        pair,
        c,
        env,
        rows.map((r) => r.id)
      );
      c.cases += remoteStatusline(
        { rust: pair.rust.port, ts: pair.ts.port },
        env,
        rows
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
  await fixtureFeatures(c, dist);
};

const REAL_NEWEST = 5;
const REAL_LARGEST = 3;

const realStage = async (c: Counts, realAll: boolean, realStats: boolean) => {
  const dist = buildUi(join(WORK, "serve", "real-ui"));
  const env = {
    PATH: process.env.PATH ?? "",
    HOME: process.env.HOME ?? "",
    PEEKTRACE_NO_TELEMETRY: "1",
    PEEKTRACE_CLIENT_DIR: dist,
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
  await realFeatures(c, dist, realStats);
};

export const runServeStage = async (args: {
  readonly fixtures: boolean;
  readonly real: boolean;
  readonly realAll: boolean;
  /** Run the real-corpus stats cases (slow: a cold pass on each side). */
  readonly realStats: boolean;
}) => {
  const c: Counts = { cases: 0, features: 0, live: 0 };
  if (args.fixtures) {
    await fixtureStage(c);
  }
  if (args.real) {
    await realStage(c, args.realAll, args.realStats);
  }
  console.log(
    `serve stage: ${c.cases} cases, ${c.live} live, features: ${c.features}`
  );
};
