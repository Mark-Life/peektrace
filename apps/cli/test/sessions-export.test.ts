/** `peektrace sessions export`, run as a real process over a throwaway home:
 * one Claude session whose text carries fake secrets and a home path, and one
 * newer Codex session. */
import { afterAll, beforeAll, expect, test } from "bun:test";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import {
  claudePath,
  ENTRY,
  envFor,
  makeHome,
  removeHome,
  write,
} from "./support/synthetic-home";

const CLAUDE_ID = "11111111-1111-4111-8111-111111111111";
const CODEX_ID = "33333333-3333-4333-8333-333333333333";
const CWD = "/home/demo/work";
const ANTHROPIC_KEY = `sk-ant-${"a1B2c3D4e5".repeat(5)}`;
const GITHUB_TOKEN = `ghp_${"x9Y8z7W6v5".repeat(3)}abcdef`;

let home = "";

beforeAll(() => {
  home = makeHome("export");
  const at = "2026-03-01T10:00:00.000Z";
  write(claudePath(home, "-home-demo-work", CLAUDE_ID), [
    {
      type: "user",
      cwd: CWD,
      timestamp: at,
      message: {
        content: `use key ${ANTHROPIC_KEY} and ${GITHUB_TOKEN} in ${CWD}/a.ts <b>x</b>`,
      },
    },
    {
      type: "assistant",
      cwd: CWD,
      timestamp: at,
      requestId: "req-1",
      message: {
        model: "claude-opus-4-1",
        content: [{ type: "text", text: "done" }],
        usage: {
          input_tokens: 1000,
          cache_read_input_tokens: 0,
          cache_creation_input_tokens: 0,
          output_tokens: 10,
        },
      },
    },
  ]);
  write(
    join(
      home,
      ".codex",
      "sessions",
      "2026",
      "06",
      "01",
      `rollout-2026-06-01T10-00-00-${CODEX_ID}.jsonl`
    ),
    [
      {
        timestamp: "2026-06-01T10:00:00.000Z",
        type: "session_meta",
        payload: {
          id: CODEX_ID,
          timestamp: "2026-06-01T10:00:00.000Z",
          cwd: "/home/demo/other",
        },
      },
      {
        timestamp: "2026-06-01T10:00:00.000Z",
        type: "response_item",
        payload: {
          type: "message",
          role: "user",
          content: [{ type: "input_text", text: "hello codex" }],
        },
      },
    ]
  );
});

afterAll(() => removeHome(home));

const run = (...args: readonly string[]) => {
  const proc = Bun.spawnSync(["bun", ENTRY, "sessions", "export", ...args], {
    env: envFor(home),
  });
  return {
    code: proc.exitCode,
    err: proc.stderr.toString(),
    out: proc.stdout.toString(),
  };
};

test("each format prints to stdout", () => {
  const html = run(CLAUDE_ID, "--format", "html");
  expect(html.code).toBe(0);
  expect(html.out.toLowerCase()).toContain("<!doctype html");
  expect(html.out).toContain("&lt;b&gt;x&lt;/b&gt;");

  const md = run(CLAUDE_ID, "--format", "md");
  expect(md.code).toBe(0);
  expect(md.out.startsWith("# ")).toBe(true);

  const json = run(CLAUDE_ID, "--format", "json");
  expect(json.code).toBe(0);
  expect(JSON.parse(json.out).schema).toBe("peektrace.session-export");
}, 60_000);

test("default is redacted markdown", () => {
  const { code, out } = run(CLAUDE_ID);
  expect(code).toBe(0);
  expect(out.startsWith("# ")).toBe(true);
  expect(out).toContain("[REDACTED");
  expect(out).not.toContain(ANTHROPIC_KEY);
  expect(out).not.toContain(GITHUB_TOKEN);
  expect(out).not.toContain("/home/demo");
  expect(out).not.toContain(home);
}, 60_000);

test("--no-redact keeps the text", () => {
  const { code, out } = run(CLAUDE_ID, "--no-redact");
  expect(code).toBe(0);
  expect(out).toContain(ANTHROPIC_KEY);
  expect(out).toContain(GITHUB_TOKEN);
  expect(out).not.toContain(home);
}, 60_000);

test("--json alone gives JSON, before or after the subcommand", () => {
  expect(JSON.parse(run(CLAUDE_ID, "--json").out).version).toBe(1);
  const proc = Bun.spawnSync(
    ["bun", ENTRY, "--json", "sessions", "export", CLAUDE_ID],
    { env: envFor(home) }
  );
  expect(JSON.parse(proc.stdout.toString()).version).toBe(1);
}, 60_000);

test("--out writes the file, infers the format from its extension", () => {
  const target = join(home, "out.html");
  const { code, out, err } = run(CLAUDE_ID, "--out", target);
  expect(code).toBe(0);
  expect(out).toBe("");
  expect(err).toContain(`Wrote ${target}`);
  expect(existsSync(target)).toBe(true);
  expect(readFileSync(target, "utf8").toLowerCase()).toContain(
    "<!doctype html"
  );
}, 60_000);

test("--format wins over the --out extension", () => {
  const target = join(home, "x.html");
  run(CLAUDE_ID, "--format", "json", "--out", target);
  expect(JSON.parse(readFileSync(target, "utf8")).version).toBe(1);
}, 60_000);

test("--latest exports a non-Claude session", () => {
  const { code, out } = run("--latest", "--format", "json");
  expect(code).toBe(0);
  const doc = JSON.parse(out);
  expect(doc.session.sessionId).toBe(CODEX_ID);
  expect(doc.session.provider).toBe("codex");
  expect(out).not.toContain(home);
}, 60_000);

test("unknown id fails with a message", () => {
  const { code, err } = run("99999999-9999-4999-8999-999999999999");
  expect(code).not.toBe(0);
  expect(err.length).toBeGreaterThan(0);
}, 60_000);

test("a bad --format is rejected", () => {
  const { code, out } = run(CLAUDE_ID, "--format", "pdf");
  expect(code).not.toBe(0);
  expect(out).not.toContain("# ");
}, 60_000);

test("no id and no flag fails with usage", () => {
  const { code, err } = run();
  expect(code).toBe(1);
  expect(err).toContain("Pass a session id, --latest, or --cwd <dir>");
}, 60_000);

test("--out - prints to stdout and writes no file", () => {
  const { code, out, err } = run(CLAUDE_ID, "--out", "-");
  expect(code).toBe(0);
  expect(out.startsWith("# ")).toBe(true);
  expect(err).not.toContain("Wrote");
}, 60_000);

test("--out into a missing directory fails with one clear line", () => {
  const target = join(home, "no-such-dir", "x.md");
  const { code, err } = run(CLAUDE_ID, "--out", target);
  expect(code).toBe(1);
  expect(err).toContain("Cannot write");
  expect(err).toContain("the directory does not exist");
  expect(err).not.toContain("ENOENT");
  expect(err.split(target).length - 1).toBe(1);
}, 60_000);
