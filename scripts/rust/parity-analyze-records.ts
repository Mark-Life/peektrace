/**
 * Record builders and edge-case Claude transcripts for
 * `parity-analyze-fixtures.ts`. Each transcript targets one rule of the full
 * parse, analysis, redaction or export, so a diff names the rule.
 */

const BOUNDARY = "__SYSTEM_PROMPT_DYNAMIC_BOUNDARY__";
export const MEMORY_LINES = 250;
export const WIDE_MEMORY = 26_000;

export const L = (o: unknown) => JSON.stringify(o);
export const doc = (...rows: unknown[]) =>
  `${rows.map((r) => (typeof r === "string" ? r : L(r))).join("\n")}\n`;
const at = (n: number) =>
  `2026-03-01T10:${String(Math.floor(n / 60)).padStart(2, "0")}:${String(n % 60).padStart(2, "0")}.000Z`;

const usage = (input: number, output: number, extra: object = {}) => ({
  input_tokens: input,
  cache_read_input_tokens: input * 2,
  cache_creation_input_tokens: 10,
  output_tokens: output,
  ...extra,
});

export const user = (n: number, content: unknown, extra: object = {}) => ({
  type: "user",
  timestamp: at(n),
  message: { role: "user", content },
  ...extra,
});

export const asst = (
  n: number,
  req: string,
  content: unknown,
  extra: object = {},
  model: unknown = "claude-opus-4-5"
) => ({
  type: "assistant",
  timestamp: at(n),
  requestId: req,
  message: { model, content, usage: usage(100 * n, 20) },
  ...extra,
});

export const text = (t: string) => ({ type: "text", text: t });
export const tool = (id: string, input: unknown, name: unknown = "Bash") => ({
  type: "tool_use",
  id,
  name,
  input,
});
export const result = (id: string, content: unknown, extra: object = {}) => ({
  type: "tool_result",
  tool_use_id: id,
  content,
  ...extra,
});
const att = (n: number, attachment: unknown, extra: object = {}) => ({
  type: "attachment",
  timestamp: at(n),
  attachment,
  ...extra,
});
const image = (mt: string) => ({
  type: "image",
  source: { type: "base64", media_type: mt, data: "iVBORw0KGgo=" },
});

export const rewind = doc(
  { ...user(1, "hello"), uuid: "a", parentUuid: null },
  { ...asst(2, "r1", [text("hi")]), uuid: "b", parentUuid: "a" },
  { ...user(3, "first try"), uuid: "c", parentUuid: "b" },
  { ...asst(4, "r2", [text("dead answer")]), uuid: "d", parentUuid: "c" },
  { ...user(5, "second try"), uuid: "e", parentUuid: "b" },
  { ...asst(6, "r3", [text("live answer")]), uuid: "f", parentUuid: "e" }
);

export const rewindDeadLeaf = doc(
  { ...user(1, "hello"), uuid: "a", parentUuid: null },
  { ...asst(2, "r1", [text("hi")]), uuid: "b", parentUuid: "a" },
  { ...user(3, "first try"), uuid: "c", parentUuid: "b" },
  { ...asst(4, "r2", [text("a")]), uuid: "d", parentUuid: "c" },
  { ...user(5, "second try"), uuid: "e", parentUuid: "b" },
  { ...asst(6, "r3", [text("b")]), uuid: "f", parentUuid: "e" },
  { ...asst(7, "r4", [text("c")]), uuid: "g", parentUuid: "d" }
);

export const compaction = doc(
  user(1, "start"),
  asst(2, "r1", [text("working")]),
  {
    type: "system",
    subtype: "compact_boundary",
    timestamp: at(3),
    content: "Conversation compacted",
    compactMetadata: { preTokens: 150_000, postTokens: 9000 },
  },
  { ...user(4, "Summary of the work so far."), isCompactSummary: true },
  asst(5, "r2", [text("continuing")]),
  {
    ...user(6, [text("second summary")]),
    isCompactSummary: true,
    isMeta: true,
  },
  { type: "summary", summary: "Rolling summary text", leafUuid: "x" },
  { type: "system", subtype: "agents_killed", timestamp: at(7) },
  { type: "system", timestamp: at(8) },
  { type: "relocated", timestamp: at(9) },
  { type: "123", timestamp: at(9) },
  { type: "frame-link" },
  { type: "relocated" }
);

const toolA = {
  name: "Read",
  description: "Read a file",
  input_schema: {
    type: "object",
    properties: { path: { type: "string", description: "Path\nto read" } },
    required: ["path"],
  },
};
export const snapshots = doc(
  att(1, {
    type: "prompt_snapshot",
    cliPrefix: "You are Claude Code.",
    systemPrompt: ["Static part", BOUNDARY, "Dynamic part"],
    tools: [
      toolA,
      { schema: { name: "Wire", description: "x" }, name: "Wire" },
    ],
  }),
  user(2, "go"),
  asst(3, "r1", [text("ok")]),
  att(4, {
    type: "prompt_snapshot",
    cliPrefix: "You are Claude Code.",
    systemPrompt: ["Static part", BOUNDARY, "Dynamic part"],
    tools: [toolA],
  }),
  att(
    5,
    { type: "prompt_snapshot", systemPrompt: ["Sub prompt"] },
    { isSidechain: true, agentId: "sub1" }
  ),
  att(6, {
    type: "prompt_snapshot",
    cliPrefix: "You are Claude Code.",
    systemPrompt: ["Static part", BOUNDARY, "Dynamic part"],
    tools: [toolA],
  }),
  att(7, { type: "prompt_snapshot", systemPrompt: ["  "] })
);

export const queued = doc(
  user(1, "first"),
  asst(2, "r1", [text("busy")]),
  att(3, { type: "queued_command", commandMode: "prompt", prompt: "do more" }),
  att(4, {
    type: "queued_command",
    commandMode: "prompt",
    prompt: [text("look"), image("image/png"), text("")],
  }),
  att(5, {
    type: "queued_command",
    commandMode: "prompt",
    prompt: "batched",
    renderedByBatchHead: true,
  }),
  att(6, {
    type: "queued_command",
    commandMode: "prompt",
    prompt: "<command-name>/x</command-name>",
    isMeta: true,
  }),
  att(7, { type: "queued_command", commandMode: "prompt", prompt: "" }),
  att(
    8,
    { type: "queued_command", commandMode: "prompt", prompt: "shown" },
    { rendered: [{ content: "rendered text" }, { content: "" }] }
  ),
  asst(9, "r2", [text("done")])
);

export const attachments = doc(
  user(1, "start"),
  att(2, { type: "file", filename: "/x/CLAUDE.md", content: "rules" }),
  att(3, {
    type: "file",
    filename: "/x/a.ts",
    content: { file: { content: "code" } },
  }),
  att(4, { type: "file", displayPath: "b/", content: { other: 1 } }),
  att(5, { type: "skill_listing", skillCount: 2, content: "a\nb" }),
  att(6, { type: "skill_listing", content: ["x"] }),
  att(7, { type: "selected_lines_in_ide", content: { lines: [1, 2] } }),
  att(8, { type: "agent_listing_delta", addedLines: ["a", null, 3] }),
  att(9, { type: "agent_listing_delta" }),
  att(10, { type: "deferred_tools_delta", addedNames: ["A", "B"] }),
  att(11, { type: "mcp_instructions_delta", addedBlocks: ["x", "y"] }),
  att(12, { type: "mcp_instructions_delta", other: true }),
  att(13, { type: "edited_text_file", filename: "c.ts", snippet: "diff" }),
  att(14, {
    type: "hook_success",
    hookEvent: "Stop",
    command: "lint",
    stdout: "ok",
  }),
  att(15, {
    type: "hook_success",
    hookEvent: "SessionStart",
    command: "c",
    stderr: "e",
  }),
  att(16, { type: "hook_success", command: "old" }),
  att(17, {
    type: "instructions",
    files: [{ path: "/p/CLAUDE.md", content: "x" }],
  }),
  att(18, { type: "instructions" }),
  att(19, {
    type: "nested_memory",
    path: "/n/CLAUDE.md",
    content: { content: "mem" },
  }),
  att(20, { type: "nested_memory", displayPath: "n", content: "raw" }),
  att(21, {
    type: "date",
    content: "<system-reminder>\n  Today is x\n</system-reminder>",
  }),
  att(22, { type: "thinking_drop", content: "meta only" }),
  att(23, { type: "plan_mode" }),
  att(24, { type: "goals" }, { rendered: [{ content: "rendered goal" }] }),
  att(25, null),
  att(26, "plain"),
  att(27, { type: 7, content: "" }),
  att(28, {
    type: "opened_file_in_ide",
    filename: "/a/b/AGENTS.md",
    content: "x",
  }),
  asst(29, "r1", [text("done")])
);

export const results = doc(
  user(1, "start"),
  asst(2, "r1", [tool("t1", { command: "ls" })]),
  user(3, [
    result("t1", [text("out"), image("image/png"), image("image/svg+xml")], {
      is_error: "yes",
    }),
    result("t2", "boom", { is_error: true }),
    result("t3", null),
    { type: "tool_result", content: { a: 1 } },
    image("image/jpeg"),
    { type: "image", mimeType: "image/gif" },
    text(
      "<task-notification><status>done</status><summary>All\ngood</summary></task-notification>"
    ),
    text(
      " <command-name>/model</command-name> <command-args> opus </command-args>"
    ),
    text("<bash-stdout>out</bash-stdout>"),
    null,
    "str",
  ]),
  asst(4, "r2", [text("done")]),
  { ...user(5, "sdk prompt"), promptSource: "sdk" },
  { ...user(6, "wake up"), turnOrigin: "scheduled" },
  { ...user(7, "meta"), isMeta: true },
  { ...user(8, "notified"), origin: { kind: "task-notification" } },
  asst(9, "r3", [text("end")])
);

export const thinking = doc(
  user(1, "think"),
  {
    ...asst(2, "r1", [
      { type: "thinking", thinking: "Long reasoning here", signature: "s" },
      { type: "thinking", thinking: "", signature: "s" },
      { type: "redacted_thinking", data: "x" },
      text("answer"),
    ]),
    thinkingDurationMs: 1234,
  },
  {
    type: "assistant",
    timestamp: at(3),
    requestId: "r1",
    message: {
      model: "claude-opus-4-5",
      content: [],
      usage: {
        output_tokens: 900,
        output_tokens_details: { thinking_tokens: 700 },
      },
    },
  },
  user(4, "again"),
  asst(5, "r2", [{ type: "thinking", thinking: "" }, text("x")], {
    thinkingDurationMs: "slow",
  })
);

export const streamed = doc(
  user(1, "go"),
  asst(2, "r1", [text("part one")], { effort: "high" }),
  {
    type: "assistant",
    timestamp: at(3),
    requestId: "r1",
    message: {
      model: "claude-opus-4-5",
      content: [text("part two")],
      usage: {
        output_tokens: 50,
        output_tokens_details: { thinking_tokens: 10 },
        server_tool_use: { web_search_requests: 2, web_fetch_requests: 0 },
      },
    },
  },
  {
    type: "assistant",
    timestamp: at(4),
    requestId: "r1",
    message: {
      model: "claude-opus-4-5",
      content: [],
      usage: { output_tokens: 40, server_tool_use: { web_fetch_requests: 1 } },
    },
  },
  user(5, "next"),
  asst(6, "r2", [text("ok")])
);

export const models = doc(
  user(1, "go"),
  asst(2, "r1", [text("synthetic")], {}, "<synthetic>"),
  {
    type: "assistant",
    timestamp: at(3),
    message: { content: [text("no model")], usage: usage(5, 1) },
  },
  asst(4, "r3", [text("numeric")], {}, 5),
  asst(5, "r4", [text("object")], {}, { id: "x" }),
  asst(6, "r5", [text("unknown model")], {}, "mystery-model"),
  { type: "cost-state", modelUsage: { "claude-opus-4-5[1m]": { cost: 1 } } },
  asst(7, "r6", [text("zero")], {}, "claude-sonnet-4-5"),
  {
    type: "assistant",
    timestamp: at(8),
    requestId: "r7",
    message: { model: "claude-haiku-4-5", content: [text("no usage")] },
  }
);

export const oddContent = doc(
  user(1, "go"),
  asst(2, "r1", "abc"),
  asst(3, "r2", [
    { type: "mystery" },
    text("after unknown"),
    { type: "mystery" },
  ]),
  { ...asst(4, "r3", [text("side")]), isSidechain: true },
  { type: "assistant", timestamp: at(5), message: null },
  { type: "user", timestamp: at(6), message: "string message" },
  { type: "user", timestamp: at(7) }
);

export const server = doc(
  user(1, "search"),
  asst(2, "r1", [
    {
      type: "server_tool_use",
      id: "s1",
      name: "web_search",
      input: { query: "q" },
    },
    {
      type: "web_search_tool_result",
      tool_use_id: "s1",
      content: [
        {
          type: "web_search_result",
          title: "A",
          url: "https://a",
          page_age: "1 day",
        },
        { type: "web_search_result", url: "https://b" },
      ],
    },
    {
      type: "web_fetch_tool_result",
      tool_use_id: "s2",
      content: {
        url: "https://c",
        content: { title: "C", source: { type: "text", data: "page text" } },
      },
    },
    { type: "web_fetch_tool_result", content: { url: "https://d" } },
    { type: "web_search_tool_result", content: { error_code: "max_uses" } },
    {
      type: "advisor_tool_result",
      tool_use_id: "a1",
      content: { type: "advisor_redacted_result" },
    },
    { type: "advisor_tool_result", content: { text: "advice" } },
    { type: "advisor_tool_result" },
    { type: "code_execution_tool_result", content: [text("ran"), { x: 1 }] },
  ])
);

export const numbers = [
  L(user(1, "go")),
  `{"type":"assistant","timestamp":"${at(2)}","requestId":"r1","message":{"model":"claude-opus-4-5","usage":{"input_tokens":10.0,"output_tokens":1e1},"content":[{"type":"tool_use","id":"t","name":"X","input":{"b":1,"10":2,"2":3,"b":4,"x":1.0,"y":1e21,"z":1e-7,"big":12345678901234567890,"neg":-0,"s":"\\ud83d tail"}}]}}`,
  `{"type":"assistant","timestamp":"${at(3)}","requestId":"r2","message":{"model":"claude-opus-4-5","usage":{"input_tokens":1e400},"content":[{"type":"tool_use","name":"Y","input":"just a string"}]}}`,
  "",
].join("\n");

const emoji = "\u{1F600}";
export const surrogate = doc(
  user(1, `${"a".repeat(199)}${emoji} tail`),
  asst(2, "r1", [text(`${"b".repeat(199)}${emoji}`)])
);

export const bomCrlf = `\uFEFF${[L(user(1, "bom first")), L(asst(2, "r1", [text("crlf")]))].join("\r\n")}\r\n`;

export const cwdEmpty = doc(
  { ...user(1, "x"), cwd: "", gitBranch: "", version: "" },
  { type: "ai-title", aiTitle: "" },
  asst(2, "r1", [text("y")])
);

export const nonIsoTime = doc(
  { ...user(1, "x"), timestamp: "2026-01-07T10:00:00" },
  { ...asst(2, "r1", [text("y")]), timestamp: "2026-01-31T12:00:00" },
  { ...user(3, "z"), timestamp: "" }
);

export const badTime = doc(
  { ...user(1, "x"), timestamp: "not a date" },
  { ...asst(2, "r1", [text("y")]), timestamp: "also bad" }
);
