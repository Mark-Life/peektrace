/** MCP dispatch with no corpus: `handleLine` over the real tool table and a
 * stub tool, one JSON-RPC line in, at most one line out. */
import { describe, expect, test } from "bun:test";
import { Effect, Schema } from "effect";
import type { PeektraceClient } from "../src/client";
import {
  defineTool,
  ErrorCode,
  handleLine,
  type McpTool,
  PROTOCOL_VERSIONS,
} from "../src/mcp/protocol";
import { TOOLS } from "../src/mcp/tools";

const TOOL_NAME = /^[a-zA-Z0-9_-]{1,64}$/;

// The real tools fail input decoding before they touch the client.
const unusedClient = {} as PeektraceClient;

const boom = defineTool({
  name: "boom",
  description: "dies",
  input: Schema.Struct({}),
  run: () => Effect.die(new Error("kaput")),
}) as McpTool<PeektraceClient>;

const handle = handleLine({ tools: [...TOOLS, boom], client: unusedClient });

interface ListedTool {
  readonly annotations: unknown;
  readonly inputSchema: {
    readonly properties: Record<string, unknown>;
    readonly type: string;
  };
  readonly name: string;
}

interface Reply {
  readonly error?: { readonly code: number; readonly message: string };
  readonly id: number | string | null;
  readonly result?: {
    readonly capabilities?: unknown;
    readonly content?: readonly { readonly text: string }[];
    readonly isError?: boolean;
    readonly protocolVersion?: string;
    readonly serverInfo?: { readonly name: string };
    readonly tools?: readonly ListedTool[];
  };
}

const send = async (message: unknown) => {
  const line = typeof message === "string" ? message : JSON.stringify(message);
  const out = await Effect.runPromise(handle(line));
  return out === undefined ? undefined : (JSON.parse(out) as Reply);
};

const request = (id: number, method: string, params?: unknown) => ({
  jsonrpc: "2.0",
  id,
  method,
  ...(params === undefined ? {} : { params }),
});

describe("initialize", () => {
  test("echoes a supported protocol version", async () => {
    const r = await send(
      request(1, "initialize", { protocolVersion: "2025-03-26" })
    );
    expect(r?.id).toBe(1);
    expect(r?.result?.protocolVersion).toBe("2025-03-26");
    expect(r?.result?.serverInfo?.name).toBe("peektrace");
    expect(r?.result?.capabilities).toEqual({ tools: { listChanged: false } });
  });

  test("falls back to the newest version for an unknown one", async () => {
    const r = await send(
      request(2, "initialize", { protocolVersion: "1999-01-01" })
    );
    expect(r?.result?.protocolVersion).toBe(PROTOCOL_VERSIONS[0]);
  });
});

describe("envelope", () => {
  test("a notification gets no reply", async () => {
    expect(
      await send({ jsonrpc: "2.0", method: "notifications/initialized" })
    ).toBeUndefined();
  });

  test("a blank line gets no reply", async () => {
    expect(await send("   ")).toBeUndefined();
  });

  test("ping answers an empty result", async () => {
    expect((await send(request(3, "ping")))?.result).toEqual({});
  });

  test("unknown method is -32601", async () => {
    const r = await send(request(4, "resources/list"));
    expect(r?.error?.code).toBe(ErrorCode.methodNotFound);
    expect(r?.id).toBe(4);
  });

  test("bad JSON is -32700 with a null id", async () => {
    const r = await send("{not json");
    expect(r?.error?.code).toBe(ErrorCode.parse);
    expect(r?.id).toBeNull();
  });

  test("a request without jsonrpc 2.0 is -32600 and keeps its id", async () => {
    const r = await send({ id: 5, method: "ping" });
    expect(r?.error?.code).toBe(ErrorCode.invalidRequest);
    expect(r?.id).toBe(5);
  });

  test("unknown tool is -32602", async () => {
    const r = await send(request(6, "tools/call", { name: "nope" }));
    expect(r?.error?.code).toBe(ErrorCode.invalidParams);
  });
});

describe("tools", () => {
  test("lists the four read-only tools with object schemas", async () => {
    const r = await send(request(7, "tools/list"));
    const tools = r?.result?.tools ?? [];
    const names = tools.map((t) => t.name);
    expect(names).toEqual(
      expect.arrayContaining([
        "sessions_list",
        "sessions_analyze",
        "stats_report",
        "stats_drill",
      ])
    );
    for (const t of tools.filter((tool) => tool.name !== "boom")) {
      expect(t.name).toMatch(TOOL_NAME);
      expect(t.inputSchema.type).toBe("object");
      expect(t.inputSchema).not.toHaveProperty("$schema");
      expect(t.inputSchema.properties).not.toHaveProperty("redact");
      expect(t.inputSchema.properties).not.toHaveProperty("force");
      expect(t.annotations).toEqual({
        readOnlyHint: true,
        openWorldHint: false,
      });
    }
  });

  test.each(
    TOOLS.map((t) => t.name)
  )("%s refuses redact:false", async (name) => {
    const r = await send(
      request(8, "tools/call", { name, arguments: { redact: false } })
    );
    expect(r?.result?.isError).toBe(true);
    expect(r?.result?.content?.[0]?.text).toContain("redact");
  });

  test("a defect becomes an isError result", async () => {
    const r = await send(request(9, "tools/call", { name: "boom" }));
    expect(r?.result).toEqual({
      content: [{ type: "text", text: "kaput" }],
      isError: true,
    });
  });
});
