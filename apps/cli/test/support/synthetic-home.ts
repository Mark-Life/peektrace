/** Throwaway home with synthetic sessions, plus a runner for the real CLI process. */
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

export const ENTRY = join(import.meta.dir, "..", "..", "src", "index.ts");

export const write = (path: string, lines: readonly unknown[]) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${lines.map((l) => JSON.stringify(l)).join("\n")}\n`);
};

export const makeHome = (prefix: string) =>
  mkdtempSync(join(tmpdir(), `peektrace-${prefix}-`));

export const removeHome = (home: string) =>
  rmSync(home, { recursive: true, force: true });

/** Clean env: nothing from the developer's machine can leak in. */
export const envFor = (home: string) => ({
  PATH: process.env.PATH,
  HOME: home,
  CLAUDE_CONFIG_DIR: join(home, ".claude"),
  CODEX_HOME: join(home, ".codex"),
  XDG_DATA_HOME: join(home, ".local", "share"),
  PEEKTRACE_DIR: join(home, ".peektrace"),
  PEEKTRACE_NO_TELEMETRY: "1",
});

/** Claude transcript: a user line then one assistant turn using `contextTokens`. */
export const claudeLines = (opts: {
  readonly at: string;
  readonly contextTokens: number;
  readonly cwd: string;
  readonly model?: string;
}) => [
  {
    type: "user",
    cwd: opts.cwd,
    timestamp: opts.at,
    message: { content: "hello" },
  },
  {
    type: "assistant",
    cwd: opts.cwd,
    timestamp: opts.at,
    requestId: "req-1",
    message: {
      model: opts.model ?? "claude-opus-4-1",
      content: [{ type: "text", text: "hi" }],
      usage: {
        input_tokens: opts.contextTokens,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
        output_tokens: 10,
      },
    },
  },
];

export const claudePath = (home: string, slug: string, id: string) =>
  join(home, ".claude", "projects", slug, `${id}.jsonl`);
