import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { FileSystem } from "@effect/platform";
import { BunFileSystem } from "@effect/platform-bun";
import { Effect, Layer } from "effect";
import { AgentRegistry, AgentRegistryLive } from "../../src/services/agents";
import {
  findSubagents,
  gatherOnDiskContextFiles,
  resolveClaudeSession,
} from "../../src/services/sessions/resolve";

const ID_A = "aaaaaaaa-1111-4aaa-8aaa-aaaaaaaaaaaa";
const ID_B = "aaaaaaaa-2222-4aaa-8aaa-aaaaaaaaaaaa";
const ID_C = "cccccccc-3333-4ccc-8ccc-cccccccccccc";

let base = "";
let cwd = "";
const keys = [
  "PEEKTRACE_DIR",
  "CLAUDE_CONFIG_DIR",
  "PEEKTRACE_CLAUDE_PROJECTS",
] as const;
const prev = Object.fromEntries(keys.map((k) => [k, process.env[k]]));

const layer = Layer.merge(
  AgentRegistryLive.pipe(Layer.provide(BunFileSystem.layer)),
  BunFileSystem.layer
);
const run = <A, E>(
  program: Effect.Effect<A, E, AgentRegistry | FileSystem.FileSystem>
) =>
  Effect.runPromise(
    program.pipe(Effect.provide(layer)) as Effect.Effect<A, E, never>
  );

beforeAll(async () => {
  base = mkdtempSync(join(tmpdir(), "peektrace-resolve-"));
  const home = join(base, "claude");
  const projects = join(home, "projects");
  cwd = join(base, "work", "demo");
  mkdirSync(join(base, "pt"), { recursive: true });
  process.env.PEEKTRACE_DIR = join(base, "pt");
  process.env.CLAUDE_CONFIG_DIR = home;
  process.env.PEEKTRACE_CLAUDE_PROJECTS = projects;
  mkdirSync(home, { recursive: true });
  mkdirSync(join(cwd, ".claude"), { recursive: true });
  writeFileSync(join(home, "CLAUDE.md"), "global rules".padEnd(40, "."));
  writeFileSync(join(cwd, "AGENTS.md"), "project rules");
  writeFileSync(join(cwd, ".claude", "CLAUDE.md"), "nested");
  const slug = await run(
    Effect.map(AgentRegistry, (agents) => agents.encodeSlug(cwd))
  );
  const mem = join(projects, slug, "memory");
  mkdirSync(join(mem, "sub"), { recursive: true });
  writeFileSync(join(mem, "MEMORY.md"), "1234");
  writeFileSync(join(mem, "sub", "note.md"), "12345678");
  for (const id of [ID_A, ID_B, ID_C]) {
    writeFileSync(join(projects, slug, `${id}.jsonl`), "{}\n");
  }
  const subs = join(projects, slug, ID_C, "subagents");
  mkdirSync(subs, { recursive: true });
  writeFileSync(join(subs, "agent-x1.jsonl"), "{}\n");
  writeFileSync(
    join(subs, "agent-x1.meta.json"),
    JSON.stringify({ agentType: "Explore", description: "scan files" })
  );
});

afterAll(() => {
  for (const k of keys) {
    if (prev[k] === undefined) {
      delete process.env[k];
    } else {
      process.env[k] = prev[k];
    }
  }
  rmSync(base, { recursive: true, force: true });
});

describe("gatherOnDiskContextFiles", () => {
  test("collects global, project and memory files", () =>
    run(
      Effect.gen(function* () {
        const fs = yield* FileSystem.FileSystem;
        const agents = yield* AgentRegistry;
        const all = yield* gatherOnDiskContextFiles({ fs, agents, cwd });
        // The ancestor walk reaches host dirs above the temp tree; ignore them.
        const files = all.filter((f) => f.path.startsWith(base));
        // AGENTS.md is skipped (the project has a CLAUDE file) and only
        // memory/MEMORY.md loads, not the rest of memory/.
        expect(files.map((f) => [f.scope, f.bytes])).toEqual([
          ["global", 40],
          ["project", 6],
          ["memory", 4],
        ]);
        expect(files.at(-1)?.label).toBe("memory/MEMORY.md");
      })
    ));
});

describe("resolveClaudeSession", () => {
  test("unique prefix resolves; ambiguous prefix is not found", () =>
    run(
      Effect.gen(function* () {
        const fs = yield* FileSystem.FileSystem;
        const agents = yield* AgentRegistry;
        const hit = yield* resolveClaudeSession({
          fs,
          agents,
          idOrPath: "cccccccc",
        });
        expect(hit.sessionId).toBe(ID_C);
        expect(hit.subagentDir?.endsWith("/subagents")).toBe(true);
        const err = yield* resolveClaudeSession({
          fs,
          agents,
          idOrPath: "aaaaaaaa",
        }).pipe(Effect.flip);
        expect(err._tag).toBe("SessionNotFoundError");
      })
    ));

  test("findSubagents reads meta; no dir yields none", () =>
    run(
      Effect.gen(function* () {
        const fs = yield* FileSystem.FileSystem;
        const agents = yield* AgentRegistry;
        const { subagentDir } = yield* resolveClaudeSession({
          fs,
          agents,
          idOrPath: ID_C,
        });
        const stubs = yield* findSubagents({ fs, subagentDir });
        expect(stubs.map((s) => [s.agentType, s.description])).toEqual([
          ["Explore", "scan files"],
        ]);
        expect(yield* findSubagents({ fs })).toEqual([]);
      })
    ));
});
