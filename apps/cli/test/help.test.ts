/** `--help` surface: owned root page, per-command descriptions, completion scripts. */
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { join } from "node:path";
import { COMMAND_ROWS, GROUP_PATHS, ROOT_FLAGS } from "../src/help";
import { ENTRY, envFor, makeHome, removeHome } from "./support/synthetic-home";

const LIST_COMMANDS = join(import.meta.dir, "support", "list-commands.ts");

let home = "";

const run = async (args: readonly string[], stdin?: string) => {
  const proc = Bun.spawn(["bun", ENTRY, ...args], {
    env: { ...envFor(home), PEEKTRACE_NO_UPDATE_CHECK: "1" },
    stdin: stdin === undefined ? "ignore" : new Blob([stdin]),
    stdout: "pipe",
    stderr: "pipe",
  });
  const [stdout, code] = await Promise.all([
    new Response(proc.stdout).text(),
    proc.exited,
  ]);
  return { stdout, code };
};

// biome-ignore lint/suspicious/noControlCharactersInRegex: ANSI escape
const stripAnsi = (text: string) => text.replace(/\u001b\[[0-9;]*m/g, "");

const OPTION_START = /\n(?=--)/;

const count = (text: string, needle: string) => text.split(needle).length - 1;

beforeAll(() => {
  home = makeHome("help");
});
afterAll(() => {
  removeHome(home);
});

describe("root help", () => {
  for (const args of [["--help"], ["-h"], ["--json", "--help"]]) {
    test(`peektrace ${args.join(" ")} lists every command compactly`, async () => {
      const { stdout, code } = await run(args);
      expect(code).toBe(0);
      const text = stripAnsi(stdout);
      for (const row of COMMAND_ROWS) {
        expect(text).toContain(row.path);
        expect(text).toContain(row.summary);
      }
      expect(text).toContain("EXAMPLES");
      expect(text).not.toContain("--wizard");
      expect(text).not.toContain("A true or false value.");
      const lines = text.split("\n");
      expect(lines.length).toBeLessThanOrEqual(55);
      for (const line of lines) {
        expect(line.length).toBeLessThanOrEqual(100);
      }
    });
  }

  test("every root flag appears on the page", async () => {
    const { stdout } = await run(["--help"]);
    const text = stripAnsi(stdout);
    for (const flag of Object.values(ROOT_FLAGS)) {
      expect(text).toContain(flag.usage);
      expect(text).toContain(flag.description);
    }
  });

  test("rows match the real command tree", async () => {
    const proc = Bun.spawn(["bun", LIST_COMMANDS], {
      env: envFor(home),
      stdout: "pipe",
      stderr: "pipe",
    });
    const paths = JSON.parse(await new Response(proc.stdout).text());
    expect(await proc.exited).toBe(0);
    expect(new Set(paths)).toEqual(
      new Set([...COMMAND_ROWS.map((row) => row.path), ...GROUP_PATHS])
    );
  });
});

describe("command pages", () => {
  for (const name of ["doctor", "upgrade"]) {
    test(`${name} --help states each default once, hides the wizard`, async () => {
      const { stdout, code } = await run([name, "--help"]);
      expect(code).toBe(0);
      const text = stripAnsi(stdout);
      const options = text.split(OPTION_START).slice(1);
      expect(options.length).toBeGreaterThan(0);
      for (const block of options) {
        expect(count(block, "This setting is optional.")).toBeLessThanOrEqual(
          1
        );
      }
      expect(text).not.toContain("--wizard");
    });
  }

  test("a flag-less group page still renders", async () => {
    const { stdout, code } = await run(["sessions", "--help"]);
    expect(code).toBe(0);
    expect(stripAnsi(stdout)).toContain("List and analyze sessions");
  });
});

describe("built-ins still work", () => {
  test("--version and -v print the version", async () => {
    for (const flag of ["--version", "-v"]) {
      const { stdout, code } = await run([flag]);
      expect(code).toBe(0);
      expect(stdout.trim().length).toBeGreaterThan(0);
    }
  });

  for (const shell of ["bash", "zsh"] as const) {
    test(`--completions ${shell} emits a script ${shell} accepts`, async () => {
      const { stdout, code } = await run(["--completions", shell]);
      expect(code).toBe(0);
      expect(stdout.trim().length).toBeGreaterThan(0);
      if (Bun.which(shell) === null) {
        return;
      }
      const check = Bun.spawn([shell, "-n"], {
        stdin: new Blob([stdout]),
        stdout: "ignore",
        stderr: "pipe",
      });
      expect(await check.exited).toBe(0);
    });
  }
});
