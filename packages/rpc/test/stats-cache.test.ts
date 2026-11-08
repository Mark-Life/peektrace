/** The shard cache across scoped and full `stats.*` calls.
 *
 * A temp corpus holds one Claude and one Codex transcript, both synthetic. A
 * pass narrowed to one agent must leave the other agent's shards warm, and a
 * full pass must drop the shard of a transcript that no longer exists. A
 * window that starts after every fact of a shard skips reading it, but keeps
 * its index entry and its shard file.
 */
import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { BunFileSystem } from "@effect/platform-bun";
import {
  AgentRegistry,
  type AgentRegistryShape,
  type AgentRoots,
} from "@workspace/core";
import { makeHandlersLayer, makeInProcessClient } from "@workspace/rpc";
import { Clock, Effect, Layer } from "effect";

const SLUG = "-work-demo";
const CLAUDE_SID = "33333333-3333-4333-8333-333333333333";
const CODEX_SID = "44444444-4444-4444-8444-444444444444";

const jsonl = (lines: readonly unknown[]) =>
  `${lines.map((line) => JSON.stringify(line)).join("\n")}\n`;

const CLAUDE = jsonl([
  {
    type: "user",
    cwd: "/work/demo",
    timestamp: "2026-06-01T10:00:00.000Z",
    message: { content: "list the files" },
  },
  {
    type: "assistant",
    requestId: "req-1",
    timestamp: "2026-06-01T10:00:05.000Z",
    message: {
      model: "claude-opus-4",
      usage: { input_tokens: 10, output_tokens: 5 },
      content: [{ type: "text", text: "done" }],
    },
  },
]);

const CODEX = jsonl([
  {
    timestamp: "2026-06-02T10:00:00.000Z",
    type: "session_meta",
    payload: {
      id: CODEX_SID,
      timestamp: "2026-06-02T10:00:00.000Z",
      cwd: "/work/demo",
      cli_version: "0.1.0",
      model_provider: "openai",
    },
  },
  {
    timestamp: "2026-06-02T10:00:01.000Z",
    type: "turn_context",
    payload: { turn_id: "turn-1", cwd: "/work/demo", model: "gpt-5" },
  },
  {
    timestamp: "2026-06-02T10:00:02.000Z",
    type: "response_item",
    payload: {
      type: "message",
      role: "user",
      content: [{ type: "input_text", text: "list the files" }],
    },
  },
  {
    timestamp: "2026-06-02T10:00:03.000Z",
    type: "response_item",
    payload: {
      type: "message",
      role: "assistant",
      content: [{ type: "output_text", text: "done" }],
    },
  },
]);

let root = "";

const claudePath = () => join(root, "claude", SLUG, `${CLAUDE_SID}.jsonl`);
const codexPath = () => join(root, "codex", `rollout-${CODEX_SID}.jsonl`);
const cacheDir = () => join(root, "pt", "stats");

/** Registry stub: both agents supported, each listing whatever is on disk. */
const makeAgents = (base: string): AgentRegistryShape => {
  const roots = (id: AgentRoots["id"]): AgentRoots => ({
    id,
    home: join(base, "home"),
    layout: "none",
    projectsRoot: join(base, id),
    supported: id === "claude" || id === "codex",
  });
  const listed = (id: AgentRoots["id"]) => {
    if (id === "claude" && existsSync(claudePath())) {
      return [{ path: claudePath(), id: CLAUDE_SID, slug: SLUG }];
    }
    if (id === "codex" && existsSync(codexPath())) {
      return [{ path: codexPath(), id: CODEX_SID, slug: "" }];
    }
    return [];
  };
  return {
    encodeSlug: (p) => p.replace(/[/.]/g, "-"),
    allowedRoots: [base, tmpdir()],
    roots,
    gitRoot: (cwd) => Effect.succeed(cwd),
    projectsRoot: () => Effect.succeed(base),
    sessionsGlob: () => Effect.succeed(join(base, "**", "*.jsonl")),
    memoryDir: ({ slug }) => Effect.succeed(join(base, slug, "memory")),
    listProjectSlugs: () => Effect.succeed([SLUG]),
    listSessionFiles: (id) => Effect.sync(() => listed(id)),
    loadTranscript: ({ ref }) =>
      Effect.sync(() => {
        const text = readFileSync(ref.path, "utf8");
        return {
          text,
          sizeBytes: Buffer.byteLength(text, "utf8"),
          mtimeMs: 1,
        };
      }),
  };
};

const FIXED_MS = Date.UTC(2026, 6, 1, 12, 0, 0);
const NANOS_PER_MS = 1_000_000n;

const fixedClock: Clock.Clock = {
  [Clock.ClockTypeId]: Clock.ClockTypeId,
  currentTimeMillis: Effect.succeed(FIXED_MS),
  currentTimeNanos: Effect.succeed(BigInt(FIXED_MS) * NANOS_PER_MS),
  unsafeCurrentTimeMillis: () => FIXED_MS,
  unsafeCurrentTimeNanos: () => BigInt(FIXED_MS) * NANOS_PER_MS,
  sleep: () => Effect.void,
};

/** Build the in-process client under the pinned clock and run `use`. */
const withClient = <A, E>(
  use: (
    client: Effect.Effect.Success<ReturnType<typeof makeInProcessClient>>
  ) => Effect.Effect<A, E, never>
): Promise<A> =>
  Effect.runPromise(
    Effect.gen(function* () {
      const client = yield* makeInProcessClient();
      return yield* use(client);
    }).pipe(
      Effect.scoped,
      Effect.provide(
        makeHandlersLayer({
          agents: Layer.succeed(AgentRegistry, makeAgents(root)),
          fileSystem: BunFileSystem.layer,
        })
      ),
      Effect.withClock(fixedClock)
    ) as Effect.Effect<A, E, never>
  );

const indexPaths = () => {
  const raw = readFileSync(join(cacheDir(), "index.json"), "utf8");
  return Object.keys(
    (JSON.parse(raw) as { entries: Record<string, unknown> }).entries
  ).sort();
};

const shardNames = () => readdirSync(join(cacheDir(), "shards")).sort();

const prevPtDir = process.env.PEEKTRACE_DIR;

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), "peektrace-stats-cache-"));
  mkdirSync(join(root, "claude", SLUG), { recursive: true });
  mkdirSync(join(root, "codex"), { recursive: true });
  mkdirSync(join(root, "home"), { recursive: true });
  writeFileSync(claudePath(), CLAUDE);
  writeFileSync(codexPath(), CODEX);
  process.env.PEEKTRACE_DIR = join(root, "pt");
});
afterEach(() => {
  if (prevPtDir === undefined) {
    delete process.env.PEEKTRACE_DIR;
  } else {
    process.env.PEEKTRACE_DIR = prevPtDir;
  }
  rmSync(root, { recursive: true, force: true });
});

describe("stats cache across scopes", () => {
  test("a scoped refresh keeps the other agent cached", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const first = yield* client.stats.refresh({});
        expect(first.transcripts).toBe(2);
        expect(first.extracted).toBe(2);
        const scoped = yield* client.stats.refresh({ agents: ["codex"] });
        expect(scoped.transcripts).toBe(1);
        expect(scoped.cached).toBe(1);
        const full = yield* client.stats.refresh({});
        expect(full.extracted).toBe(0);
        expect(full.cached).toBe(2);
      })
    ));

  test("a scoped report keeps the other agent cached", () =>
    withClient((client) =>
      Effect.gen(function* () {
        yield* client.stats.refresh({});
        yield* client.stats.report({ agents: ["codex"] });
        expect(indexPaths()).toEqual([claudePath(), codexPath()].sort());
        const full = yield* client.stats.refresh({});
        expect(full.extracted).toBe(0);
        expect(full.cached).toBe(2);
      })
    ));

  test("a scoped force re-extracts only its own agent", () =>
    withClient((client) =>
      Effect.gen(function* () {
        yield* client.stats.refresh({});
        const forced = yield* client.stats.refresh({
          agents: ["codex"],
          force: true,
        });
        expect(forced.extracted).toBe(1);
        const full = yield* client.stats.refresh({});
        expect(full.extracted).toBe(0);
        expect(full.cached).toBe(2);
      })
    ));

  test("a full pass drops the shard of a deleted transcript", () =>
    withClient((client) =>
      Effect.gen(function* () {
        yield* client.stats.refresh({});
        expect(shardNames()).toHaveLength(2);
        rmSync(claudePath());
        const full = yield* client.stats.refresh({});
        expect(full.transcripts).toBe(1);
        expect(full.cached).toBe(1);
        expect(indexPaths()).toEqual([codexPath()]);
        expect(shardNames()).toHaveLength(1);
      })
    ));
});

/** Overwrite every shard, so a pass that reads one has to re-extract it. */
const corruptShards = () => {
  for (const name of shardNames()) {
    writeFileSync(join(cacheDir(), "shards", name), "not json");
  }
};

const shardBodies = () =>
  shardNames().map((name) =>
    readFileSync(join(cacheDir(), "shards", name), "utf8")
  );

describe("stats window skip", () => {
  test("a narrow full report keeps every index entry and shard", () =>
    withClient((client) =>
      Effect.gen(function* () {
        yield* client.stats.refresh({});
        const narrow = yield* client.stats.report({ days: 1 });
        expect(narrow.corpus.transcripts).toBe(0);
        expect(indexPaths()).toEqual([claudePath(), codexPath()].sort());
        expect(shardNames()).toHaveLength(2);
        const wide = yield* client.stats.refresh({});
        expect(wide.extracted).toBe(0);
        expect(wide.cached).toBe(2);
        expect(wide.facts).toBeGreaterThan(0);
      })
    ));

  test("a narrow scoped report keeps every index entry", () =>
    withClient((client) =>
      Effect.gen(function* () {
        yield* client.stats.refresh({});
        yield* client.stats.report({ agents: ["claude"], days: 1 });
        expect(indexPaths()).toEqual([claudePath(), codexPath()].sort());
        const wide = yield* client.stats.refresh({});
        expect(wide.extracted).toBe(0);
        expect(wide.cached).toBe(2);
      })
    ));

  test("a shard with nothing in the window is not read", () =>
    withClient((client) =>
      Effect.gen(function* () {
        yield* client.stats.refresh({});
        corruptShards();
        // A read re-extracts and rewrites a corrupt shard (see "corrupt shard"),
        // so unchanged bodies mean no read.
        const narrow = yield* client.stats.report({ days: 1 });
        expect(narrow.corpus.transcripts).toBe(0);
        expect(shardBodies()).toEqual(["not json", "not json"]);
        expect(indexPaths()).toEqual([claudePath(), codexPath()].sort());
      })
    ));

  test("a cold narrow report still writes shards a wider run reads", () =>
    withClient((client) =>
      Effect.gen(function* () {
        yield* client.stats.report({ days: 1 });
        expect(indexPaths()).toEqual([claudePath(), codexPath()].sort());
        const wide = yield* client.stats.refresh({});
        expect(wide.extracted).toBe(0);
        expect(wide.cached).toBe(2);
      })
    ));

  test("cold and warm reports match, inside and outside the window", () =>
    withClient((client) =>
      Effect.gen(function* () {
        for (const days of [1, 60]) {
          const cold = yield* client.stats.report({ days, force: true });
          const warm = yield* client.stats.report({ days });
          expect(warm).toEqual(cold);
        }
      })
    ));
});

describe("corrupt shard", () => {
  test("a corrupt shard is re-extracted and rewritten, not a failed pass", () =>
    withClient((client) =>
      Effect.gen(function* () {
        yield* client.stats.refresh({});
        corruptShards();
        const wide = yield* client.stats.refresh({});
        expect(wide.extracted).toBe(2);
        expect(shardBodies()).not.toContain("not json");
        const again = yield* client.stats.refresh({});
        expect(again.cached).toBe(2);
      })
    ));
});
