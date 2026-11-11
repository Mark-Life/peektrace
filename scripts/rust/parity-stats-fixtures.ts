/**
 * Fixture HOME for the `stats` parity stage: the golden project, every
 * `sessions-claude` transcript, and an edge tree built here (nested and
 * workflow agents, a journal, an empty and an unreadable file, CRLF output,
 * non-ASCII commands) plus memory files the rule check reads.
 */
import {
  chmodSync,
  cpSync,
  mkdirSync,
  readdirSync,
  symlinkSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { REPO } from "./parity-lib";

/** Every file gets this mtime, so ledger dates and cache keys are stable. */
const FIXED_MTIME = new Date("2026-11-11T16:42:21.851Z");
const NO_ACCESS = 0o000;
const READ_WRITE = 0o644;

const write = (path: string, body: string) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, body);
};

interface Step {
  readonly cmd?: string;
  readonly err?: boolean;
  readonly gap?: number;
  readonly input?: Record<string, unknown>;
  readonly out: string;
  readonly tool?: string;
}

/** A Claude transcript of tool calls, one model call per step. */
const transcript = (
  cwd: string,
  steps: readonly Step[],
  opts: { readonly sidechain?: boolean; readonly start?: number } = {}
): string => {
  const side = opts.sidechain === true;
  let t = opts.start ?? Date.parse("2026-10-20T09:00:00.000Z");
  const at = () => new Date(t).toISOString();
  const lines: unknown[] = [
    {
      type: "user",
      cwd,
      isSidechain: side,
      timestamp: at(),
      message: { role: "user", content: "run the checks" },
    },
  ];
  steps.forEach((s, i) => {
    t += 5000;
    const id = `toolu_${String(i).padStart(3, "0")}`;
    lines.push({
      type: "assistant",
      requestId: `req_${i}`,
      isSidechain: side,
      cwd,
      timestamp: at(),
      message: {
        model: i % 2 === 0 ? "claude-opus-4-5" : "claude-sonnet-4-5",
        usage: {
          input_tokens: 3,
          cache_read_input_tokens: 1000 * (i + 1),
          cache_creation_input_tokens: 200,
          output_tokens: 20,
        },
        content: [
          {
            type: "tool_use",
            id,
            name: s.tool ?? "Bash",
            input: s.input ?? { command: s.cmd ?? "" },
          },
        ],
      },
    });
    t += (s.gap ?? 2) * 1000;
    lines.push({
      type: "user",
      isSidechain: side,
      timestamp: at(),
      message: {
        role: "user",
        content: [
          {
            type: "tool_result",
            tool_use_id: id,
            content: s.out,
            is_error: s.err === true,
          },
        ],
      },
    });
  });
  return `${lines.map((l) => JSON.stringify(l)).join("\n")}\n`;
};

const SHELL_STEPS: readonly Step[] = [
  {
    cmd: "rg -n todo src -g *.ts",
    out: "Exit code 1\r\n(eval):1: no matches found: *.ts\r\n",
    err: true,
  },
  { cmd: "rg -n todo src -g '*.ts'", out: "src/a.ts:1:todo\r\n" },
  {
    cmd: "ls build/*.zip",
    out: "zsh:1: no matches found: build/*.zip",
    err: true,
  },
  { cmd: "find build -name '*.zip'", out: "" },
  {
    cmd: "jqq . data.json",
    out: "Exit code 127\nzsh:1: command not found: jqq",
    err: true,
  },
  { cmd: "jq . data.json", out: "{}" },
  {
    cmd: "echo === done",
    out: "Exit code 1\n(eval):1: == not found",
    err: true,
  },
  { cmd: 'echo "===" done', out: "=== done" },
  {
    cmd: "sleep 30 && curl -s localhost:3000/health",
    out: "Blocked: sleep 30 followed by: curl -s localhost:3000/health. To wait for a condition, use Monitor.",
    err: true,
  },
  {
    cmd: "until curl -s localhost:3000/health; do sleep 2; done",
    out: "ok",
    gap: 121,
  },
  { cmd: "pkill -f 'bun run dev'", out: "Exit code 144", err: true },
  { cmd: "pkill -x bun", out: "" },
  {
    cmd: "bun run typecheck",
    out: "src/a.ts(3,1): error TS2304: Cannot find name 'x'.",
  },
  { cmd: "bun test | tail -5", out: " 3 pass\n 2 fail\n" },
  {
    cmd: "rm -rf $DIR/*",
    out: "Dangerous rm operation detected: refusing",
    err: true,
  },
  { cmd: "bun run build &", out: "started", gap: 600 },
  { cmd: "grep -rn 'naïve café' docs", out: "docs/a.md:1:naïve café ☕" },
  { cmd: "echo 日本語 > notes.txt && cat notes.txt", out: "日本語" },
  {
    cmd: "cat huge.log",
    out: "<persisted-output>\nOutput too large (2.1MB). Full output saved to: /tmp/claude/out-1.txt\n</persisted-output>",
  },
  {
    tool: "Read",
    input: { file_path: "/tmp/claude/out-1.txt" },
    out: "line 1\nline 2",
  },
  {
    tool: "Edit",
    input: { file_path: "/work/edge/a.ts", old_string: "x", new_string: "y" },
    out: "<tool_use_error>File has not been read yet. Read it first before writing to it.</tool_use_error>",
    err: true,
  },
  {
    tool: "Read",
    input: { file_path: "/work/edge/a.ts" },
    out: "x",
  },
  {
    tool: "Edit",
    input: { file_path: "/work/edge/a.ts", old_string: "x", new_string: "y" },
    out: "ok",
  },
  {
    tool: "WebFetch",
    input: { url: "https://example.com/docs", prompt: "read" },
    out: "docs",
  },
  {
    tool: "WebFetch",
    input: { url: "https://example.com/docs", prompt: "read" },
    out: "docs again",
  },
  {
    cmd: "node /tmp/scratch/check.mjs",
    out: "error: Cannot find module 'effect' from '/tmp/scratch/check.mjs'",
    err: true,
  },
  {
    cmd: "git diff",
    out: "<persisted-output>\nOutput too large (1MB). Full output saved to: /tmp/claude/out-2.txt\n</persisted-output>",
  },
  { cmd: "git diff --stat", out: " a.ts | 2 +-" },
];

const SUB_STEPS: readonly Step[] = [
  { cmd: "ls dist/*.js", out: "zsh:1: no matches found: dist/*.js", err: true },
  { cmd: "ls dist", out: "a.js" },
  {
    cmd: "fdd src",
    out: "Exit code 127\nzsh:1: command not found: fdd",
    err: true,
  },
  {
    cmd: "bun test src/x.test.ts",
    out: "FAIL src/x.test.ts\n 1 fail",
    err: true,
  },
];

const MEMORY = [
  "# Rules",
  "",
  "- Shell is zsh: quote every glob passed as a flag value, e.g. --include='*.ts'.",
  "- Never sleep N && cmd: use an until-loop, Monitor or run_in_background.",
  `- rm absolute paths only: rm -rf -- "$\{D:?}"/x`.replace("\\", ""),
  "- jq is installed; there is no jqq binary.",
  "",
  "@./more.md",
  "",
  "```",
  "@./not-an-import.md",
  "```",
].join("\n");

const MORE = [
  "- Read a file with the Read tool before you Edit or Write it.",
  "- pkill -f kills the shell: use pkill -x NAME.",
].join("\n");

const touchAll = (dir: string) => {
  for (const name of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, name.name);
    if (name.isDirectory()) {
      touchAll(path);
    }
    if (!name.isSymbolicLink()) {
      utimesSync(path, FIXED_MTIME, FIXED_MTIME);
    }
  }
};

/** Build the stats fixture HOME under `root`; returns HOME. */
export const buildStatsHome = (root: string): string => {
  const home = join(root, "stats-home");
  const projects = join(home, ".claude/projects");
  const fixtures = join(REPO, "packages/core/test/fixtures");
  cpSync(
    join(fixtures, "sessions/projects/-Users-demo-proj"),
    join(projects, "-Users-demo-proj"),
    { recursive: true }
  );
  const stats = join(projects, "-Users-demo-stats");
  mkdirSync(stats, { recursive: true });
  for (const name of readdirSync(join(fixtures, "sessions-claude"))) {
    cpSync(join(fixtures, "sessions-claude", name), join(stats, name));
  }
  const cwd = join(home, "work/edge");
  const edge = join(projects, "-work-edge");
  const sid = "22222222-2222-4222-8222-222222222222";
  write(join(edge, `${sid}.jsonl`), transcript(cwd, SHELL_STEPS));
  write(
    join(edge, sid, "subagents/agent-a1.jsonl"),
    transcript(cwd, SUB_STEPS, { sidechain: true })
  );
  write(
    join(edge, sid, "subagents/workflows/wf_x/agent-w1.jsonl"),
    transcript(cwd, SUB_STEPS, { sidechain: true })
  );
  write(
    join(edge, sid, "subagents/workflows/wf_x/agent-w2.jsonl"),
    transcript(cwd, SUB_STEPS.slice(0, 2), { sidechain: true })
  );
  write(join(edge, sid, "subagents/journal.jsonl"), '{"type":"note"}\n');
  write(join(edge, "memory/x.jsonl"), transcript(cwd, SUB_STEPS.slice(2)));
  write(join(edge, "33333333-3333-4333-8333-333333333333.jsonl"), "");
  const locked = join(edge, "44444444-4444-4444-8444-444444444444.jsonl");
  write(locked, transcript(cwd, SUB_STEPS));
  write(join(home, ".claude/CLAUDE.md"), MEMORY);
  write(join(home, ".claude/more.md"), MORE);
  write(
    join(home, ".claude/rules/a.md"),
    "- Re-read a file after a formatter touched it.\n"
  );
  write(
    join(cwd, "CLAUDE.md"),
    "- Copy old_string from a fresh Read; Edit needs exact lines.\n"
  );
  write(
    join(home, "shared/rules.md"),
    "- ugrep is grep here: use command grep for big patterns.\n"
  );
  symlinkSync(
    join(home, "shared/rules.md"),
    join(home, ".claude/rules/link.md")
  );
  touchAll(home);
  chmodSync(locked, NO_ACCESS);
  return home;
};

/** Undo the unreadable fixture so the work dir can be removed. */
export const unlockStatsHome = (home: string) => {
  const locked = join(
    home,
    ".claude/projects/-work-edge/44444444-4444-4444-8444-444444444444.jsonl"
  );
  try {
    chmodSync(locked, READ_WRITE);
  } catch {
    // already gone
  }
};
