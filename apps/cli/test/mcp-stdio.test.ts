/** `peektrace mcp` over stdio against a throwaway home: initialize, list the
 * tools, call one. */
import { afterAll, beforeAll, expect, test } from "bun:test";
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
let home = "";

beforeAll(() => {
  home = makeHome("mcp");
  write(
    claudePath(home, "-home-demo-work", ID),
    claudeLines({
      at: "2026-03-01T10:00:00.000Z",
      contextTokens: 20_000,
      cwd: "/home/demo/work",
    })
  );
});
afterAll(() => removeHome(home));

const frame = (id: number, method: string, params: unknown = {}) =>
  `${JSON.stringify({ jsonrpc: "2.0", id, method, params })}\n`;

test("mcp answers initialize, tools/list and tools/call over stdio", async () => {
  const proc = Bun.spawn(["bun", ENTRY, "mcp"], {
    env: envFor(home),
    stdin: "pipe",
    stdout: "pipe",
    stderr: "pipe",
  });
  proc.stdin.write(
    frame(1, "initialize", {
      protocolVersion: "2025-06-18",
      capabilities: {},
      clientInfo: { name: "test", version: "0" },
    })
  );
  proc.stdin.write(
    `${JSON.stringify({ jsonrpc: "2.0", method: "notifications/initialized" })}\n`
  );
  proc.stdin.write(frame(2, "tools/list"));
  proc.stdin.write(
    frame(3, "tools/call", { name: "sessions_list", arguments: {} })
  );
  proc.stdin.end();
  const out = await new Response(proc.stdout).text();
  expect(await proc.exited).toBe(0);
  const replies = out
    .split("\n")
    .filter((l) => l.trim() !== "")
    .map(
      (l) =>
        JSON.parse(l) as {
          id?: number;
          result?: {
            serverInfo?: { name: string };
            tools?: { name: string }[];
          };
        }
    );
  const byId = new Map(replies.map((r) => [r.id, r]));
  expect(byId.get(1)?.result?.serverInfo?.name).toBe("peektrace");
  const names = (byId.get(2)?.result?.tools ?? []).map((t) => t.name);
  expect(names).toEqual(
    expect.arrayContaining([
      "stats_report",
      "stats_drill",
      "sessions_list",
      "sessions_analyze",
    ])
  );
  expect(JSON.stringify(byId.get(3)?.result)).toContain("44444444");
}, 30_000);

test("a raw U+2028 inside a request still gets one reply with its id", async () => {
  const proc = Bun.spawn(["bun", ENTRY, "mcp"], {
    env: envFor(home),
    stdin: "pipe",
    stdout: "pipe",
    stderr: "pipe",
  });
  proc.stdin.write(
    frame(7, "tools/call", {
      name: "sessions_list",
      arguments: { "a\u2028b": 1 },
    })
  );
  proc.stdin.end();
  const out = await new Response(proc.stdout).text();
  expect(await proc.exited).toBe(0);
  const replies = out
    .split("\n")
    .filter((l) => l.trim() !== "")
    .map(
      (l) => JSON.parse(l) as { id: unknown; result?: { isError?: boolean } }
    );
  expect(replies).toHaveLength(1);
  expect(replies[0]?.id).toBe(7);
  expect(replies[0]?.result?.isError).toBe(true);
}, 30_000);
