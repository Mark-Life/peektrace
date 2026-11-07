import { afterEach, describe, expect, test } from "bun:test";
import {
  appendFileSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { BunFileSystem } from "@effect/platform-bun";
import { Effect, Layer } from "effect";
import type { AgentId } from "../../src/services/agent-id";
import {
  AgentRegistry,
  type AgentRegistryShape,
  type SessionFileRef,
  TranscriptReadError,
} from "../../src/services/agents";
import {
  SessionsService,
  SessionsServiceLive,
} from "../../src/services/sessions/service";

const line = (sessionId: string, i: number) =>
  JSON.stringify({
    type: "user",
    uuid: `u-${i}`,
    sessionId,
    cwd: "/work/demo",
    timestamp: new Date(1_700_000_000_000 + i * 1000).toISOString(),
    message: { role: "user", content: `hello ${i}` },
  });

const sessionId = (c: string) => `${c.repeat(8)}-1111-4111-8111-111111111111`;

const writeSession = (root: string, id: string) => {
  const path = join(root, `${id}.jsonl`);
  writeFileSync(path, `${line(id, 0)}\n`);
  return { path, id, slug: "-work-demo" } satisfies SessionFileRef;
};

const roots: string[] = [];

afterEach(() => {
  for (const root of roots.splice(0)) {
    rmSync(root, { recursive: true, force: true });
  }
});

const makeCorpus = (letters: readonly string[] = ["a", "b", "c"]) => {
  const root = mkdtempSync(join(tmpdir(), "pt-list-cache-"));
  roots.push(root);
  return {
    root,
    refs: letters.map((c) => writeSession(root, sessionId(c))),
  };
};

interface StubOptions {
  /** Ref paths whose next load fails once. */
  readonly failOnce?: Set<string>;
  /** Agents given the OpenCode (SQLite) layout. */
  readonly opencode?: readonly AgentId[];
  /** Refs per agent, read live so a test can add and remove files. */
  readonly refs: () => Partial<Record<AgentId, readonly SessionFileRef[]>>;
}

/** Registry stub over real files; counts transcript loads per agent. */
const makeStub = (opts: StubOptions) => {
  const loads: Partial<Record<AgentId, number>> = {};
  const stamped: { ref: string; stamped: boolean }[] = [];
  const listed = (id: AgentId) => opts.refs()[id];
  const stub = {
    listSessionFiles: (id) => Effect.succeed(listed(id) ?? []),
    loadTranscript: ({ agent, ref, stamp }) =>
      Effect.suspend(() => {
        loads[agent] = (loads[agent] ?? 0) + 1;
        stamped.push({ ref: ref.path, stamped: stamp !== undefined });
        if (opts.failOnce?.delete(ref.path)) {
          return Effect.fail(
            new TranscriptReadError({ path: ref.path, reason: "boom" })
          );
        }
        const st = statSync(ref.path);
        const text = readFileSync(ref.path, "utf8");
        // Whole ms, as the real loader reads mtime through a Date.
        return Effect.succeed({
          text,
          sizeBytes: st.size,
          mtimeMs: Math.trunc(st.mtimeMs),
        });
      }),
    roots: (id) => ({
      id,
      home: "/",
      layout: opts.opencode?.includes(id)
        ? "opencode-sqlite"
        : "claude-projects",
      projectsRoot: "/",
      supported: listed(id) !== undefined,
    }),
  } as Partial<AgentRegistryShape> as AgentRegistryShape;
  const layer = () =>
    SessionsServiceLive.pipe(
      Layer.provide(Layer.succeed(AgentRegistry, stub)),
      Layer.provide(BunFileSystem.layer)
    );
  const total = () => Object.values(loads).reduce((a, b) => a + b, 0);
  return { loads, layer, stamped, total };
};

/** A fresh service has an empty cache, so its list is the uncached answer. */
const uncachedList = (layer: ReturnType<typeof makeStub>["layer"]) =>
  Effect.runPromise(
    Effect.flatMap(SessionsService, (s) => s.list()).pipe(
      Effect.provide(layer())
    )
  );

describe("sessions.list header cache", () => {
  test("second list over unchanged files reads no transcripts", async () => {
    const { refs } = makeCorpus();
    const { layer, total } = makeStub({ refs: () => ({ claude: refs }) });
    const [first, second, loadsSecond] = await Effect.runPromise(
      Effect.gen(function* () {
        const s = yield* SessionsService;
        const a = yield* s.list();
        const before = total();
        const b = yield* s.list();
        return [a, b, total() - before] as const;
      }).pipe(Effect.provide(layer()))
    );
    expect(second).toEqual(first);
    expect(first.length).toBe(3);
    expect(loadsSecond).toBe(0);
  });

  test("cached list equals an uncached list across append, delete, add, touch", async () => {
    const corpus = makeCorpus();
    let refs: SessionFileRef[] = [...corpus.refs];
    const [a, b, c] = corpus.refs;
    if (!(a && b && c)) {
      throw new Error("corpus");
    }
    const { layer, total } = makeStub({ refs: () => ({ claude: refs }) });
    const steps: readonly {
      readonly name: string;
      readonly run: () => void;
    }[] = [
      { name: "baseline", run: () => undefined },
      {
        name: "append",
        run: () => appendFileSync(a.path, `${line(a.id, 1)}\n`),
      },
      {
        name: "delete",
        run: () => {
          rmSync(b.path);
          refs = refs.filter((r) => r !== b);
        },
      },
      {
        name: "add",
        run: () => {
          refs = [...refs, writeSession(corpus.root, sessionId("d"))];
        },
      },
      {
        name: "touch",
        run: () =>
          utimesSync(
            c.path,
            new Date(2_000_000_000_000),
            new Date(2_000_000_000_000)
          ),
      },
    ];
    const results = await Effect.runPromise(
      Effect.gen(function* () {
        const s = yield* SessionsService;
        const out: {
          name: string;
          cached: unknown;
          uncached: unknown;
          loads: number;
          length: number;
        }[] = [];
        for (const step of steps) {
          step.run();
          const before = total();
          const cached = yield* s.list();
          const loads = total() - before;
          const uncached = yield* Effect.promise(() => uncachedList(layer));
          out.push({
            name: step.name,
            cached,
            uncached,
            loads,
            length: cached.length,
          });
        }
        return out;
      }).pipe(Effect.provide(layer()))
    );
    for (const r of results) {
      expect(r.cached).toEqual(r.uncached);
    }
    expect(results.map((r) => [r.name, r.loads, r.length])).toEqual([
      ["baseline", 3, 3],
      ["append", 1, 3],
      ["delete", 0, 2],
      ["add", 1, 3],
      ["touch", 1, 3],
    ]);
  });

  test("agent filter reads only that agent's transcripts", async () => {
    const claude = makeCorpus(["a", "b"]).refs;
    const codex = makeCorpus(["c"]).refs;
    const { layer, loads } = makeStub({ refs: () => ({ claude, codex }) });
    await Effect.runPromise(
      Effect.flatMap(SessionsService, (s) => s.list({ agent: "codex" })).pipe(
        Effect.provide(layer())
      )
    );
    expect(loads.claude ?? 0).toBe(0);
    expect(loads.codex).toBe(1);
  });

  test("a failed load is not cached", async () => {
    const { refs } = makeCorpus(["a", "b"]);
    const [a] = refs;
    if (!a) {
      throw new Error("corpus");
    }
    const { layer } = makeStub({
      refs: () => ({ claude: refs }),
      failOnce: new Set([a.path]),
    });
    const [first, second] = await Effect.runPromise(
      Effect.gen(function* () {
        const s = yield* SessionsService;
        return [yield* s.list(), yield* s.list()] as const;
      }).pipe(Effect.provide(layer()))
    );
    expect(first.map((h) => h.id)).not.toContain(a.id);
    expect(second.map((h) => h.id)).toContain(a.id);
    expect(second).toHaveLength(2);
  });

  test("OpenCode-layout sessions are read on every list", async () => {
    const { refs } = makeCorpus(["a", "b"]);
    const { layer, loads } = makeStub({
      refs: () => ({ opencode: refs }),
      opencode: ["opencode"],
    });
    await Effect.runPromise(
      Effect.gen(function* () {
        const s = yield* SessionsService;
        yield* s.list();
        yield* s.list();
      }).pipe(Effect.provide(layer()))
    );
    expect(loads.opencode).toBe(4);
  });

  test("a cache miss hands its pre-read stamp to the loader", async () => {
    const { refs } = makeCorpus(["a", "b"]);
    const { layer, stamped } = makeStub({ refs: () => ({ claude: refs }) });
    await Effect.runPromise(
      Effect.flatMap(SessionsService, (s) => s.list()).pipe(
        Effect.provide(layer())
      )
    );
    expect(stamped).toHaveLength(2);
    expect(stamped.every((l) => l.stamped)).toBe(true);
  });
});
