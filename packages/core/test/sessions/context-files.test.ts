/** On-disk context files Claude Code loads for a cwd. Synthetic temp tree only. */
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
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
const agentsRepo = () => join(base, "agents-only");
const mainRepo = () => join(base, "top", "user", "main");
const worktree = () => join(mainRepo(), ".trees", "feat");

const encodeSlug = (p: string) => p.replace(/[/.]/g, "-");

const put = (path: string, body: string) => {
  mkdirSync(join(path, ".."), { recursive: true });
  writeFileSync(path, body);
};

beforeAll(() => {
  // The walk reads every ancestor, so keep the tree away from any real home.
  const root = existsSync("/tmp") ? "/tmp" : tmpdir();
  base = mkdtempSync(join(root, "peektrace-ctxfiles-"));
  claudeHome = join(base, "top", "user", ".claude");
  projectsRoot = join(claudeHome, "projects");
  repo = join(base, "top", "user", "repo");
  cwd = join(repo, "pkg");
  put(join(claudeHome, "CLAUDE.md"), "global rules\n");
  // Above the user's home: still read, Claude Code walks up to `/`.
  put(join(base, "top", "CLAUDE.local.md"), "above home\n");
  put(join(repo, "CLAUDE.md"), "parent dir rules\n");
  put(join(repo, "CLAUDE.local.md"), "parent local rules\n");
  put(join(cwd, "CLAUDE.md"), "cwd rules\n");
  put(join(cwd, "CLAUDE.local.md"), "cwd local rules\n");
  // A project with CLAUDE.md does not load AGENTS.md.
  put(join(cwd, "AGENTS.md"), "x".repeat(4000));
  // A project with only AGENTS.md loads it where CLAUDE.md would be.
  put(join(agentsRepo(), "AGENTS.md"), "agents rules\n");
  put(join(agentsRepo(), "app", ".claude", "AGENTS.md"), "nested agents\n");
  // A worktree nested inside its main checkout.
  mkdirSync(join(mainRepo(), ".git", "worktrees", "feat"), { recursive: true });
  put(join(mainRepo(), "CLAUDE.md"), "main checkout rules\n");
  put(join(mainRepo(), "CLAUDE.local.md"), "main local rules\n");
  put(join(mainRepo(), ".trees", "CLAUDE.md"), "between rules\n");
  put(join(worktree(), ".git"), "gitdir: ../../.git/worktrees/feat\n");
  put(join(worktree(), "CLAUDE.md"), "worktree rules\n");
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

  test("skips AGENTS.md when the project has CLAUDE.md", async () => {
    const paths = (await gather()).map((f) => f.path);
    expect(paths).not.toContain(join(cwd, "AGENTS.md"));
  });

  test("reads AGENTS.md when the project has no CLAUDE file", async () => {
    const paths = (await gather(join(agentsRepo(), "app"))).map((f) => f.path);
    expect(paths).toContain(join(agentsRepo(), "AGENTS.md"));
    expect(paths).toContain(join(agentsRepo(), "app", ".claude", "AGENTS.md"));
  });

  test("counts only memory/MEMORY.md", async () => {
    const mem = (await gather()).filter((f) => f.scope === "memory");
    expect(mem).toHaveLength(1);
    expect(mem[0]?.bytes).toBe("- index line\n".length);
  });

  test("walks above the user's home", async () => {
    const paths = (await gather()).map((f) => f.path);
    expect(paths).toContain(join(base, "top", "CLAUDE.local.md"));
  });

  test("lists ancestors farthest first after the global file", async () => {
    const paths = (await gather())
      .map((f) => f.path)
      .filter((p) => p.startsWith(base));
    expect(paths.slice(0, 6)).toEqual([
      join(claudeHome, "CLAUDE.md"),
      join(base, "top", "CLAUDE.local.md"),
      join(repo, "CLAUDE.md"),
      join(repo, "CLAUDE.local.md"),
      join(cwd, "CLAUDE.md"),
      join(cwd, "CLAUDE.local.md"),
    ]);
  });

  test("a nested worktree skips the main checkout's project files", async () => {
    const paths = (await gather(join(worktree(), "src"))).map((f) => f.path);
    expect(paths).toContain(join(worktree(), "CLAUDE.md"));
    expect(paths).toContain(join(mainRepo(), "CLAUDE.local.md"));
    expect(paths).not.toContain(join(mainRepo(), "CLAUDE.md"));
    expect(paths).not.toContain(join(mainRepo(), ".trees", "CLAUDE.md"));
  });

  test("the main checkout itself reads its CLAUDE.md", async () => {
    const paths = (await gather(mainRepo())).map((f) => f.path);
    expect(paths).toContain(join(mainRepo(), "CLAUDE.md"));
  });

  test("caps MEMORY.md at 200 lines", async () => {
    const mem = (await gather(longCwd())).filter((f) => f.scope === "memory");
    expect(mem[0]?.bytes).toBe("- line\n".repeat(200).length - 1);
  });
});
