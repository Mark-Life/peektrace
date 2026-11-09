import { expect, test } from "bun:test";
import {
  beforeSeparator,
  firstPositional,
  helpGroup,
  isRootHelp,
  loggableArgv,
  skipsTelemetry,
} from "../src/argv";

const argv = (...rest: readonly string[]) => ["bun", "index.ts", ...rest];

test("firstPositional skips global flags and the --remote value", () => {
  expect(firstPositional(argv("statusline"))).toBe("statusline");
  expect(firstPositional(argv("--json", "--otel", "sessions", "ls"))).toBe(
    "sessions"
  );
  expect(firstPositional(argv("--remote", "http://x:1", "statusline"))).toBe(
    "statusline"
  );
  expect(firstPositional(argv("--json"))).toBeUndefined();
});

test("skipsTelemetry only for the statusline command", () => {
  expect(skipsTelemetry(argv("--remote", "statusline", "sessions"))).toBe(
    false
  );
  expect(skipsTelemetry(argv("--json", "statusline", "--timeout", "5"))).toBe(
    true
  );
  expect(skipsTelemetry(argv("sessions", "analyze", "statusline"))).toBe(false);
});

test("skipsTelemetry sees statusline after value flags", () => {
  expect(skipsTelemetry(argv("--log-level", "debug", "statusline"))).toBe(true);
  expect(firstPositional(argv("--completions", "zsh", "sessions"))).toBe(
    "sessions"
  );
});

test("isRootHelp is true only for help without a subcommand", () => {
  expect(isRootHelp(argv("--help"))).toBe(true);
  expect(isRootHelp(argv("-h"))).toBe(true);
  expect(isRootHelp(argv("--remote", "http://x:1", "--help"))).toBe(true);
  expect(isRootHelp(argv("--log-level", "debug", "--help"))).toBe(true);
  expect(isRootHelp(argv("--json", "-h"))).toBe(true);
  expect(isRootHelp(argv("stats", "--help"))).toBe(false);
  expect(isRootHelp(argv("sessions", "-h"))).toBe(false);
  expect(isRootHelp(argv("--help", "sessions"))).toBe(true);
  expect(isRootHelp(argv("--completions", "zsh", "--help"))).toBe(false);
  expect(isRootHelp(argv("--completions=zsh", "--help"))).toBe(false);
  expect(isRootHelp(argv("--help=true"))).toBe(true);
  expect(isRootHelp(argv())).toBe(false);
});

test("helpGroup names a group only for `<group> --help` with no deeper command", () => {
  const groups = ["sessions", "memory"];
  expect(helpGroup(argv("sessions", "--help"), groups)).toBe("sessions");
  expect(helpGroup(argv("--json", "memory", "-h"), groups)).toBe("memory");
  expect(helpGroup(argv("--help", "sessions"), groups)).toBe("sessions");
  expect(helpGroup(argv("sessions", "ls", "--help"), groups)).toBeUndefined();
  expect(helpGroup(argv("stats", "--help"), groups)).toBeUndefined();
  expect(helpGroup(argv("sessions"), groups)).toBeUndefined();
  expect(helpGroup(argv("--help"), groups)).toBeUndefined();
  expect(
    helpGroup(argv("sessions", "--completions", "zsh", "--help"), groups)
  ).toBeUndefined();
});

test("beforeSeparator stops at a bare --", () => {
  expect(beforeSeparator(argv("sessions", "search", "--", "--otel"))).toEqual([
    "bun",
    "index.ts",
    "sessions",
    "search",
  ]);
  expect(beforeSeparator(argv("ls", "--otel"))).toContain("--otel");
});

test("loggableArgv hides the text of a search, keeps flags", () => {
  const out = loggableArgv(
    argv("sessions", "search", "secretword", "--agent", "claude", "--", "-v")
  );
  expect(out).not.toContain("secretword");
  expect(out).not.toContain("-v");
  expect(out).toContain("sessions search");
  expect(out).toContain("--agent");
  expect(loggableArgv(argv("sessions", "ls", "--limit", "5"))).toBe(
    "sessions ls --limit 5"
  );
});
