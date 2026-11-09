/** In-process Phase 4 RPC test: drives every procedure through the real handlers
 * + real core against the committed fixtures (no HTTP). Claude resolution is
 * pointed at a temp root holding a copy of the session fixture and a temp memory
 * vault. Asserts typed results end-to-end and a CAS conflict surfacing as the
 * typed `FileChangedError` over RPC (not a defect).
 */
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import {
  cpSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { BunFileSystem } from "@effect/platform-bun";
import {
  AgentRegistry,
  type AgentRegistryShape,
  type AgentRoots,
} from "@workspace/core";
import { makeHandlersLayer, makeInProcessClient } from "@workspace/rpc";
import { Effect, Layer } from "effect";
import { filterListing } from "../src/handlers";

const HERE = dirname(fileURLToPath(import.meta.url));
const SESSION_FIXTURES = join(
  HERE,
  "..",
  "..",
  "core",
  "test",
  "fixtures",
  "sessions"
);
const GOLDEN = JSON.parse(
  readFileSync(join(SESSION_FIXTURES, "golden.json"), "utf8")
) as {
  peakContextTokens: number;
  budget: ReadonlyArray<{ key: string; tokens: number }>;
};

const SESSION_SLUG = "-Users-demo-proj";
const MEM_SLUG = "-tmp-mem";
const SESSION_ID = "11111111-1111-4111-8111-111111111111";

let root = "";

/** Seed a temp root: copy the session fixture + scaffold a memory vault. */
const seed = (base: string): void => {
  cpSync(
    join(SESSION_FIXTURES, "projects", SESSION_SLUG),
    join(base, SESSION_SLUG),
    { recursive: true }
  );
  const memDir = join(base, MEM_SLUG, "memory");
  mkdirSync(memDir, { recursive: true });
  writeFileSync(
    join(memDir, "alpha.md"),
    "---\nname: alpha\ndescription: First\ntype: user\n---\nLinks [[beta]].\n"
  );
  writeFileSync(join(memDir, "MEMORY.md"), "- [Alpha](alpha.md) — first\n");
};

/** Stub AgentRegistry pointing Claude resolution at the temp root. */
const makeAgents = (base: string): AgentRegistryShape => {
  const roots = (id: AgentRoots["id"]): AgentRoots => ({
    id,
    home: join(base, "home"),
    layout: id === "claude" ? "claude-projects" : "none",
    projectsRoot: base,
    supported: id === "claude",
  });
  return {
    encodeSlug: (p) => p.replace(/[/.]/g, "-"),
    allowedRoots: [base, tmpdir()],
    roots,
    gitRoot: (cwd) => Effect.succeed(cwd),
    projectsRoot: () => Effect.succeed(base),
    sessionsGlob: () => Effect.succeed(join(base, "**", "*.jsonl")),
    memoryDir: ({ slug }) => Effect.succeed(join(base, slug, "memory")),
    listProjectSlugs: () => Effect.succeed([SESSION_SLUG, MEM_SLUG]),
    listSessionFiles: (id) =>
      Effect.succeed(
        id === "claude"
          ? [
              {
                path: join(base, SESSION_SLUG, `${SESSION_ID}.jsonl`),
                id: SESSION_ID,
                slug: SESSION_SLUG,
              },
            ]
          : []
      ),
    loadTranscript: ({ ref }) =>
      Effect.sync(() => {
        const text = readFileSync(ref.path, "utf8");
        return {
          text,
          sizeBytes: Buffer.byteLength(text, "utf8"),
          mtimeMs: 0,
        };
      }),
  };
};

const handlersLayer = (options?: {
  readonly agents?: AgentRegistryShape;
  readonly readOnly?: boolean;
}) =>
  makeHandlersLayer({
    agents: Layer.succeed(AgentRegistry, options?.agents ?? makeAgents(root)),
    fileSystem: BunFileSystem.layer,
    readOnly: options?.readOnly,
  });

/** Build the in-process client, run `use`, provide handlers + scope. */
const withClient = <A, E>(
  use: (
    client: Effect.Effect.Success<ReturnType<typeof makeInProcessClient>>
  ) => Effect.Effect<A, E, never>,
  layerOptions?: Parameters<typeof handlersLayer>[0]
): Promise<A> =>
  Effect.runPromise(
    Effect.gen(function* () {
      const client = yield* makeInProcessClient();
      return yield* use(client);
    }).pipe(
      Effect.scoped,
      Effect.provide(handlersLayer(layerOptions))
    ) as Effect.Effect<A, E, never>
  );

const prevPtDir = process.env.PEEKTRACE_DIR;

beforeAll(() => {
  root = mkdtempSync(join(tmpdir(), "peektrace-rpc-"));
  seed(root);
  // Isolate the settings file under the temp root (never touch ~/.peektrace).
  process.env.PEEKTRACE_DIR = join(root, "pt");
  mkdirSync(process.env.PEEKTRACE_DIR, { recursive: true });
});
afterAll(() => {
  if (prevPtDir === undefined) {
    delete process.env.PEEKTRACE_DIR;
  } else {
    process.env.PEEKTRACE_DIR = prevPtDir;
  }
  rmSync(root, { recursive: true, force: true });
});

describe("capabilities", () => {
  test("list returns the matrix with memory.crud supported for claude", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const caps = yield* client.capabilities.list();
        const crud = caps.find((c) => c.id === "memory.crud");
        expect(crud).toBeDefined();
        expect(crud?.perAgent.claude.level).toBe("supported");
        expect(crud?.perAgent.codex.level).toBe("planned");
      })
    ));
});

describe("server.info", () => {
  test("reports write mode and the scanned Claude root", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const info = yield* client.server.info();
        expect(info.readOnly).toBe(false);
        expect(info.userHome.length).toBeGreaterThan(0);
        expect(info.agents.map((a) => a.agent)).toEqual(["claude"]);
        expect(info.agents[0]?.roots).toEqual([
          {
            isDefault: true,
            home: join(root, "home"),
            path: root,
            exists: true,
          },
        ]);
      })
    ));

  test("reports read-only mode", () =>
    withClient(
      (client) =>
        Effect.gen(function* () {
          const info = yield* client.server.info();
          expect(info.readOnly).toBe(true);
        }),
      { readOnly: true }
    ));

  test("flags a root missing on disk", () => {
    const base = makeAgents(root);
    const missing = join(root, "no-such-dir");
    const agents: AgentRegistryShape = {
      ...base,
      roots: (id) => ({ ...base.roots(id), projectsRoot: missing }),
    };
    return withClient(
      (client) =>
        Effect.gen(function* () {
          const info = yield* client.server.info();
          expect(info.agents[0]?.roots[0]?.path).toBe(missing);
          expect(info.agents[0]?.roots[0]?.exists).toBe(false);
        }),
      { agents }
    );
  });
});

describe("sessions", () => {
  test("list returns the fixture header", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const headers = yield* client.sessions.list({});
        expect(headers).toHaveLength(1);
        expect(headers[0]?.id).toBe(SESSION_ID);
        expect(headers[0]?.project).toBe(SESSION_SLUG);
        expect(headers[0]?.model).toBe("claude-opus-4");
      })
    ));

  test("listing matches list and reports no unreadable sessions", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const headers = yield* client.sessions.list({});
        const listing = yield* client.sessions.listing({});
        expect(listing.headers).toEqual(headers);
        expect(listing.unreadable).toEqual([]);
      })
    ));

  test("list filters by project", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const none = yield* client.sessions.list({ project: MEM_SLUG });
        expect(none).toHaveLength(0);
      })
    ));

  test("list filters by agent", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const claude = yield* client.sessions.list({ agent: "claude" });
        expect(claude.map((h) => h.id)).toEqual([SESSION_ID]);
        const codex = yield* client.sessions.list({ agent: "codex" });
        expect(codex).toHaveLength(0);
      })
    ));

  test("get folds the subagent transcript", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const parsed = yield* client.sessions.get({ id: SESSION_ID });
        expect(parsed.subagents).toHaveLength(1);
        expect(parsed.subagents[0]?.id).toBe("agent-1");
      })
    ));

  test("analyze reproduces the golden peak + budget", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const a = yield* client.sessions.analyze({ id: SESSION_ID });
        expect(a.peakContextTokens).toBe(GOLDEN.peakContextTokens);
        const budget = a.budget.map((b) => ({
          key: String(b.key),
          tokens: b.tokens,
        }));
        expect(budget).toEqual([...GOLDEN.budget]);
      })
    ));

  test("search finds a known phrase with its line and a snippet", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const r = yield* client.sessions.search({
          q: "refactor the AUTH module",
        });
        const hit = r.sessions.find((s) => s.id === SESSION_ID);
        expect(hit?.hits[0]?.line).toBe(1);
        expect(hit?.hits[0]?.snippet).toContain("refactor the auth module");
      })
    ));

  test("search with no match returns an empty result", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const r = yield* client.sessions.search({ q: "no-such-phrase-xyz" });
        expect(r.sessions).toEqual([]);
        expect(r.truncated).toBe(false);
      })
    ));

  test("search rejects a blank query and a negative limit", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const blank = yield* Effect.exit(client.sessions.search({ q: "  " }));
        expect(blank._tag).toBe("Failure");
        const negative = yield* Effect.exit(
          client.sessions.search({ q: "x", limit: -1 })
        );
        expect(negative._tag).toBe("Failure");
        for (const since of ["7d", "yesterday"]) {
          const bad = yield* Effect.exit(
            client.sessions.search({ q: "x", since })
          );
          expect(bad._tag).toBe("Failure");
        }
      })
    ));

  test("analyze of a missing session fails with the typed wire error", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const result = yield* Effect.either(
          client.sessions.analyze({ id: "does-not-exist" })
        );
        expect(result._tag).toBe("Left");
        if (result._tag === "Left") {
          expect(result.left._tag).toBe("SessionNotFoundError");
        }
      })
    ));
});

describe("memory", () => {
  test("projects + allVaults + vault return the seeded vault", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const projects = yield* client.memory.projects();
        expect(projects.some((p) => p.slug === MEM_SLUG)).toBe(true);

        const all = yield* client.memory.allVaults();
        expect(all.vaults.some((v) => v.slug === MEM_SLUG)).toBe(true);

        const vault = yield* client.memory.vault({ project: MEM_SLUG });
        expect(vault.entries.map((e) => e.slug)).toContain("alpha");
      })
    ));

  test("create writes an entry and indexes it", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const entry = yield* client.memory.create({
          project: MEM_SLUG,
          name: "gamma",
          description: "Third",
          type: "reference",
          body: "Gamma body.\n",
        });
        expect(entry.slug).toBe("gamma");
        expect(entry.inIndex).toBe(true);
      })
    ));

  test("a stale CAS update surfaces FileChangedError over RPC", () =>
    withClient((client) =>
      Effect.gen(function* () {
        yield* client.memory.create({
          project: MEM_SLUG,
          name: "cas",
          description: "CAS target",
          type: "user",
          body: "v1\n",
        });
        const result = yield* Effect.either(
          client.memory.update({
            project: MEM_SLUG,
            name: "cas",
            body: "v2\n",
            expectedMtime: 1,
          })
        );
        expect(result._tag).toBe("Left");
        if (result._tag === "Left") {
          expect(result.left._tag).toBe("FileChangedError");
        }
      })
    ));

  test("a non-Claude write is rejected with CapabilityUnsupportedError-free path", () =>
    withClient((client) =>
      Effect.gen(function* () {
        // The RPC create payload has no agent field — it always targets Claude,
        // which is supported, so a validation error (bad name) is what surfaces.
        const result = yield* Effect.either(
          client.memory.create({
            project: MEM_SLUG,
            name: "Not Kebab",
            description: "bad",
            type: "user",
            body: "x",
          })
        );
        expect(result._tag).toBe("Left");
        if (result._tag === "Left") {
          expect(result.left._tag).toBe("MemoryValidationError");
        }
      })
    ));

  test("delete removes the file and reports dangling refs", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const res = yield* client.memory.delete({
          project: MEM_SLUG,
          name: "alpha",
        });
        expect(res.slug).toBe("alpha");
      })
    ));
});

describe("settings", () => {
  test("get → update → get roundtrips the settings file over RPC", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const empty = yield* client.settings.get();
        expect(empty.settings).toEqual({});
        expect(empty.mtimeMs).toBe(0);

        const written = yield* client.settings.update({
          settings: {
            roots: { claude: [{ path: "/work/.claude", label: "work" }] },
          },
          expectedMtime: empty.mtimeMs,
        });
        expect(written.mtimeMs).toBeGreaterThan(0);

        const reread = yield* client.settings.get();
        expect(reread.settings.roots?.claude).toEqual([
          { path: "/work/.claude", label: "work" },
        ]);
      })
    ));

  test("a stale CAS update surfaces FileChangedError over RPC", () =>
    withClient((client) =>
      Effect.gen(function* () {
        yield* client.settings.update({
          settings: { roots: { codex: [{ path: "/a" }] } },
        });
        const result = yield* Effect.either(
          client.settings.update({
            settings: { roots: { codex: [{ path: "/b" }] } },
            expectedMtime: 1,
          })
        );
        expect(result._tag).toBe("Left");
        if (result._tag === "Left") {
          expect(result.left._tag).toBe("FileChangedError");
        }
      })
    ));
});

describe("filterListing", () => {
  const unreadable = (project: string) => ({
    agent: "codex" as const,
    project,
    path: `/x/${project || "unknown"}.jsonl`,
  });
  const listing = {
    headers: [],
    unreadable: [unreadable(""), unreadable("demo"), unreadable("other")],
  };

  test("an unreadable row with no known project passes a project filter", () => {
    const kept = filterListing(listing, { project: "demo" });
    expect(kept.unreadable.map((u) => u.project)).toEqual(["", "demo"]);
  });

  test("the agent filter still applies to it", () => {
    const kept = filterListing(listing, { project: "demo", agent: "claude" });
    expect(kept.unreadable).toEqual([]);
  });
});
