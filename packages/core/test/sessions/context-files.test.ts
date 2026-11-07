/** On-disk context files Claude Code loads for a cwd. Synthetic temp tree only. */
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { FileSystem } from "@effect/platform";
import { BunFileSystem } from "@effect/platform-bun";
import { Effect } from "effect";
import type { AgentId, AgentRegistryShape } from "../../src/services/agents";
import { gatherOnDiskContextFiles } from "../../src/services/sessions/resolve";

let base = "";
let claudeHome = "";
let projectsRoot = "";
let repo = "";
let cwd = "";

const longCwd = () => join(repo, "long");

const encodeSlug = (p: string) => p.replace(/[/.]/g, "-");

const put = (path: string, body: string) => {
  mkdirSync(join(path, ".."), { recursive: true });
  writeFileSync(path, body);
};

beforeAll(() => {
  base = mkdtempSync(join(tmpdir(), "peektrace-ctxfiles-"));
  claudeHome = join(base, "user", ".claude");
  projectsRoot = join(claudeHome, "projects");
  repo = join(base, "user", "repo");
  cwd = join(repo, "pkg");
  put(join(claudeHome, "CLAUDE.md"), "global rules\n");
  // Above the user's home: never read.
  put(join(base, "CLAUDE.md"), "outside home\n");
  put(join(repo, "CLAUDE.md"), "parent dir rules\n");
  put(join(repo, "CLAUDE.local.md"), "parent local rules\n");
  put(join(cwd, "CLAUDE.md"), "cwd rules\n");
  put(join(cwd, "CLAUDE.local.md"), "cwd local rules\n");
  // Claude Code does not load AGENTS.md.
  put(join(cwd, "AGENTS.md"), "x".repeat(4000));
  const mem = join(projectsRoot, encodeSlug(cwd), "memory");
  put(join(mem, "MEMORY.md"), "- index line\n");
  const longMem = join(projectsRoot, encodeSlug(longCwd()), "memory");
  put(join(longMem, "MEMORY.md"), "- line\n".repeat(300));
  // Topic files are read on demand, not injected.
  put(join(mem, "topic.md"), "y".repeat(8000));
});

afterAll(() => {
  rmSync(base, { recursive: true, force: true });
});

const agents = (): AgentRegistryShape =>
  ({
    encodeSlug,
    roots: (id: AgentId) => ({
      id,
      home: claudeHome,
      layout: "claude-projects",
      projectsRoot,
      supported: id === "claude",
    }),
  }) as unknown as AgentRegistryShape;

const gather = (at = cwd) =>
  Effect.runPromise(
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      return yield* gatherOnDiskContextFiles({ fs, agents: agents(), cwd: at });
    }).pipe(Effect.provide(BunFileSystem.layer))
  );

describe("gatherOnDiskContextFiles", () => {
  test("reads parent-dir CLAUDE.md and CLAUDE.local.md", async () => {
    const paths = (await gather()).map((f) => f.path);
    expect(paths).toContain(join(repo, "CLAUDE.md"));
    expect(paths).toContain(join(repo, "CLAUDE.local.md"));
    expect(paths).toContain(join(cwd, "CLAUDE.local.md"));
    expect(paths).toContain(join(cwd, "CLAUDE.md"));
    expect(paths).toContain(join(claudeHome, "CLAUDE.md"));
  });

  test("skips AGENTS.md", async () => {
    const paths = (await gather()).map((f) => f.path);
    expect(paths).not.toContain(join(cwd, "AGENTS.md"));
  });

  test("counts only memory/MEMORY.md", async () => {
    const mem = (await gather()).filter((f) => f.scope === "memory");
    expect(mem).toHaveLength(1);
    expect(mem[0]?.bytes).toBe("- index line\n".length);
  });

  test("stops at the user's home", async () => {
    const paths = (await gather()).map((f) => f.path);
    expect(paths).not.toContain(join(base, "CLAUDE.md"));
  });

  test("lists ancestors farthest first after the global file", async () => {
    const paths = (await gather()).map((f) => f.path);
    expect(paths.slice(0, 5)).toEqual([
      join(claudeHome, "CLAUDE.md"),
      join(repo, "CLAUDE.md"),
      join(repo, "CLAUDE.local.md"),
      join(cwd, "CLAUDE.md"),
      join(cwd, "CLAUDE.local.md"),
    ]);
  });

  test("caps MEMORY.md at 200 lines", async () => {
    const mem = (await gather(longCwd())).filter((f) => f.scope === "memory");
    expect(mem[0]?.bytes).toBe("- line\n".repeat(200).length - 1);
  });
});
