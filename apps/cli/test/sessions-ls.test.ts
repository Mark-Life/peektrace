/** `peektrace sessions ls`, run as a user runs it: a real process over a
 * throwaway home holding one synthetic Claude, Codex and Pi session each, with
 * known cwds and update times (Codex newest, Claude oldest). OpenCode's data
 * dir is empty. */
import { afterAll, beforeAll, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";

const ENTRY = join(import.meta.dir, "..", "src", "index.ts");

const CLAUDE_ID = "11111111-1111-4111-8111-111111111111";
const CODEX_ID = "22222222-2222-4222-8222-222222222222";
const PI_ID = "33333333-3333-4333-8333-333333333333";

const CLAUDE_CWD = "/home/demo/work";
const PI_CWD = "/home/demo/work/sub";
const CODEX_CWD = "/home/demo/other";

const CLAUDE_AT = "2026-03-01T10:00:00.000Z";
const PI_AT = "2026-04-15T10:00:00.000Z";
const CODEX_AT = "2026-06-01T10:00:00.000Z";

let home = "";

const write = (path: string, lines: readonly unknown[]) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${lines.map((l) => JSON.stringify(l)).join("\n")}\n`);
};

beforeAll(() => {
  home = mkdtempSync(join(tmpdir(), "peektrace-ls-"));
  write(
    join(home, ".claude", "projects", "-home-demo-work", `${CLAUDE_ID}.jsonl`),
    [
      {
        type: "user",
        cwd: CLAUDE_CWD,
        timestamp: CLAUDE_AT,
        message: { content: "hello claude" },
      },
    ]
  );
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
        timestamp: CODEX_AT,
        type: "session_meta",
        payload: { id: CODEX_ID, timestamp: CODEX_AT, cwd: CODEX_CWD },
      },
      {
        timestamp: CODEX_AT,
        type: "response_item",
        payload: {
          type: "message",
          role: "user",
          content: [{ type: "input_text", text: "hello codex" }],
        },
      },
    ]
  );
  write(
    join(
      home,
      ".pi",
      "agent",
      "sessions",
      "--home-demo-work-sub--",
      `2026-04-15T10-00-00-000Z_${PI_ID}.jsonl`
    ),
    [
      { type: "session", version: 3, id: PI_ID, timestamp: PI_AT, cwd: PI_CWD },
      {
        type: "message",
        id: "a1000000",
        parentId: null,
        timestamp: PI_AT,
        message: {
          role: "user",
          content: [{ type: "text", text: "hello pi" }],
        },
      },
    ]
  );
});

afterAll(() => {
  rmSync(home, { recursive: true, force: true });
});

const run = (...args: readonly string[]) => {
  const proc = Bun.spawnSync(["bun", ENTRY, "sessions", "ls", ...args], {
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

interface Header {
  readonly agent: string;
  readonly id: string;
}

const ids = (out: string) => (JSON.parse(out) as Header[]).map((h) => h.id);

/** Data rows of the tab-separated table, header excluded. */
const dataRows = (out: string) =>
  out
    .trim()
    .split("\n")
    .slice(1)
    .map((line) => line.split("\t"));

test("no --agent lists every agent, newest first, with an AGENT column", () => {
  const { code, out } = run();
  expect(code).toBe(0);
  expect(out.split("\n")[0]?.split("\t").slice(0, 2)).toEqual(["id", "agent"]);
  const rows = dataRows(out);
  expect(rows.map((r) => r[1])).toEqual(["codex", "pi", "claude"]);
}, 60_000);

test("the table id is the first uuid segment, for a bare-uuid agent too", () => {
  const rows = dataRows(run().out);
  expect(rows[0]?.[0]).toBe(CODEX_ID.split("-")[0]);
}, 60_000);

test("--agent narrows to one agent", () => {
  for (const [agent, id] of [
    ["codex", CODEX_ID],
    ["pi", PI_ID],
    ["claude", CLAUDE_ID],
  ] as const) {
    const { code, out } = run("--agent", agent, "--json");
    expect(code).toBe(0);
    const headers = JSON.parse(out) as Header[];
    expect(headers.map((h) => h.id)).toEqual([id]);
    expect(headers.every((h) => h.agent === agent)).toBe(true);
  }
}, 60_000);

test("--agent opencode with no sessions is empty, not an error", () => {
  const json = run("--agent", "opencode", "--json");
  expect(json.code).toBe(0);
  expect(JSON.parse(json.out)).toEqual([]);
  const table = run("--agent", "opencode");
  expect(table.code).toBe(0);
  expect(dataRows(table.out)).toEqual([["(none)"]]);
}, 60_000);

test("--agent all equals no --agent", () => {
  expect(ids(run("--agent", "all", "--json").out)).toEqual(
    ids(run("--json").out)
  );
}, 60_000);

test("an unknown agent fails and lists the known ones", () => {
  const { code, err } = run("--agent", "bogus");
  expect(code).not.toBe(0);
  expect(err).toContain(
    'Unknown agent "bogus". Known: claude, codex, pi, opencode, all.'
  );
}, 60_000);

test("--limit caps rows and hints on stderr only", () => {
  const { code, err, out } = run("--limit", "1");
  expect(code).toBe(0);
  expect(dataRows(out)).toHaveLength(1);
  expect(out).not.toContain("hidden");
  expect(err).toContain("2 more hidden, use --limit 0 to show all");
}, 60_000);

test("--limit with --json stays valid JSON", () => {
  const { err, out } = run("--limit", "1", "--json");
  expect(ids(out)).toEqual([CODEX_ID]);
  expect(err).not.toContain("hidden");
}, 60_000);

test("--limit 0 shows all and a negative limit fails", () => {
  expect(dataRows(run("--limit", "0").out)).toHaveLength(3);
  const bad = run("--limit", "-1");
  expect(bad.code).not.toBe(0);
  expect(bad.err).toContain("Invalid --limit -1");
}, 60_000);

test("--since keeps only sessions updated on or after the date", () => {
  expect(ids(run("--since", "2026-04-01", "--json").out)).toEqual([
    CODEX_ID,
    PI_ID,
  ]);
  expect(ids(run("--since", "2026-05-01", "--json").out)).toEqual([CODEX_ID]);
  expect(JSON.parse(run("--since", "2027-01-01", "--json").out)).toEqual([]);
}, 60_000);

test("a bad --since fails with the accepted forms", () => {
  const { code, err } = run("--since", "nope");
  expect(code).not.toBe(0);
  expect(err).toContain('Invalid --since "nope"');
}, 60_000);

test("--cwd matches the directory and anything below it", () => {
  expect(ids(run("--cwd", CODEX_CWD, "--json").out)).toEqual([CODEX_ID]);
  expect(ids(run("--cwd", CLAUDE_CWD, "--json").out)).toEqual([
    PI_ID,
    CLAUDE_ID,
  ]);
  expect(ids(run("--cwd", `${CLAUDE_CWD}/`, "--json").out)).toEqual([
    PI_ID,
    CLAUDE_ID,
  ]);
  expect(JSON.parse(run("--cwd", "/home/demo/wor", "--json").out)).toEqual([]);
}, 60_000);

test("an empty result prints the header and a (none) line", () => {
  const { code, out } = run("--cwd", "/nowhere");
  expect(code).toBe(0);
  expect(dataRows(out)).toEqual([["(none)"]]);
}, 60_000);
