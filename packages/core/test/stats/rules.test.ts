/** Rule present but still firing: memory files read through the platform FS,
 * symlinks followed and deduped, then matched against each cause's probes. */
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import {
  mkdirSync,
  mkdtempSync,
  rmSync,
  symlinkSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { FileSystem } from "@effect/platform";
import { BunFileSystem } from "@effect/platform-bun";
import { Effect } from "effect";
import type { AgentId } from "../../src/services/agent-id";
import type { AgentRegistryShape, AgentRoots } from "../../src/services/agents";
import { GIT_DIFF_OUTPUT } from "../../src/services/stats/detectors/causes";
import {
  byMtime,
  type CauseCounts,
  type CoverSince,
  matchRules,
} from "../../src/services/stats/detectors/rules";
import {
  type MemoryDoc,
  readMemoryDocs,
} from "../../src/services/stats/memory-files";
import { dateLines, EMPTY_LEDGER } from "../../src/services/stats/rule-dates";
import type { CauseRow } from "../../src/services/stats/schema";
import { redact } from "./facts";

const SECRET = "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234";

let base = "";

const agentsAt = (user: string, claudeDir = ".claude"): AgentRegistryShape =>
  ({
    roots: (id: AgentRoots["id"]): AgentRoots => ({
      id,
      home: join(user, id === "claude" ? claudeDir : `.${id}`),
      layout: "none",
      projectsRoot: join(user, "projects"),
      supported: true,
    }),
  }) as unknown as AgentRegistryShape;

const readDocs = (
  projectDirs: readonly string[],
  claudeDir = ".claude",
  scope: readonly AgentId[] = ["claude", "codex"]
) =>
  Effect.runPromise(
    Effect.gen(function* () {
      const fs = yield* FileSystem.FileSystem;
      return yield* readMemoryDocs({
        fs,
        agents: agentsAt(join(base, "u"), claudeDir),
        scope,
        projectDirs,
      });
    }).pipe(Effect.provide(BunFileSystem.layer))
  );

beforeAll(() => {
  base = mkdtempSync(join(tmpdir(), "peektrace-rules-"));
  const user = join(base, "u");
  mkdirSync(join(user, ".agents"), { recursive: true });
  mkdirSync(join(user, ".claude"), { recursive: true });
  mkdirSync(join(user, ".codex"), { recursive: true });
  mkdirSync(join(user, "proj", "app"), { recursive: true });
  mkdirSync(join(user, "proj", ".git"), { recursive: true });
  mkdirSync(join(user, ".agents", "skills"), { recursive: true });
  const shared = join(user, ".agents", "AGENTS.md");
  writeFileSync(
    shared,
    [
      "# Shell",
      `- Token for the bot: ${SECRET}`,
      '- Quote separator echoes: `echo "==="`, never bare `echo ===`.',
      "- Quote globs passed as flag values: `grep -rn --include='*.ts' x .`.",
      "- Never `sleep N` then `tail` to wait on a job. Call Monitor.",
      "@~/.agents/skills/rm.md",
      "Example only: `@~/.agents/skills/missing.md`",
    ].join("\n")
  );
  writeFileSync(
    join(user, ".agents", "skills", "rm.md"),
    "- rm absolute paths only, never a bare $VAR target\n"
  );
  const mtime = new Date("2026-08-01T12:00:00.000Z");
  utimesSync(shared, mtime, mtime);
  symlinkSync(shared, join(user, ".claude", "CLAUDE.md"));
  symlinkSync(shared, join(user, ".codex", "AGENTS.md"));
  writeFileSync(
    join(base, "u", "proj", "AGENTS.md"),
    "- pkill: never chain it.\n- This host has no jq: use bun -e instead of jq."
  );
  writeFileSync(
    join(base, "u", "proj", "app", "CLAUDE.local.md"),
    "- Never install `node_modules` or build in `/tmp`."
  );
  writeFileSync(
    join(base, "u", "proj", "app", "AGENTS.override.md"),
    "- Override for Codex and Pi."
  );
  writeFileSync(
    join(base, "u", "proj", "app", "AGENTS.md"),
    "- Shadowed by the override."
  );
  mkdirSync(join(user, ".pi", "agent"), { recursive: true });
  writeFileSync(join(user, ".pi", "agent", "AGENTS.md"), "- Pi global rules.");
  writeFileSync(join(user, ".pi", "agent", "CLAUDE.md"), "- Shadowed for Pi.");
});

afterAll(() => {
  rmSync(base, { recursive: true, force: true });
});

const row = (over: Partial<CauseRow>): CauseRow => ({
  key: "cause:zsh-glob-flag",
  cause: "zsh-glob-flag",
  kind: "rule",
  label: "glob",
  fix: "quote",
  failing: true,
  hits: 10,
  flagged: 0,
  hitsBy: { main: 1, nested: 9 },
  pairs: 3,
  pairsBy: { main: 0, nested: 3 },
  retrySessions: 3,
  sessions: 9,
  projects: 1,
  retrySec: 10,
  secMeasured: 3,
  retryCtxTokens: 0,
  tokensMeasured: 0,
  per1k: { main: 1, nested: 1 },
  details: [],
  diffs: [],
  rule: null,
  samples: [],
  pairSamples: [],
  ...over,
});

/** Counts that reach every file and remember the days the check asked about. */
const countsFor = (asked: CoverSince[][]): CauseCounts => ({
  since: (_key, covers) => {
    asked.push([...covers]);
    return {
      hits: { main: 1, nested: 9 },
      calls: { main: 100, nested: 400 },
    };
  },
});

/** Counts where only Claude made calls: a Codex-only file reaches no hit. */
const claudeOnly: CauseCounts = {
  since: (_key, covers) => {
    const n = covers.some((c) => c.cover.agent !== "codex") ? 1 : 0;
    return {
      hits: { main: n, nested: 0 },
      calls: { main: 100 * n, nested: 0 },
    };
  },
};

describe("readMemoryDocs", () => {
  test("follows symlinks, dedupes by real path and reach, collapses home", async () => {
    const docs = await readDocs([join(base, "u", "proj")]);
    // The shared file (and its import) is read once per agent it reaches.
    expect(docs.map((d) => [d.cover.agent, d.cover.dir !== null])).toEqual([
      ["claude", false],
      ["claude", false],
      ["codex", false],
      ["codex", false],
      ["codex", true],
    ]);
    expect(docs[2]?.displayPath).toBe(
      "~/.codex/AGENTS.md → ~/.agents/AGENTS.md"
    );
    expect(docs[2]?.cover).toEqual({ agent: "codex", dir: null });
    expect(docs[0]?.displayPath).toBe(
      "~/.claude/CLAUDE.md → ~/.agents/AGENTS.md"
    );
    expect(docs[0]?.cover).toEqual({ agent: "claude", dir: null });
    expect(docs[0]?.mtimeMs).toBe(Date.parse("2026-08-01T12:00:00.000Z"));
    expect(docs[4]?.displayPath).toBe("~/proj/AGENTS.md");
    // A project AGENTS.md is Codex's file, never Claude's.
    expect(docs[4]?.cover).toEqual({
      agent: "codex",
      dir: join(base, "u", "proj"),
      codex: { exact: false, stops: [] },
    });
  });

  test("Claude's global and project .claude/rules dirs are read", async () => {
    const user = join(base, "u");
    const globalRules = join(user, ".claude", "rules");
    const projectRules = join(user, "proj", ".claude", "rules", "shell");
    mkdirSync(globalRules, { recursive: true });
    mkdirSync(projectRules, { recursive: true });
    writeFileSync(join(globalRules, "zsh.md"), "- Quote every glob.");
    writeFileSync(join(globalRules, "notes.txt"), "- Not a rule file.");
    writeFileSync(join(projectRules, "pkill.md"), "- pkill -x only.");
    try {
      const docs = await readDocs([join(user, "proj", "app")], ".claude", [
        "claude",
      ]);
      const ruleDocs = docs
        .filter((d) => d.displayPath.includes("/rules/"))
        .map((d) => [d.displayPath, d.cover]);
      expect(ruleDocs).toEqual([
        ["~/.claude/rules/zsh.md", { agent: "claude", dir: null }],
        [
          "~/proj/.claude/rules/shell/pkill.md",
          { agent: "claude", dir: join(user, "proj") },
        ],
      ]);
    } finally {
      rmSync(globalRules, { recursive: true, force: true });
      rmSync(join(user, "proj", ".claude"), { recursive: true, force: true });
    }
  });

  test("Codex walks up only to the repo root, or reads the cwd alone", async () => {
    const work = join(base, "u", "work");
    mkdirSync(join(work, "repo", "src", ".git"), { recursive: true });
    mkdirSync(join(work, "loose", "sub"), { recursive: true });
    writeFileSync(join(work, "AGENTS.md"), "- Above every repo.");
    writeFileSync(join(work, "loose", "AGENTS.md"), "- Above the cwd.");
    try {
      const inRepo = join(work, "repo", "src");
      const loose = join(work, "loose", "sub");
      const docs = await readDocs([inRepo, loose], ".claude", ["codex"]);
      // The repo root is `src` itself; `loose/sub` has no root, so no walk.
      expect(docs.filter((d) => d.cover.dir !== null)).toEqual([]);
      const pi = await readDocs([inRepo], ".claude", ["pi"]);
      expect(pi.map((d) => d.displayPath)).toContain("~/work/AGENTS.md");
    } finally {
      rmSync(work, { recursive: true, force: true });
    }
  });

  test("a Codex file reaches its repo's cwds, but not a nested repo's or a loose dir's", async () => {
    const work = join(base, "u", "nest");
    const inner = join(work, "outer", "vendor", "inner");
    mkdirSync(join(work, "outer", ".git"), { recursive: true });
    mkdirSync(join(inner, ".git"), { recursive: true });
    mkdirSync(join(work, "loose"), { recursive: true });
    writeFileSync(join(work, "outer", "AGENTS.md"), "- Outer rules.");
    writeFileSync(join(work, "loose", "AGENTS.md"), "- Loose rules.");
    try {
      const docs = await readDocs(
        [join(work, "outer"), inner, join(work, "loose")],
        ".claude",
        ["codex"]
      );
      const covers = docs
        .filter((d) => d.cover.dir !== null)
        .map((d) => [d.cover.dir, d.cover.codex]);
      expect(covers).toEqual([
        [join(work, "loose"), { exact: true, stops: [] }],
        [join(work, "outer"), { exact: false, stops: [inner] }],
      ]);
    } finally {
      rmSync(work, { recursive: true, force: true });
    }
  });

  test("each Claude source's global CLAUDE.md reaches only its own sessions", async () => {
    const user = join(base, "u");
    const work = join(user, ".claude-work");
    mkdirSync(work, { recursive: true });
    writeFileSync(join(work, "CLAUDE.md"), "- Work rules.");
    const plain = agentsAt(user);
    const agents = {
      roots: (id: AgentRoots["id"]): AgentRoots => {
        const roots = plain.roots(id);
        return id === "claude"
          ? {
              ...roots,
              sources: [
                {
                  id: "claude",
                  label: "default",
                  home: roots.home,
                  projectsRoot: roots.projectsRoot,
                  isDefault: true,
                },
                {
                  id: "work",
                  label: "work",
                  home: work,
                  projectsRoot: join(work, "projects"),
                  isDefault: false,
                },
              ],
            }
          : roots;
      },
    } as unknown as AgentRegistryShape;
    try {
      const docs = await Effect.runPromise(
        Effect.gen(function* () {
          const fs = yield* FileSystem.FileSystem;
          return yield* readMemoryDocs({
            fs,
            agents,
            scope: ["claude"],
            projectDirs: [],
          });
        }).pipe(Effect.provide(BunFileSystem.layer))
      );
      expect(
        docs.map((d) => [d.displayPath, d.cover.source?.id ?? null])
      ).toEqual([
        ["~/.claude/CLAUDE.md → ~/.agents/AGENTS.md", "claude"],
        ["~/.agents/skills/rm.md", "claude"],
        ["~/.claude-work/CLAUDE.md", "work"],
      ]);
    } finally {
      rmSync(work, { recursive: true, force: true });
    }
  });

  test("a repo outside the home keeps its root file, for Claude and Codex", async () => {
    const repo = join(base, "outside", "repo");
    const pkg = join(repo, "packages", "app");
    mkdirSync(pkg, { recursive: true });
    mkdirSync(join(repo, ".git"), { recursive: true });
    writeFileSync(join(repo, "CLAUDE.md"), "- Repo-root Claude rules.");
    writeFileSync(join(repo, "AGENTS.md"), "- Repo-root Codex rules.");
    try {
      const docs = await readDocs([pkg], ".claude", ["claude", "codex"]);
      const own = docs
        .filter((d) => d.realPath.startsWith(repo))
        .map((d) => [d.cover.agent, d.cover.dir]);
      expect(own).toContainEqual(["claude", repo]);
      expect(own).toContainEqual(["codex", repo]);
    } finally {
      rmSync(join(base, "outside"), { recursive: true, force: true });
    }
  });

  test("OpenCode reads its global AGENTS.md, else ~/.claude/CLAUDE.md", async () => {
    const fallback = await readDocs([], ".claude", ["opencode"]);
    expect(fallback.map((d) => [d.displayPath, d.cover])).toEqual([
      [
        "~/.claude/CLAUDE.md → ~/.agents/AGENTS.md",
        { agent: "opencode", dir: null },
      ],
      ["~/.agents/skills/rm.md", { agent: "opencode", dir: null }],
    ]);
    const config = join(base, "u", ".config", "opencode");
    mkdirSync(config, { recursive: true });
    writeFileSync(join(config, "AGENTS.md"), "- OpenCode global rules.");
    try {
      const own = await readDocs([], ".claude", ["opencode"]);
      expect(own.map((d) => d.displayPath)).toEqual([
        "~/.config/opencode/AGENTS.md",
      ]);
    } finally {
      rmSync(join(base, "u", ".config"), { recursive: true, force: true });
    }
  });

  test("each agent reads only its own project files; an override shadows", async () => {
    const docs = await readDocs([join(base, "u", "proj", "app")], ".claude", [
      "claude",
      "codex",
      "pi",
    ]);
    const shown = docs
      .filter((d) => d.cover.dir !== null)
      .map((d) => [d.displayPath, d.cover.agent]);
    expect(shown).toEqual([
      ["~/proj/AGENTS.md", "codex"],
      ["~/proj/AGENTS.md", "pi"],
      ["~/proj/app/CLAUDE.local.md", "claude"],
      ["~/proj/app/AGENTS.override.md", "codex"],
      ["~/proj/app/AGENTS.override.md", "pi"],
    ]);
  });

  test("Pi's global context file is read for Pi calls", async () => {
    const docs = await readDocs([], ".claude", ["pi"]);
    expect(docs.map((d) => [d.displayPath, d.cover])).toEqual([
      ["~/.pi/agent/AGENTS.md", { agent: "pi", dir: null }],
    ]);
  });

  test("an @import is read with its importer's reach; a code span is not one", async () => {
    const docs = await readDocs([]);
    expect(docs[1]?.displayPath).toBe("~/.agents/skills/rm.md");
    expect(docs[1]?.cover).toEqual({ agent: "claude", dir: null });
    expect(docs.some((d) => d.displayPath.includes("missing"))).toBe(false);
  });

  test("ancestors and CLAUDE.local.md of a project dir are read", async () => {
    const docs = await readDocs([join(base, "u", "proj", "app")]);
    const shown = docs.map((d) => [d.displayPath, d.cover.dir]);
    expect(shown).toContainEqual([
      "~/proj/app/CLAUDE.local.md",
      join(base, "u", "proj", "app"),
    ]);
    expect(shown).toContainEqual(["~/proj/AGENTS.md", join(base, "u", "proj")]);
  });

  test("a moved Claude home (CLAUDE_CONFIG_DIR) still finds the user's home", async () => {
    const docs = await readDocs(
      [join(base, "u", "proj", "app")],
      ".claude-work"
    );
    const shown = docs.map((d) => d.displayPath);
    expect(shown).toContain("~/proj/AGENTS.md");
    expect(shown).toContain("~/.agents/skills/rm.md");
  });
});

describe("matchRules", () => {
  let docs: readonly MemoryDoc[] = [];
  beforeAll(async () => {
    docs = await readDocs([join(base, "u", "proj", "app")]);
  });

  test("a probe matching one line wins over a window", () => {
    const asked: CoverSince[][] = [];
    const [checked] = matchRules({
      rows: [row({})],
      docs,
      counts: countsFor(asked),
      dateOf: byMtime,
      redact,
    });
    const rule = checked?.rule;
    expect(rule?.matches.map((m) => m.line)).toEqual([4]);
    expect(rule?.sinceIso).toBe("2026-08-01T12:00:00.000Z");
    // One line, listed once, though Claude and Codex both load the file.
    expect(asked.at(-1)).toEqual([
      { cover: { agent: "claude", dir: null }, afterDay: "2026-08-01" },
      { cover: { agent: "codex", dir: null }, afterDay: "2026-08-01" },
    ]);
    expect(rule?.per1kSince).toEqual({ main: 10, nested: 22.5 });
    expect(rule?.advice).toContain("setopt nonomatch");
    expect(rule?.advice).toContain("90% of hits come from subagents");
  });

  test("a file changed on the last day says what to do if it keeps firing", () => {
    const fresh: CauseCounts = {
      since: (_key, covers) =>
        covers.every((c) => c.afterDay === "")
          ? { hits: { main: 1, nested: 0 }, calls: { main: 10, nested: 0 } }
          : { hits: { main: 0, nested: 0 }, calls: { main: 0, nested: 0 } },
    };
    const [checked] = matchRules({
      rows: [row({})],
      docs,
      counts: fresh,
      dateOf: byMtime,
      redact,
    });
    expect(checked?.rule?.advice).toStartWith("if it keeps firing: ");
    expect(checked?.rule?.advice).toContain("setopt nonomatch");
  });

  test("a file that reaches none of the hits does not count", () => {
    const codexOnly: readonly MemoryDoc[] = [
      {
        displayPath: "~/.codex/AGENTS.md",
        realPath: "/c/AGENTS.md",
        cover: { agent: "codex", dir: null },
        mtimeMs: null,
        lines: ["- Quote every glob in a flag value."],
      },
    ];
    const [checked] = matchRules({
      rows: [row({})],
      docs: codexOnly,
      counts: claudeOnly,
      dateOf: byMtime,
      redact,
    });
    expect(checked?.rule).toBeNull();
  });

  test("a flag-glob line is not the path-glob rule; node_modules is not a module rule", () => {
    const rows = matchRules({
      rows: [
        row({ key: "cause:zsh-glob-path", cause: "zsh-glob-path" }),
        row({
          key: "cause:scratch-module-resolution",
          cause: "scratch-module-resolution",
        }),
      ],
      docs,
      counts: countsFor([]),
      dateOf: byMtime,
      redact,
    });
    expect(rows.map((r) => r.rule)).toEqual([null, null]);
  });

  test("a cause with no matching line, or no probe, has no rule", () => {
    const rows = matchRules({
      rows: [
        row({ key: "cause:ugrep-complexity", cause: "ugrep-complexity" }),
        row({ key: "cause:x", cause: "structured-output-schema" }),
      ],
      docs,
      counts: countsFor([]),
      dateOf: byMtime,
      redact,
    });
    expect(rows.map((r) => r.rule)).toEqual([null, null]);
  });

  test("a missing binary matches only a line that says it is missing", () => {
    const [checked] = matchRules({
      rows: [
        row({
          key: "cause:command-not-found",
          cause: "command-not-found",
          details: [
            { value: "jq", hits: 3 },
            { value: "file", hits: 1 },
          ],
        }),
      ],
      docs,
      counts: countsFor([]),
      dateOf: byMtime,
      redact,
    });
    expect(checked?.rule?.matches.map((m) => m.line)).toEqual([2]);
  });

  test("prose that says no file was found is not a missing-binary rule", () => {
    const prose = (lines: string[]): readonly MemoryDoc[] => [
      {
        displayPath: "~/x.md",
        realPath: "/x.md",
        cover: { agent: "claude", dir: null },
        mtimeMs: null,
        lines,
      },
    ];
    const check = (lines: string[]) =>
      matchRules({
        rows: [
          row({
            key: "cause:command-not-found",
            cause: "command-not-found",
            details: [{ value: "file", hits: 1 }],
          }),
        ],
        docs: prose(lines),
        counts: countsFor([]),
        dateOf: byMtime,
        redact,
      })[0]?.rule ?? null;
    expect(check(["- If the search finds no file, say so."])).toBeNull();
    expect(check(["- This box has no file: use stat."])).not.toBeNull();
    expect(check(["- There is no `file` binary here."])).not.toBeNull();
  });

  test("a binary's name must be a whole word of the line", () => {
    const check = (bin: string, line: string) =>
      matchRules({
        rows: [
          row({
            key: "cause:command-not-found",
            cause: "command-not-found",
            details: [{ value: bin, hits: 1 }],
          }),
        ],
        docs: [
          {
            displayPath: "~/x.md",
            realPath: "/x.md",
            cover: { agent: "claude", dir: null },
            mtimeMs: null,
            lines: [line],
          },
        ],
        counts: countsFor([]),
        dateOf: byMtime,
        redact,
      })[0]?.rule ?? null;
    expect(
      check("file", "- Never install files with sudo; ask first.")
    ).toBeNull();
    expect(
      check("file", "- Edit in place without filesystem-wide searches.")
    ).toBeNull();
    expect(
      check("node", "- Use bun instead of node_modules/.bin scripts.")
    ).toBeNull();
    expect(
      check("jq", "- Prefer python instead of jquery-style parsing.")
    ).toBeNull();
    expect(check("bc", "- Do not install bcrypt globally.")).toBeNull();
    expect(check("file", "- Never install file.txt fixtures.")).toBeNull();
    expect(check("jq", "- Use bun -e instead of jq.")).not.toBeNull();
    expect(check("rsync", "- Install `rsync` before syncing.")).not.toBeNull();
  });

  test("a loose line is not a rule: JS equality, git diff HEAD, the Glob tool", () => {
    const check = (over: Partial<CauseRow>, line: string) =>
      matchRules({
        rows: [row(over)],
        docs: [
          {
            displayPath: "~/x.md",
            realPath: "/x.md",
            cover: { agent: "claude", dir: null },
            mtimeMs: null,
            lines: [line],
          },
        ],
        counts: countsFor([]),
        dateOf: byMtime,
        redact,
      })[0]?.rule ?? null;
    const equals = {
      key: "cause:zsh-equals-abort",
      cause: "zsh-equals-abort",
    } as const;
    const output = {
      key: "cause:output-too-large",
      cause: "output-too-large",
    } as const;
    const path = {
      key: "cause:zsh-glob-path",
      cause: "zsh-glob-path",
    } as const;
    expect(check(equals, "- Use `===` and `!==`, never `==`.")).toBeNull();
    expect(
      check(equals, '- Quote separator echoes: `echo "==="`.')
    ).not.toBeNull();
    expect(
      check(output, "- Before committing, review with git diff HEAD.")
    ).toBeNull();
    expect(
      check(path, "- Use the Glob tool instead of find for file search.")
    ).toBeNull();
    expect(
      check(path, "- Not `ls dir/*.x` or `(N)` globs: zsh aborts on them.")
    ).not.toBeNull();
    // A git diff rule covers a row whose fix is the git diff one, not all output.
    const diffLine = "- Run `git diff --stat` first, then one file at a time.";
    expect(check(output, diffLine)).toBeNull();
    expect(
      check({ ...output, fix: GIT_DIFF_OUTPUT.fix }, diffLine)
    ).not.toBeNull();
    expect(
      check(output, "- Bound shell output: pipe it through | head -n 200.")
    ).not.toBeNull();
  });

  test("every file counts from the newest one's day, which the row shows", () => {
    const asked: CoverSince[][] = [];
    const doc = (path: string, day: string, dir: string | null): MemoryDoc => ({
      displayPath: path,
      realPath: path,
      cover: { agent: "claude", dir },
      mtimeMs: Date.parse(`${day}T12:00:00Z`),
      lines: ["Quote every glob passed as a flag value."],
    });
    const [checked] = matchRules({
      rows: [row({})],
      docs: [
        doc("~/.claude/CLAUDE.md", "2026-10-01", null),
        doc("~/proj/CLAUDE.md", "2026-10-21", "/u/proj"),
      ],
      counts: countsFor(asked),
      dateOf: byMtime,
      redact,
    });
    expect(checked?.rule?.sinceIso?.slice(0, 10)).toBe("2026-10-21");
    expect(checked?.rule?.matches[0]?.path).toBe("~/proj/CLAUDE.md");
    expect(asked.at(-1)?.map((c) => c.afterDay)).toEqual([
      "2026-10-21",
      "2026-10-21",
    ]);
  });

  test("a secret on a matched line is redacted in the excerpt", () => {
    const secretDocs: readonly MemoryDoc[] = [
      {
        displayPath: "~/x.md",
        realPath: "/x.md",
        cover: { agent: "claude", dir: null },
        mtimeMs: null,
        lines: [`Quote every glob; token ${SECRET}`],
      },
    ];
    const [checked] = matchRules({
      rows: [row({})],
      docs: secretDocs,
      counts: countsFor([]),
      dateOf: byMtime,
      redact,
    });
    expect(checked?.rule?.matches[0]?.excerpt).not.toContain(SECRET);
    expect(checked?.rule?.sinceIso).toBeNull();
  });

  test("a secret that straddles the excerpt's clip is redacted whole", () => {
    // Puts the key's first 16 chars inside the 160-char excerpt.
    const lead = `Quote every glob; ${"x".repeat(160 - 18 - 1 - 16)} `;
    const straddling: readonly MemoryDoc[] = [
      {
        displayPath: "~/x.md",
        realPath: "/x.md",
        cover: { agent: "claude", dir: null },
        mtimeMs: null,
        lines: [`${lead}${SECRET} end`],
      },
    ];
    const [checked] = matchRules({
      rows: [row({})],
      docs: straddling,
      counts: countsFor([]),
      dateOf: byMtime,
      redact,
    });
    const excerpt = checked?.rule?.matches[0]?.excerpt ?? "";
    expect(excerpt).not.toContain("sk-ant-");
    expect(excerpt).toContain("[REDACTED:");
  });
});

describe("dateLines", () => {
  const glob = "- Quote every glob passed as a flag value.";
  const doc = (lines: string[], day: string): MemoryDoc => ({
    displayPath: "~/.agents/AGENTS.md",
    realPath: "/u/.agents/AGENTS.md",
    cover: { agent: "claude", dir: null },
    mtimeMs: Date.parse(`${day}T12:00:00Z`),
    lines,
  });
  const now = Date.parse("2026-10-21T18:00:00Z");

  test("editing another line keeps a rule's date", () => {
    const first = dateLines({
      docs: [doc([glob], "2026-10-15")],
      ledger: EMPTY_LEDGER,
      now,
    });
    const edited = doc(["- A new line.", glob], "2026-10-21");
    const second = dateLines({ docs: [edited], ledger: first.next, now });
    expect(second.dateOf(edited, 1)).toBe(Date.parse("2026-10-15T12:00:00Z"));
    expect(second.dateOf(edited, 0)).toBe(Date.parse("2026-10-21T12:00:00Z"));
    const asked: CoverSince[][] = [];
    const [checked] = matchRules({
      rows: [row({})],
      docs: [edited],
      counts: countsFor(asked),
      dateOf: second.dateOf,
      redact,
    });
    expect(checked?.rule?.sinceIso?.slice(0, 10)).toBe("2026-10-15");
    expect(asked.at(-1)?.map((c) => c.afterDay)).toEqual(["2026-10-15"]);
  });

  test("the ledger stores hashes, and drops lines a file no longer holds", () => {
    const first = dateLines({
      docs: [doc([glob, `token ${SECRET}`], "2026-10-15")],
      ledger: EMPTY_LEDGER,
      now,
    });
    expect(JSON.stringify(first.next)).not.toContain("sk-ant");
    const second = dateLines({
      docs: [doc([glob], "2026-10-21")],
      ledger: first.next,
      now,
    });
    expect(
      Object.keys(second.next.files["/u/.agents/AGENTS.md"] ?? {})
    ).toHaveLength(1);
  });
});
