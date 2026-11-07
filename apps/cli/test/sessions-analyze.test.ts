/** `peektrace sessions analyze --latest / --cwd`, run as a real process over a
 * throwaway home: two Claude sessions (different cwds) and one newer Codex session. */
import { afterAll, beforeAll, expect, test } from "bun:test";
import { join } from "node:path";
import {
  claudeLines,
  claudePath,
  ENTRY,
  envFor,
  makeHome,
  removeHome,
  write,
} from "./support/synthetic-home";

const OLD_ID = "11111111-1111-4111-8111-111111111111";
const MID_ID = "22222222-2222-4222-8222-222222222222";
const CODEX_ID = "33333333-3333-4333-8333-333333333333";
const SUB_ID = "agent-x";

const OLD_CWD = "/home/demo/work";
const MID_CWD = "/home/demo/work/sub";
const CODEX_CWD = "/home/demo/other";

let home = "";

beforeAll(() => {
  home = makeHome("analyze");
  write(
    claudePath(home, "-home-demo-work", OLD_ID),
    claudeLines({
      at: "2026-03-01T10:00:00.000Z",
      contextTokens: 20_000,
      cwd: OLD_CWD,
    })
  );
  write(
    claudePath(home, "-home-demo-work-sub", MID_ID),
    claudeLines({
      at: "2026-04-01T10:00:00.000Z",
      contextTokens: 40_000,
      cwd: MID_CWD,
    })
  );
  // Subagent transcript newer than its parent: `list()` must not surface it.
  write(
    join(
      home,
      ".claude",
      "projects",
      "-home-demo-work-sub",
      MID_ID,
      "subagents",
      `${SUB_ID}.jsonl`
    ),
    claudeLines({
      at: "2026-05-01T10:00:00.000Z",
      contextTokens: 99_000,
      cwd: MID_CWD,
    })
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
        timestamp: "2026-06-01T10:00:00.000Z",
        type: "session_meta",
        payload: {
          id: CODEX_ID,
          timestamp: "2026-06-01T10:00:00.000Z",
          cwd: CODEX_CWD,
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
  const proc = Bun.spawnSync(["bun", ENTRY, "sessions", "analyze", ...args], {
    env: envFor(home),
  });
  return {
    code: proc.exitCode,
    err: proc.stderr.toString(),
    out: proc.stdout.toString(),
  };
};

const sessionOf = (out: string) =>
  (JSON.parse(out) as { sessionId: string }).sessionId;

test("--latest picks the newest session of any agent", () => {
  const { code, out } = run("--latest", "--json");
  expect(code).toBe(0);
  expect(sessionOf(out)).toBe(CODEX_ID);
}, 60_000);

test("--cwd ignores a newer subagent transcript", () => {
  const { code, out } = run("--cwd", MID_CWD, "--json");
  expect(code).toBe(0);
  const a = JSON.parse(out) as {
    sessionId: string;
    peakContextTokens: number;
  };
  expect(a.sessionId).toBe(MID_ID);
  expect(a.peakContextTokens).toBe(40_000);
}, 60_000);

test("--cwd picks the newest session at or below the directory", () => {
  const { code, out } = run("--cwd", OLD_CWD, "--json");
  expect(code).toBe(0);
  expect(sessionOf(out)).toBe(MID_ID);
}, 60_000);

test("--latest --cwd combine; the table form works too", () => {
  const { code, out } = run("--latest", "--cwd", OLD_CWD);
  expect(code).toBe(0);
  expect(out).toContain(MID_ID);
  expect(out).toContain("verdict");
}, 60_000);

test("--cwd with no match fails and names the directory", () => {
  const { code, err } = run("--cwd", "/nowhere/at/all");
  expect(code).toBe(1);
  expect(err).toContain("No sessions found under /nowhere/at/all");
}, 60_000);

test("no id and no flag fails with usage", () => {
  const { code, err } = run();
  expect(code).toBe(1);
  expect(err).toContain("Pass a session id, --latest, or --cwd <dir>");
}, 60_000);

test("an id together with --latest fails", () => {
  const { code, err } = run(OLD_ID, "--latest");
  expect(code).toBe(1);
  expect(err).toContain("not both");
}, 60_000);

test("an explicit id still works, with --json before or after it", () => {
  expect(sessionOf(run("--json", OLD_ID).out)).toBe(OLD_ID);
  expect(sessionOf(run(OLD_ID, "--json").out)).toBe(OLD_ID);
}, 60_000);

test("--parent without an id fails instead of looking up the newest id", () => {
  const { code, err } = run("--latest", "--parent", OLD_ID);
  expect(code).toBe(1);
  expect(err).toContain("--parent needs an explicit subagent id");
}, 60_000);

test("a flag missing its value is reported, not taken as the id", () => {
  for (const args of [["--cwd"], ["--latest", "--cwd"]]) {
    const { code, err } = run(...args);
    expect(code).toBe(1);
    expect(err).toContain("Missing value for --cwd");
  }
}, 60_000);

test("--latest --json parses as flags, not a positional", () => {
  const { code, out } = run("--json", "--latest");
  expect(code).toBe(0);
  expect(() => JSON.parse(out)).not.toThrow();
}, 60_000);
