/** `peektrace statusline`, run as a real process fed Claude Code's stdin JSON. */
import { afterAll, beforeAll, expect, test } from "bun:test";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { FALLBACK } from "../src/commands/statusline";
import {
  claudeLines,
  claudePath,
  ENTRY,
  envFor,
  makeHome,
  removeHome,
  write,
} from "./support/synthetic-home";

const ID = "44444444-4444-4444-8444-444444444444";
const BIG_ID = "55555555-5555-4555-8555-555555555555";
const FALLBACK_LINE = `${FALLBACK}\n`;

let home = "";
let path = "";
let bigPath = "";

beforeAll(() => {
  home = makeHome("statusline");
  path = claudePath(home, "-home-demo-work", ID);
  // Claude transcripts carry no window: the analyzer default (1M) applies.
  write(
    path,
    claudeLines({
      at: "2026-03-01T10:00:00.000Z",
      contextTokens: 50_000,
      cwd: "/home/demo/work",
    })
  );
  bigPath = claudePath(home, "-home-demo-work", BIG_ID);
  write(
    bigPath,
    claudeLines({
      at: "2026-03-02T10:00:00.000Z",
      contextTokens: 150_000,
      cwd: "/home/demo/work",
    })
  );
});

afterAll(() => removeHome(home));

const run = async (
  stdin: string,
  args: readonly string[] = [],
  env: Record<string, string | undefined> = envFor(home)
) => {
  const proc = Bun.spawn(["bun", ENTRY, "statusline", ...args], {
    env,
    stdin: new Blob([stdin]),
    stdout: "pipe",
    stderr: "pipe",
  });
  const [out, err, code] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ]);
  return { code, err, out };
};

test("transcript_path prints percent and verdict", async () => {
  const r = await run(JSON.stringify({ transcript_path: path, extra: 1 }));
  expect(r).toEqual({ code: 0, err: "", out: "ctx 5% Healthy\n" });
}, 60_000);

test("session_id alone resolves the same session", async () => {
  const r = await run(JSON.stringify({ session_id: ID }));
  expect(r).toEqual({ code: 0, err: "", out: "ctx 5% Healthy\n" });
}, 60_000);

test("context_window_size overrides the default window", async () => {
  const narrow = await run(
    JSON.stringify({
      transcript_path: bigPath,
      context_window: { context_window_size: 200_000 },
    })
  );
  expect(narrow.out).toBe("ctx 75% Rotting\n");
  const usual = await run(JSON.stringify({ transcript_path: bigPath }));
  expect(usual.out).toBe("ctx 15% Healthy\n");
}, 60_000);

test("JSON null fields count as absent", async () => {
  const ok = { out: "ctx 5% Healthy\n" };
  for (const payload of [
    { transcript_path: path, context_window: null },
    { transcript_path: path, context_window: { context_window_size: null } },
    { transcript_path: path, cwd: null, session_id: null },
    { transcript_path: null, session_id: ID },
  ]) {
    expect(await run(JSON.stringify(payload))).toMatchObject(ok);
  }
}, 60_000);

test("empty transcript_path falls back to session_id; a short id is ignored", async () => {
  const r = await run(JSON.stringify({ transcript_path: "", session_id: ID }));
  expect(r.out).toBe("ctx 5% Healthy\n");
  const short = await run(JSON.stringify({ session_id: "4" }));
  expect(short.out).toBe(FALLBACK_LINE);
}, 60_000);

test("an unresolvable transcript_path falls back to session_id", async () => {
  const r = await run(
    JSON.stringify({
      session_id: ID,
      transcript_path: `/elsewhere/${ID}.jsonl`,
    })
  );
  expect(r.out).toBe("ctx 5% Healthy\n");
}, 60_000);

test("a wrongly typed context_window loses only the window", async () => {
  const r = await run(
    JSON.stringify({
      transcript_path: path,
      context_window: { context_window_size: "200000" },
    })
  );
  expect(r.out).toBe("ctx 5% Healthy\n");
}, 60_000);

test("writes no telemetry even when telemetry is enabled", async () => {
  const { PEEKTRACE_NO_TELEMETRY: _off, ...env } = envFor(home);
  const r = await run(JSON.stringify({ transcript_path: path }), [], env);
  expect(r.out).toBe("ctx 5% Healthy\n");
  expect(existsSync(join(home, ".peektrace", "telemetry.db"))).toBe(false);
}, 60_000);

test("bad stdin, empty object and unknown id fall back with exit 0", async () => {
  for (const stdin of [
    "not json",
    "",
    "{}",
    JSON.stringify({ session_id: "no-such-session" }),
  ]) {
    const r = await run(stdin);
    expect(r).toEqual({ code: 0, err: "", out: FALLBACK_LINE });
  }
}, 60_000);

test("stdin held open: times out, falls back and the process exits", async () => {
  const started = Date.now();
  const proc = Bun.spawn(["bun", ENTRY, "statusline", "--timeout", "200"], {
    env: envFor(home),
    stdin: "pipe",
    stdout: "pipe",
    stderr: "pipe",
  });
  const out = await new Response(proc.stdout).text();
  const code = await proc.exited;
  expect(out).toBe(FALLBACK_LINE);
  expect(code).toBe(0);
  expect(Date.now() - started).toBeLessThan(2000 + 1500);
}, 60_000);
