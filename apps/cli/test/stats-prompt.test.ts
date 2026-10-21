/** `peektrace stats prompt`, run as a user runs it: a real process over a
 * throwaway home holding one Claude session, where a zsh glob abort is retried
 * with the glob quoted. The prompt must name that call by transcript path and
 * tool-use id, and a key that names no row must fail loudly. */
import { afterAll, beforeAll, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const SLUG = "-home-demo-work";
const SID = "33333333-3333-4333-8333-333333333333";
const ENTRY = join(import.meta.dir, "..", "src", "index.ts");

/** How long before now the session starts, so it sits inside any window. */
const MINUTE_MS = 60_000;

const GLOBBED = "grep -rn needle src --include=*.ts | head -3";
const QUOTED = "grep -rn needle src --include='*.ts' | head -3";

/** One assistant tool call and its result, a second apart. */
const call = (args: {
  readonly at: number;
  readonly command: string;
  readonly id: string;
  readonly output: string;
}) => [
  {
    type: "assistant",
    requestId: `req-${args.id}`,
    timestamp: new Date(args.at).toISOString(),
    message: {
      model: "claude-opus-4",
      usage: {
        input_tokens: 10,
        cache_read_input_tokens: 2000,
        cache_creation_input_tokens: 0,
        output_tokens: 5,
      },
      content: [
        {
          type: "tool_use",
          id: args.id,
          name: "Bash",
          input: { command: args.command },
        },
      ],
    },
  },
  {
    type: "user",
    timestamp: new Date(args.at + 1000).toISOString(),
    message: {
      content: [
        { type: "tool_result", tool_use_id: args.id, content: args.output },
      ],
    },
  },
];

let home = "";
let transcript = "";

beforeAll(() => {
  home = mkdtempSync(join(tmpdir(), "peektrace-prompt-"));
  const dir = join(home, ".claude", "projects", SLUG);
  mkdirSync(dir, { recursive: true });
  const start = Date.now() - MINUTE_MS;
  const lines = [
    {
      type: "user",
      cwd: "/home/demo/work",
      timestamp: new Date(start).toISOString(),
      message: { content: "find the needle" },
    },
    ...call({
      at: start + 1000,
      command: GLOBBED,
      id: "toolu_glob",
      output: "(eval):1: no matches found: --include=*.ts",
    }),
    ...call({
      at: start + 5000,
      command: QUOTED,
      id: "toolu_quoted",
      output: "src/a.ts:3:needle",
    }),
  ];
  transcript = join(dir, `${SID}.jsonl`);
  writeFileSync(
    transcript,
    `${lines.map((line) => JSON.stringify(line)).join("\n")}\n`
  );
});

afterAll(() => {
  rmSync(home, { recursive: true, force: true });
});

/** Run the CLI against the throwaway home only. */
const run = (args: readonly string[], sub = "prompt") => {
  const proc = Bun.spawnSync(["bun", ENTRY, "stats", sub, ...args], {
    env: {
      PATH: process.env.PATH,
      HOME: home,
      CLAUDE_CONFIG_DIR: join(home, ".claude"),
      CODEX_HOME: join(home, ".codex"),
      XDG_DATA_HOME: join(home, ".local", "share"),
      PEEKTRACE_DIR: join(home, ".peektrace"),
      PEEKTRACE_NO_TELEMETRY: "1",
    },
  });
  return {
    code: proc.exitCode,
    err: proc.stderr.toString(),
    out: proc.stdout.toString(),
  };
};

test("a cause prompt names the failing call by transcript path and id", () => {
  const { code, out } = run([
    "--cause",
    "zsh-glob-flag",
    "--days",
    "7",
    "--agent",
    "claude",
  ]);
  expect(code).toBe(0);
  expect(out.startsWith("Goal: ")).toBe(true);
  expect(out).toContain("(last 7 days, claude)");
  expect(out).toContain("- cause: zsh-glob-flag");
  expect(out).toContain(`· toolu_glob · ${transcript}`);
  expect(out).toContain(`  failed: ${GLOBBED}`);
  expect(out).toContain(`  retry: ${QUOTED}`);
  expect(out).toContain(
    "peektrace stats drill --days 7 --agent claude --cause zsh-glob-flag --json"
  );
}, 60_000);

test("the drill command the prompt prints finds the row and its transcripts", () => {
  const { code, out } = run(
    ["--days", "7", "--agent", "claude", "--cause", "zsh-glob-flag", "--json"],
    "drill"
  );
  expect(code).toBe(0);
  const result = JSON.parse(out) as {
    readonly hits: readonly { readonly sid: string }[];
    readonly key: string;
    readonly paths: Record<string, string>;
  };
  expect(result.key).toBe("cause:zsh-glob-flag");
  expect(result.hits.length).toBeGreaterThan(0);
  expect(Object.values(result.paths)).toContain(transcript);
}, 60_000);

test("--limit reaches the rows the report lists past the default", () => {
  const { code } = run([
    "--cause",
    "zsh-glob-flag",
    "--days",
    "7",
    "--limit",
    "50",
  ]);
  expect(code).toBe(0);
}, 60_000);

test("no flag prints the whole-report prompt", () => {
  const { code, out } = run(["--days", "7"]);
  expect(code).toBe(0);
  expect(out).toContain("  - zsh-glob-flag: 1 hits");
  expect(out).toContain("- full report: peektrace stats --days 7 --json");
}, 60_000);

test("a key that names no row fails and says how to list keys", () => {
  const { code, err } = run(["--fail", "nope", "--days", "7"]);
  expect(code).not.toBe(0);
  expect(err).toContain('No fail row "nope"');
}, 60_000);

test("two targets at once is a usage error", () => {
  const { code, err } = run(["--cause", "a", "--time", "b"]);
  expect(code).not.toBe(0);
  expect(err).toContain("at most one of --cause, --fail or --time");
}, 60_000);
