import { expect, test } from "bun:test";
import { firstPositional, skipsTelemetry } from "../src/argv";

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
