import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { BunFileSystem } from "@effect/platform-bun";
import { Effect, Layer } from "effect";
import {
  AgentRegistry,
  type AgentRegistryShape,
  type AgentRoots,
} from "../../src/services/agents";
import { CapabilityRegistryLive } from "../../src/services/capabilities";
import { FsLive } from "../../src/services/fs";
import { MemoryService, MemoryServiceLive } from "../../src/services/memory";
import {
  composeFile,
  parseFrontmatter,
} from "../../src/services/memory/frontmatter";

const FIXTURES = join(
  dirname(fileURLToPath(import.meta.url)),
  "..",
  "fixtures",
  "memory"
);

const SLUG_A = "-tmp-proj-a";
const SLUG_B = "-tmp-proj-b";

let root = "";

/** Build a real on-disk vault spanning two projects under a temp root. */
const seedVault = (base: string): void => {
  const memA = join(base, SLUG_A, "memory");
  const memB = join(base, SLUG_B, "memory");
  mkdirSync(memA, { recursive: true });
  mkdirSync(memB, { recursive: true });

  writeFileSync(
    join(memA, "a.md"),
    "---\nname: a\ndescription: First note\ntype: user\n---\nLinks to [[b]] here.\n"
  );
  writeFileSync(
    join(memA, "b.md"),
    "---\nname: b\ndescription: Second note\ntype: project\n---\nNo links here.\n"
  );
  writeFileSync(
    join(memA, "orphan.md"),
    "---\nname: orphan\ndescription: Unindexed note\ntype: reference\n---\nAlone.\n"
  );
  writeFileSync(
    join(memA, "MEMORY.md"),
    "- [A](a.md) — first\n- [B](b.md) — second\n- [Ghost](ghost.md) — missing\n"
  );

  writeFileSync(
    join(memB, "c.md"),
    "---\nname: c\ndescription: Solo note\ntype: user\n---\nSolo body.\n"
  );
};

/** Stub AgentRegistry pointing Claude resolution at the temp root. */
const makeAgents = (base: string): AgentRegistryShape => {
  const roots: AgentRoots = {
    id: "claude",
    home: base,
    projectsRoot: base,
    supported: true,
  };
  return {
    encodeSlug: (p) => p.replace(/[/.]/g, "-"),
    allowedRoots: [base, tmpdir()],
    roots: () => roots,
    gitRoot: (cwd) => Effect.succeed(cwd),
    projectsRoot: () => Effect.succeed(base),
    sessionsGlob: () => Effect.succeed(join(base, "**", "*.jsonl")),
    memoryDir: ({ slug }) => Effect.succeed(join(base, slug, "memory")),
    listProjectSlugs: () => Effect.succeed([SLUG_A, SLUG_B]),
    loadTranscript: () =>
      Effect.succeed({ text: "", sizeBytes: 0, mtimeMs: 0 }),
  };
};

const layerFor = (base: string) => {
  const agents = Layer.succeed(AgentRegistry, makeAgents(base));
  const fs = FsLive.pipe(
    Layer.provide(agents),
    Layer.provide(BunFileSystem.layer)
  );
  return MemoryServiceLive.pipe(
    Layer.provide(agents),
    Layer.provide(fs),
    Layer.provide(CapabilityRegistryLive),
    Layer.provide(BunFileSystem.layer)
  );
};

const run = <A, E>(program: Effect.Effect<A, E, MemoryService>) =>
  Effect.runPromise(
    program.pipe(Effect.provide(layerFor(root))) as Effect.Effect<A, E, never>
  );

beforeAll(() => {
  root = mkdtempSync(join(tmpdir(), "peektrace-mem-"));
  seedVault(root);
});
afterAll(() => {
  rmSync(root, { recursive: true, force: true });
});

describe("read model", () => {
  test("listProjects returns every project with a non-empty memory dir", () =>
    run(
      Effect.gen(function* () {
        const mem = yield* MemoryService;
        const projects = yield* mem.listProjects();
        const slugs = projects.map((p) => p.slug).sort();
        expect(slugs).toEqual([SLUG_A, SLUG_B]);
        const a = projects.find((p) => p.slug === SLUG_A);
        expect(a?.fileCount).toBe(3);
        expect(a?.hasIndex).toBe(true);
      })
    ));

  test("getVault parses entries, index, budget, diff, and graph", () =>
    run(
      Effect.gen(function* () {
        const mem = yield* MemoryService;
        const vault = yield* mem.getVault(SLUG_A);
        expect(vault.state).toBe("ok");
        expect(vault.entries.map((e) => e.slug).sort()).toEqual([
          "a",
          "b",
          "orphan",
        ]);
        expect(vault.budget.kind).toBe("index");
        // orphan.md is on disk but not in the index.
        expect(vault.diff.orphans).toContain("orphan");
        // The index points at ghost.md, which has no file.
        expect(
          vault.diff.dangling.some((d) => d.target.includes("ghost"))
        ).toBe(true);
        // a -> b body link resolves.
        const edge = vault.graph.edges.find(
          (e) => e.from === "a" && e.to === "b"
        );
        expect(edge?.resolved).toBe(true);
        expect(vault.typeCounts.user).toBe(1);
      })
    ));

  test("getAllVaults groups every memory by project", () =>
    run(
      Effect.gen(function* () {
        const mem = yield* MemoryService;
        const all = yield* mem.getAllVaults();
        expect(all.projects.length).toBe(2);
        expect(all.vaults.length).toBe(2);
      })
    ));
});

describe("create", () => {
  test("writes a file on disk, adds an index line, and recomputes budget", () =>
    run(
      Effect.gen(function* () {
        const mem = yield* MemoryService;
        const entry = yield* mem.create({
          project: SLUG_A,
          name: "fresh-note",
          description: "A freshly created note",
          type: "feedback",
          body: "Fresh body.\n",
        });
        expect(entry.slug).toBe("fresh-note");
        const filePath = join(root, SLUG_A, "memory", "fresh-note.md");
        expect(existsSync(filePath)).toBe(true);

        const indexRaw = readFileSync(
          join(root, SLUG_A, "memory", "MEMORY.md"),
          "utf8"
        );
        expect(indexRaw).toContain("(fresh-note.md)");

        const vault = yield* mem.getVault(SLUG_A);
        const found = vault.entries.find((e) => e.slug === "fresh-note");
        expect(found?.inIndex).toBe(true);
        expect(found?.type).toBe("feedback");
      })
    ));

  test("rejects a non-kebab name", () =>
    run(
      Effect.gen(function* () {
        const mem = yield* MemoryService;
        const result = yield* Effect.either(
          mem.create({
            project: SLUG_A,
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
});

describe("update with compare-and-swap", () => {
  test("a fresh mtime succeeds and a stale mtime is rejected", () =>
    run(
      Effect.gen(function* () {
        const mem = yield* MemoryService;
        const created = yield* mem.create({
          project: SLUG_B,
          name: "cas-note",
          description: "CAS target",
          type: "user",
          body: "v1\n",
        });

        // Fresh CAS succeeds.
        const updated = yield* mem.update({
          project: SLUG_B,
          name: "cas-note",
          body: "v2\n",
          expectedMtime: created.mtimeMs,
        });
        expect(updated.body.trim()).toBe("v2");

        // Mutate out-of-band and bump mtime far into the future.
        const filePath = join(root, SLUG_B, "memory", "cas-note.md");
        writeFileSync(filePath, "external\n");
        const future = new Date(Date.now() + 10_000);
        utimesSync(filePath, future, future);

        // Stale CAS (old mtime) must fail.
        const stale = yield* Effect.either(
          mem.update({
            project: SLUG_B,
            name: "cas-note",
            body: "v3\n",
            expectedMtime: created.mtimeMs,
          })
        );
        expect(stale._tag).toBe("Left");
        if (stale._tag === "Left") {
          expect(stale.left._tag).toBe("FileChangedError");
        }
      })
    ));
});

describe("delete", () => {
  test("removes the file + index line and reports dangling refs", () =>
    run(
      Effect.gen(function* () {
        const mem = yield* MemoryService;
        // a.md links to b; deleting b leaves a's link dangling.
        const result = yield* mem.delete({ project: SLUG_A, name: "b" });
        expect(result.slug).toBe("b");
        expect(result.dangling.some((d) => d.from === "a")).toBe(true);

        expect(existsSync(join(root, SLUG_A, "memory", "b.md"))).toBe(false);
        const indexRaw = readFileSync(
          join(root, SLUG_A, "memory", "MEMORY.md"),
          "utf8"
        );
        expect(indexRaw).not.toContain("(b.md)");
      })
    ));
});

describe("capability gating", () => {
  test("a codex write is rejected with CapabilityUnsupportedError", () =>
    run(
      Effect.gen(function* () {
        const mem = yield* MemoryService;
        const result = yield* Effect.either(
          mem.create({
            project: SLUG_A,
            name: "codex-note",
            description: "should fail",
            type: "user",
            body: "x",
            agent: "codex",
          })
        );
        expect(result._tag).toBe("Left");
        if (result._tag === "Left") {
          expect(result.left._tag).toBe("CapabilityUnsupportedError");
        }
      })
    ));
});

describe("frontmatter round-trip", () => {
  test("parse + compose is byte-stable for both on-disk shapes", () => {
    for (const name of ["nested.md", "flat.md"]) {
      const original = readFileSync(join(FIXTURES, name), "utf8");
      const { frontmatter, body } = parseFrontmatter(original);
      expect(composeFile({ frontmatter, body })).toBe(original);
    }
  });
});
