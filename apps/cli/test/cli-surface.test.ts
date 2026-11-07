/** Spawned-CLI checks for the banner and `--json` placement. Uses an empty synthetic HOME. */
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const ENTRY = join(import.meta.dirname, "../src/index.ts");
let home = "";

const run = async (...args: string[]) => {
  const proc = Bun.spawn(["bun", ENTRY, ...args], {
    env: {
      PATH: process.env.PATH ?? "",
      HOME: home,
      PEEKTRACE_DIR: join(home, ".peektrace"),
      PEEKTRACE_NO_TELEMETRY: "1",
      PEEKTRACE_NO_UPDATE_CHECK: "1",
    },
    stdout: "pipe",
    stderr: "pipe",
  });
  const [stdout, code] = await Promise.all([
    new Response(proc.stdout).text(),
    proc.exited,
  ]);
  return { stdout, code };
};

beforeAll(() => {
  home = mkdtempSync(join(tmpdir(), "peektrace-surface-"));
});
afterAll(() => {
  rmSync(home, { recursive: true, force: true });
});

describe("cli surface", () => {
  test("banner names all four agents", async () => {
    const { stdout, code } = await run();
    expect(code).toBe(0);
    for (const name of ["Claude Code", "Codex", "Pi", "OpenCode"]) {
      expect(stdout).toContain(name);
    }
    const tryLine = stdout.split("\n").find((l) => l.startsWith("Try:"));
    expect(tryLine).toContain("stats");
    expect(tryLine).toContain("tui");
  });

  test("--json works before and after the subcommand", async () => {
    const before = await run("--json", "sessions", "ls");
    const after = await run("sessions", "ls", "--json");
    expect(before.code).toBe(0);
    expect(after.code).toBe(0);
    expect(JSON.parse(before.stdout)).toEqual([]);
    expect(JSON.parse(after.stdout)).toEqual([]);
  });
});
