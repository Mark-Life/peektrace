/** On-disk context files Claude Code loads for a cwd. Synthetic temp tree only;
 * the file system hides every path outside it, so host files never leak in. */
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { FileSystem } from "@effect/platform";
import { SystemError } from "@effect/platform/Error";
import { BunFileSystem } from "@effect/platform-bun";
import { Effect, Layer } from "effect";
import type { AgentId, AgentRegistryShape } from "../../src/services/agents";
import { capMemory } from "../../src/services/sessions/context-dirs";
import { ruleGlobs } from "../../src/services/sessions/context-rules";
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
const outsideTree = () => join(base, "elsewhere", "feat");

const encodeSlug = (p: string) => p.replace(/[/.]/g, "-");

const put = (path: string, body: string) => {
  mkdirSync(join(path, ".."), { recursive: true });
  writeFileSync(path, body);
};

beforeAll(() => {
  base = mkdtempSync(join(tmpdir(), "peektrace-ctxfiles-"));
  claudeHome = join(base, "top", "user", ".claude");
  projectsRoot = join(claudeHome, "projects");
  repo = join(base, "top", "user", "repo");
  cwd = join(repo, "pkg");
  put(join(claudeHome, "CLAUDE.md"), "global rules\n");
  put(join(claudeHome, "rules", "style.md"), "user rule\n");
  put(
    join(claudeHome, "rules", "scoped.md"),
    "---\npaths: src/**/*.ts\n---\nonly for ts\n"
  );
  put(join(repo, ".claude", "rules", "b.md"), "rule b\n");
  put(join(repo, ".claude", "rules", "a", "nested.md"), "nested rule\n");
  put(join(repo, ".claude", "rules", "notes.txt"), "not a rule\n");
  put(
    join(repo, ".claude", "rules", "scoped.md"),
    "---\npaths:\n  - lib/**\n---\nonly for lib\n"
  );
  put(
    join(repo, ".claude", "rules", "everywhere.md"),
    '---\npaths: "**"\n---\nalways\n'
  );
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
  put(join(mainRepo(), ".claude", "rules", "main.md"), "main rule\n");
  // Auto-memory is keyed by the main checkout, also from a worktree subdir.
  put(
    join(projectsRoot, encodeSlug(mainRepo()), "memory", "MEMORY.md"),
    "- main memory\n"
  );
  // A worktree outside its main checkout.
  mkdirSync(join(base, "elsewhere", "main", ".git", "worktrees", "feat"), {
    recursive: true,
  });
  put(join(outsideTree(), ".git"), "gitdir: ../main/.git/worktrees/feat\n");
  put(
    join(
      projectsRoot,
      encodeSlug(join(base, "elsewhere", "main")),
      "memory",
      "MEMORY.md"
    ),
    "- elsewhere memory\n"
  );
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

/** Bun's file system with every path outside `base` reported missing. */
const sandboxed = Layer.effect(
  FileSystem.FileSystem,
  Effect.map(FileSystem.FileSystem, (fs) => {
    const inside = (p: string) => p === base || p.startsWith(`${base}/`);
    const missing = (method: string, p: string) =>
      new SystemError({
        reason: "NotFound",
        module: "FileSystem",
        method,
        pathOrDescriptor: p,
      });
    return {
      ...fs,
      exists: (p) => (inside(p) ? fs.exists(p) : Effect.succeed(false)),
      stat: (p) => (inside(p) ? fs.stat(p) : Effect.fail(missing("stat", p))),
      realPath: (p) =>
        inside(p) ? fs.realPath(p) : Effect.fail(missing("realPath", p)),
      readFileString: (p, enc) =>
        inside(p)
          ? fs.readFileString(p, enc)
          : Effect.fail(missing("readFileString", p)),
      readDirectory: (p, opts) =>
        inside(p)
          ? fs.readDirectory(p, opts)
          : Effect.fail(missing("readDirectory", p)),
    } satisfies FileSystem.FileSystem;
  })
).pipe(Layer.provide(BunFileSystem.layer));

const gather = (at = cwd) =>
  Effect.runPromise(
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      return yield* gatherOnDiskContextFiles({ fs, agents: agents(), cwd: at });
    }).pipe(Effect.provide(sandboxed))
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
    expect(mem[0]?.bytes).toBe("- index line".length);
  });

  test("walks above the user's home", async () => {
    const paths = (await gather()).map((f) => f.path);
    expect(paths).toContain(join(base, "top", "CLAUDE.local.md"));
  });

  test("lists ancestors farthest first after the global file", async () => {
    const paths = (await gather())
      .map((f) => f.path)
      .filter((p) => p.startsWith(base));
    expect(paths).toEqual([
      join(claudeHome, "CLAUDE.md"),
      join(claudeHome, "rules", "style.md"),
      join(base, "top", "CLAUDE.local.md"),
      join(repo, "CLAUDE.md"),
      join(repo, ".claude", "rules", "a", "nested.md"),
      join(repo, ".claude", "rules", "b.md"),
      join(repo, ".claude", "rules", "everywhere.md"),
      join(repo, "CLAUDE.local.md"),
      join(cwd, "CLAUDE.md"),
      join(cwd, "CLAUDE.local.md"),
      join(projectsRoot, encodeSlug(cwd), "memory", "MEMORY.md"),
    ]);
  });

  test("a nested worktree skips the main checkout's project files", async () => {
    const paths = (await gather(join(worktree(), "src"))).map((f) => f.path);
    expect(paths).toContain(join(worktree(), "CLAUDE.md"));
    expect(paths).toContain(join(mainRepo(), "CLAUDE.local.md"));
    expect(paths).not.toContain(join(mainRepo(), "CLAUDE.md"));
    expect(paths).not.toContain(join(mainRepo(), ".trees", "CLAUDE.md"));
    expect(paths).not.toContain(
      join(mainRepo(), ".claude", "rules", "main.md")
    );
  });

  test("reads unconditional rules, skips ones scoped by paths", async () => {
    const files = await gather();
    const paths = files.map((f) => f.path);
    expect(paths).not.toContain(join(claudeHome, "rules", "scoped.md"));
    expect(paths).not.toContain(join(repo, ".claude", "rules", "scoped.md"));
    expect(paths).not.toContain(join(repo, ".claude", "rules", "notes.txt"));
    const user = files.find((f) => f.path.endsWith("style.md"));
    expect(user?.scope).toBe("global");
  });

  test("keys MEMORY.md by the main checkout from a nested worktree", async () => {
    const mem = (await gather(join(worktree(), "src"))).filter(
      (f) => f.scope === "memory"
    );
    expect(mem.map((f) => f.path)).toEqual([
      join(projectsRoot, encodeSlug(mainRepo()), "memory", "MEMORY.md"),
    ]);
  });

  test("keys MEMORY.md by the main checkout from a subdir", async () => {
    const mem = (await gather(join(mainRepo(), "src"))).filter(
      (f) => f.scope === "memory"
    );
    expect(mem[0]?.bytes).toBe("- main memory".length);
  });

  test("keys MEMORY.md by the main checkout from an outside worktree", async () => {
    const mem = (await gather(outsideTree())).filter(
      (f) => f.scope === "memory"
    );
    expect(mem[0]?.bytes).toBe("- elsewhere memory".length);
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

describe("capMemory", () => {
  test("counts characters, not UTF-8 bytes", () => {
    // ~23k chars, ~46k bytes: under the char cap, so all of it is kept.
    const text = `${"я".repeat(150)}\n`.repeat(150);
    expect(capMemory(text)).toBe(text.trim());
  });

  test("cuts past 25,000 characters at the last newline", () => {
    const line = "x".repeat(99);
    const out = capMemory(`${line}\n`.repeat(190) + "y".repeat(10_000));
    expect(out).toBe(`${line}\n`.repeat(190).trimEnd());
  });

  test("cuts at the limit when the text has no newline", () => {
    expect(capMemory("z".repeat(30_000))).toHaveLength(25_000);
  });
});

describe("ruleGlobs", () => {
  test("reads inline, bracketed and listed paths", () => {
    expect(ruleGlobs("---\npaths: a/**, b/*.ts\n---\n")).toEqual([
      "a",
      "b/*.ts",
    ]);
    expect(ruleGlobs('---\npaths: ["src/{a,b}.ts", lib]\n---\n')).toEqual([
      "src/{a,b}.ts",
      "lib",
    ]);
    expect(ruleGlobs("---\npaths:\n  - x/**\n  - 'y'\nname: n\n---\n")).toEqual(
      ["x", "y"]
    );
  });

  test("is empty without frontmatter or paths", () => {
    expect(ruleGlobs("paths: a\n")).toEqual([]);
    expect(ruleGlobs("---\nname: n\n---\nbody\n")).toEqual([]);
  });
});
