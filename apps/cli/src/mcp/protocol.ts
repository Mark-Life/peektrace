/** Minimal MCP server protocol: newline-delimited JSON-RPC 2.0 over stdio.
 *
 * Covers `initialize`, `ping`, `tools/list` and `tools/call`; notifications get
 * no reply. Pure dispatch only: `handleLine` maps one input line to at most one
 * output line and never does IO, so the stdio loop lives in `commands/mcp.ts`.
 */
import { Cause, Effect, Either, JSONSchema, ParseResult, Schema } from "effect";
import { CliUserError, formatCliError } from "../errors";
import { APP_VERSION } from "../version";

/** Newest first; the reply echoes the client's version when it is listed. */
export const PROTOCOL_VERSIONS = [
  "2025-11-25",
  "2025-06-18",
  "2025-03-26",
  "2024-11-05",
] as const;

export const ErrorCode = {
  parse: -32_700,
  invalidRequest: -32_600,
  methodNotFound: -32_601,
  invalidParams: -32_602,
} as const;

/** A tool's answer: the main text, plus notes sent as a separate content block
 * so a JSON answer stays parseable. */
export interface ToolOutput {
  readonly notes?: readonly string[];
  readonly text: string;
}

/** A tool as the server sees it: wire metadata plus a call that decodes its own input. */
export interface McpTool<C> {
  readonly call: (
    client: C,
    args: unknown
  ) => Effect.Effect<string | ToolOutput, unknown>;
  readonly description: string;
  readonly inputSchema: object;
  readonly name: string;
}

interface ToolSpec<C, A, I> {
  readonly description: string;
  readonly input: Schema.Schema<A, I>;
  readonly name: string;
  readonly run: (
    client: C,
    input: A
  ) => Effect.Effect<string | ToolOutput, unknown>;
}

/** Build a tool from an Effect Schema input. Unknown keys fail the call, so a
 * field the schema leaves out (such as `redact`) can never reach core. */
export const defineTool = <C, A, I>(spec: ToolSpec<C, A, I>): McpTool<C> => {
  const { $schema: _, ...inputSchema } = JSONSchema.make(spec.input);
  const decode = Schema.decodeUnknown(spec.input, {
    onExcessProperty: "error",
  });
  return {
    name: spec.name,
    description: spec.description,
    inputSchema,
    call: (client, args) =>
      decode(args ?? {}).pipe(
        Effect.mapError(
          (e) =>
            new CliUserError({
              message: `Invalid arguments for ${spec.name}: ${ParseResult.TreeFormatter.formatErrorSync(e)}`,
            })
        ),
        Effect.flatMap((input) => spec.run(client, input))
      ),
  };
};

const Id = Schema.Union(Schema.String, Schema.Number);
type Id = typeof Id.Type;

const Message = Schema.Struct({
  jsonrpc: Schema.Literal("2.0"),
  id: Schema.optional(Id),
  method: Schema.String,
  params: Schema.optional(Schema.Unknown),
});
const decodeMessage = Schema.decodeUnknownEither(Message);

const InitializeParams = Schema.Struct({
  protocolVersion: Schema.optional(Schema.String),
});
const CallParams = Schema.Struct({
  name: Schema.String,
  arguments: Schema.optional(Schema.Unknown),
});

const reply = (id: Id | null, result: unknown) =>
  JSON.stringify({ jsonrpc: "2.0", id, result });

const fail = (id: Id | null, code: number, message: string) =>
  JSON.stringify({ jsonrpc: "2.0", id, error: { code, message } });

/** The id of a malformed request, when it still carries a usable one. */
const idOf = (raw: unknown): Id | null => {
  if (typeof raw !== "object" || raw === null || !("id" in raw)) {
    return null;
  }
  return Either.getOrNull(Schema.decodeUnknownEither(Id)(raw.id));
};

const negotiate = (params: unknown) => {
  const asked = Either.getOrNull(
    Schema.decodeUnknownEither(InitializeParams)(params ?? {})
  )?.protocolVersion;
  return PROTOCOL_VERSIONS.find((v) => v === asked) ?? PROTOCOL_VERSIONS[0];
};

const listResult = <C>(tools: readonly McpTool<C>[]) => ({
  tools: tools.map((t) => ({
    name: t.name,
    description: t.description,
    inputSchema: t.inputSchema,
    annotations: { readOnlyHint: true, openWorldHint: false },
  })),
});

const contentOf = (out: string | ToolOutput) => {
  const { text, notes = [] } = typeof out === "string" ? { text: out } : out;
  return [text, ...(notes.length === 0 ? [] : [notes.join("\n")])].map((t) => ({
    type: "text",
    text: t,
  }));
};

/** Run a tool; any failure or defect becomes an `isError` result, never a crash. */
const callTool = <C>(tool: McpTool<C>, client: C, args: unknown) =>
  Effect.suspend(() => tool.call(client, args)).pipe(
    Effect.map((out) => ({ content: contentOf(out) })),
    Effect.catchAllCause((cause) =>
      Effect.succeed({
        content: [{ type: "text", text: formatCliError(Cause.squash(cause)) }],
        isError: true,
      })
    )
  );

const dispatch = <C>(
  tools: readonly McpTool<C>[],
  client: C,
  id: Id,
  method: string,
  params: unknown
): Effect.Effect<string> => {
  switch (method) {
    case "initialize":
      return Effect.succeed(
        reply(id, {
          protocolVersion: negotiate(params),
          capabilities: { tools: { listChanged: false } },
          serverInfo: { name: "peektrace", version: APP_VERSION },
        })
      );
    case "ping":
      return Effect.succeed(reply(id, {}));
    case "tools/list":
      return Effect.succeed(reply(id, listResult(tools)));
    case "tools/call": {
      const call = Schema.decodeUnknownEither(CallParams)(params);
      if (Either.isLeft(call)) {
        return Effect.succeed(
          fail(id, ErrorCode.invalidParams, "tools/call needs a tool name")
        );
      }
      const tool = tools.find((t) => t.name === call.right.name);
      if (tool === undefined) {
        return Effect.succeed(
          fail(id, ErrorCode.invalidParams, `Unknown tool: ${call.right.name}`)
        );
      }
      return callTool(tool, client, call.right.arguments).pipe(
        Effect.map((result) => reply(id, result))
      );
    }
    default:
      return Effect.succeed(
        fail(id, ErrorCode.methodNotFound, `Method not found: ${method}`)
      );
  }
};

const parseJson = (line: string) =>
  Either.try(() => JSON.parse(line) as unknown);

/** One input line to one reply line, or `undefined` for blank lines and notifications. */
export const handleLine =
  <C>({
    tools,
    client,
  }: {
    readonly client: C;
    readonly tools: readonly McpTool<C>[];
  }) =>
  (line: string): Effect.Effect<string | undefined> => {
    if (line.trim() === "") {
      return Effect.succeed(undefined);
    }
    const raw = parseJson(line);
    if (Either.isLeft(raw)) {
      return Effect.succeed(fail(null, ErrorCode.parse, "Parse error"));
    }
    const message = decodeMessage(raw.right);
    if (Either.isLeft(message)) {
      return Effect.succeed(
        fail(idOf(raw.right), ErrorCode.invalidRequest, "Invalid request")
      );
    }
    const { id, method, params } = message.right;
    if (id === undefined) {
      return Effect.succeed(undefined);
    }
    return dispatch(tools, client, id, method, params);
  };
