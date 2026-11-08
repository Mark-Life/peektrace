/** `peektrace sessions search`, run as a user runs it: a real process over a
 * throwaway home holding synthetic sessions. */

import { Database } from "bun:sqlite";
import { afterAll, beforeAll, expect, test } from "bun:test";
import { join } from "node:path";
import {
  ENTRY,
  envFor,
  makeHome,
  removeHome,
  write,
} from "./support/synthetic-home";

const NEW_ID = "11111111-1111-4111-8111-111111111111";
const OLD_ID = "22222222-2222-4222-8222-222222222222";
const DASH_ID = "33333333-3333-4333-8333-333333333333";
const FAKE_KEY = "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234";

const user = (at: string, text: string) => ({
  type: "user",
  cwd: "/home/demo/work",
  timestamp: at,
  message: { content: text },
});

let home = "";

beforeAll(() => {
  home = makeHome("search");
  const dir = join(home, ".claude", "projects", "-home-demo-work");
  write(join(dir, `${NEW_ID}.jsonl`), [
    user("2026-05-01T10:00:00.000Z", `use ${FAKE_KEY} for the walrus job`),
    user("2026-05-01T10:00:01.000Z", "walrus again"),
  ]);
  write(join(dir, `${OLD_ID}.jsonl`), [
    user("2026-02-01T10:00:00.000Z", "an old walrus note"),
  ]);
  write(
    join(home, ".claude", "projects", "-home-demo-dash", `${DASH_ID}.jsonl`),
    [user("2026-01-01T10:00:00.000Z", "run with -v flag zebra")]
  );
});

afterAll(() => removeHome(home));

const run = (...args: readonly string[]) => {
  const proc = Bun.spawnSync(["bun", ENTRY, "sessions", "search", ...args], {
    env: envFor(home),
  });
  return {
    code: proc.exitCode,
    err: proc.stderr.toString(),
    out: proc.stdout.toString(),
  };
};

test("prints one row per hit, newest session first, with line numbers", () => {
  const { code, out } = run("walrus");
  expect(code).toBe(0);
  const lines = out.trim().split("\n");
  expect(lines[0]?.split("\t")).toEqual([
    "session",
    "agent",
    "project",
    "updated",
    "matches",
    "snippet",
  ]);
  const first = lines[1]?.split("\t") ?? [];
  expect(first.slice(0, 2)).toEqual(["11111111", "claude"]);
  expect(first[4]).toBe("2");
  expect(first[5]).toStartWith("L1  ");
  expect(lines[2]?.split("\t")[5]).toStartWith("L2  ");
  expect(lines[3]?.split("\t")[0]).toBe("22222222");
}, 60_000);

test("--json prints the raw result", () => {
  const { code, out } = run("walrus", "--json", "--limit", "1");
  expect(code).toBe(0);
  const r = JSON.parse(out);
  expect(r.sessions.map((s: { id: string }) => s.id)).toEqual([NEW_ID]);
  expect(r.truncated).toBe(true);
  expect(typeof r.scannedSessions).toBe("number");
}, 60_000);

test("redacts secrets by default and keeps them with --no-redact", () => {
  expect(run("walrus job").out).not.toContain(FAKE_KEY);
  expect(run("walrus job", "--no-redact").out).toContain(FAKE_KEY);
}, 60_000);

test("--since narrows by update time", () => {
  const { out } = run("walrus", "--since", "2026-04-01");
  expect(out).toContain("11111111");
  expect(out).not.toContain("22222222");
}, 60_000);

test("no match exits 0 and says so on stderr", () => {
  const { code, out, err } = run("no-such-phrase");
  expect(code).toBe(0);
  expect(out).toBe("");
  expect(err).toContain("no matches");
}, 60_000);

test("bad input is a user error", () => {
  for (const args of [
    ["walrus", "--agent", "nope"],
    ["walrus", "--since", "soon"],
    ["walrus", "--limit", "-1"],
    ["   "],
  ]) {
    const { code, err } = run(...args);
    expect(code).not.toBe(0);
    expect(err).not.toBe("");
  }
}, 60_000);

test("--help exits 0", () => {
  const { code, out } = run("--help");
  expect(code).toBe(0);
  expect(out).toContain("--no-redact");
}, 60_000);

test("text after -- is the query, even when it looks like a flag", () => {
  const dash = run("--", "-v");
  expect(dash.code).toBe(0);
  expect(dash.out).toContain("33333333");
  expect(run("--", "with -v flag").out).toContain("33333333");
  expect(run("--", "--version zebra").out).not.toContain("0.0.0");
}, 60_000);

test("a project shown in the results works as --project", () => {
  const shown: string = JSON.parse(run("walrus", "--json").out).sessions[0]
    .project;
  expect(shown).not.toContain("home");
  const { out } = run("walrus", "--project", shown, "--json");
  expect(JSON.parse(out).sessions).toHaveLength(2);
}, 60_000);

test("several unquoted words form one phrase", () => {
  const { code, out } = run("an", "old", "walrus");
  expect(code).toBe(0);
  expect(out).toContain("an old walrus note");
}, 60_000);

test("typed words never reach telemetry, even in validation errors", () => {
  const { NO_TELEMETRY: _drop, ...env } = {
    ...envFor(home),
    NO_TELEMETRY: "",
    PEEKTRACE_NO_TELEMETRY: undefined,
  };
  const attempts = [
    ["zebrasecret", "--limit", "abcsecret"],
    ["zebrasecret", "--agent", "quokkasecret"],
    ["zebrasecret", "--since", "quokkasecret"],
  ];
  for (const args of attempts) {
    const proc = Bun.spawnSync(["bun", ENTRY, "sessions", "search", ...args], {
      env,
    });
    expect(proc.exitCode).not.toBe(0);
  }
  const db = new Database(join(home, ".peektrace", "telemetry.db"));
  const rows = JSON.stringify(db.query("SELECT * FROM events").all());
  db.close();
  expect(rows).toContain("sessions search");
  for (const secret of ["zebrasecret", "abcsecret", "quokkasecret"]) {
    expect(rows).not.toContain(secret);
  }
}, 60_000);
