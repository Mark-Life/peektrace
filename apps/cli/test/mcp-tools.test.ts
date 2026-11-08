/** Each MCP tool against the real in-process client over a throwaway home
 * holding one synthetic Claude session. */

import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { join } from "node:path";
import { makeHandlersLayer, makeInProcessClient } from "@workspace/rpc";
import { Effect } from "effect";
import { handleLine } from "../src/mcp/protocol";
import { TOOLS } from "../src/mcp/tools";
import {
  claudeLines,
  claudePath,
  envFor,
  makeHome,
  removeHome,
  write,
} from "./support/synthetic-home";

const ID = "44444444-4444-4444-8444-444444444444";
const ENV_KEYS = [
  "HOME",
  "CLAUDE_CONFIG_DIR",
  "CODEX_HOME",
  "XDG_DATA_HOME",
  "PEEKTRACE_DIR",
  "PEEKTRACE_PI_SESSIONS",
  "PEEKTRACE_OPENCODE_DATA",
] as const;

let home = "";
const saved = new Map<string, string | undefined>();

beforeAll(() => {
  home = makeHome("mcp-tools");
  write(
    claudePath(home, "-home-demo-work", ID),
    claudeLines({
      at: "2026-03-01T10:00:00.000Z",
      contextTokens: 20_000,
      cwd: "/home/demo/work",
    })
  );
  // Bun's `homedir()` ignores a runtime HOME change, so pin every root.
  const env: Record<(typeof ENV_KEYS)[number], string> = {
    ...envFor(home),
    PEEKTRACE_PI_SESSIONS: join(home, ".pi", "agent", "sessions"),
    PEEKTRACE_OPENCODE_DATA: join(home, ".local", "share", "opencode"),
  };
  for (const key of ENV_KEYS) {
    saved.set(key, process.env[key]);
    process.env[key] = env[key];
  }
});

afterAll(() => {
  for (const [key, value] of saved) {
    if (value === undefined) {
      delete process.env[key];
    } else {
      process.env[key] = value;
    }
  }
  removeHome(home);
});

interface ToolResult {
  readonly content: readonly { readonly text: string }[];
  readonly isError?: boolean;
}

/** One `tools/call` through the dispatcher with a fresh in-process client. */
const call = (name: string, args: Record<string, unknown> = {}) =>
  Effect.runPromise(
    Effect.gen(function* () {
      const client = yield* makeInProcessClient();
      const out = yield* handleLine({ tools: TOOLS, client })(
        JSON.stringify({
          jsonrpc: "2.0",
          id: 1,
          method: "tools/call",
          params: { name, arguments: args },
        })
      );
      const reply = JSON.parse(out ?? "{}") as { result: ToolResult };
      return reply.result;
    }).pipe(
      Effect.scoped,
      Effect.provide(makeHandlersLayer({ readOnly: true }))
    )
  );

const textOf = (r: ToolResult) => r.content.map((c) => c.text).join("\n");

describe("sessions_list", () => {
  test("lists the synthetic session as a compact table", async () => {
    const r = await call("sessions_list");
    expect(r.isError).toBeUndefined();
    expect(textOf(r)).toContain("44444444");
    expect(textOf(r)).toStartWith("id\tagent\tproject");
    expect(textOf(r)).toContain("EST COST covers the main transcript only");
  });

  test("format json parses to the listed rows", async () => {
    const r = await call("sessions_list", { format: "json" });
    const rows = JSON.parse(textOf(r).split("\n\n")[0] ?? "") as {
      id: string;
    }[];
    expect(rows.map((row) => row.id)).toEqual([ID]);
  });

  test("a bad since is a tool error, not a crash", async () => {
    const r = await call("sessions_list", { since: "soon" });
    expect(r.isError).toBe(true);
    expect(textOf(r)).toContain("Invalid --since");
  });
});

describe("sessions_analyze", () => {
  test("latest:true renders the verdict row", async () => {
    const r = await call("sessions_analyze", { latest: true });
    expect(r.isError).toBeUndefined();
    expect(textOf(r)).toContain("verdict");
    expect(textOf(r)).toContain(ID);
  });

  test("no target is a tool error", async () => {
    const r = await call("sessions_analyze");
    expect(r.isError).toBe(true);
  });

  test("format json parses", async () => {
    const r = await call("sessions_analyze", { id: ID, format: "json" });
    expect((JSON.parse(textOf(r)) as { sessionId: string }).sessionId).toBe(ID);
  });
});

describe("stats", () => {
  test("stats_report text starts with the report header", async () => {
    const r = await call("stats_report", { days: 3650 });
    expect(r.isError).toBeUndefined();
    expect(textOf(r).startsWith("peektrace stats  3650d window")).toBe(true);
  });

  test("stats_report format json parses", async () => {
    const r = await call("stats_report", { days: 3650, format: "json" });
    expect(
      (JSON.parse(textOf(r)) as { schemaVersion: string }).schemaVersion
    ).toStartWith("peektrace-stats/");
  });

  test("stats_drill with an unknown key is a tool error", async () => {
    const r = await call("stats_drill", {
      table: "cause",
      key: "cause:no-such-cause",
      days: 3650,
    });
    expect(r.isError).toBe(true);
    expect(textOf(r)).toContain("no-such-cause");
  });
});
