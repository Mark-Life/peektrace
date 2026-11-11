/**
 * Codex and Pi trees for the parity harness. `buildAgentsEdge` writes edge
 * transcripts under a fixture HOME (`.codex/sessions`, `.pi/agent/sessions`),
 * each file targeting one rule of discovery, the list header or the full
 * parse. `buildPiRoot` copies the TS Pi fixtures under one slug dir.
 */
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  readdirSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";

const NO_ACCESS = 0o000;
const BOM = "﻿";
const LONG_LINE = 199;
const WINDOW_TOKENS = 300_000;
const L = (o: unknown) => JSON.stringify(o);
const doc = (...rows: unknown[]) =>
  `${rows.map((r) => (typeof r === "string" ? r : L(r))).join("\n")}\n`;
const at = (n: number) =>
  `2026-01-02T10:${String(Math.floor(n / 60)).padStart(2, "0")}:${String(n % 60).padStart(2, "0")}.000Z`;
const uuid = (n: number, head = "019f0000") =>
  `${head}-0000-7000-8000-${String(n).padStart(12, "0")}`;

// Codex rollout rows.
const row = (n: number, type: string, payload: unknown) => ({
  timestamp: at(n),
  type,
  payload,
});
const meta = (n: number, payload: object) => row(n, "session_meta", payload);
const ctx = (n: number, payload: object) => row(n, "turn_context", payload);
const item = (n: number, payload: object) => row(n, "response_item", payload);
const ev = (n: number, payload: object) => row(n, "event_msg", payload);
const msg = (n: number, role: unknown, ...texts: unknown[]) =>
  item(n, {
    type: "message",
    role,
    content: texts.map((t) => ({ type: "input_text", text: t })),
  });
const usage = (input: number, cached: number, output: number) => ({
  input_tokens: input,
  cached_input_tokens: cached,
  output_tokens: output,
});
const tokens = (n: number, last: unknown, total?: unknown, extra = {}) =>
  ev(n, {
    type: "token_count",
    info: {
      last_token_usage: last,
      ...(total === undefined ? {} : { total_token_usage: total }),
      ...extra,
    },
  });
const call = (n: number, name: unknown, args: unknown, id = `c${n}`) =>
  item(n, { type: "function_call", name, arguments: args, call_id: id });
const output = (n: number, out: unknown, id = `c${n}`) =>
  item(n, { type: "function_call_output", call_id: id, output: out });

const codexTokens = doc(
  meta(0, { id: uuid(1), cwd: "/tmp/demo", cli_version: "0.1.0" }),
  ctx(1, { model: "gpt-5.5", cwd: "/ignored" }),
  ev(2, { type: "token_count", info: null }),
  item(3, { type: "reasoning", summary: [] }),
  tokens(4, usage(100, 40, 5)),
  call(5, "shell", '{"cmd":"ls"}'),
  tokens(6, usage(200, 50, 6), { a: 1, b: 2 }),
  tokens(7, usage(210, 50, 6), { a: 1, b: 2 }),
  tokens(8, usage(220, 50, 6), { b: 2, a: 1 }),
  tokens(9, undefined, { a: 3 }),
  call(10, "shell", "{}"),
  tokens(11, usage(0, 0, 1), { a: 4 }),
  output(12, "after zero"),
  tokens(13, usage(300, 310, 7), { a: 5 }, { model_context_window: 400_000 }),
  ev(14, {
    type: "task_started",
    turn_id: "turn-1",
    model_context_window: 258_400,
  }),
  item(15, { type: "message", role: "assistant", content: [{ text: "a" }] }),
  ev(16, { type: "task_started" }),
  tokens(17, usage(400, 0, 8), { a: 6 }),
  item(18, { type: "message", role: "assistant", content: [{ text: "b" }] }),
  tokens(19, usage(500, 0, 9), { a: 7 }),
  ctx(20, { model: "gpt-5.4", turn_id: "turn-1" }),
  call(21, "", "5"),
  tokens(22, usage(600, 100, 10), { a: 8 }),
  ev(23, { type: "task_started", turn_id: "turn-1" }),
  call(24, null, "x"),
  tokens(25, usage(700, 100, 11), { a: 9 })
);

const codexItems = doc(
  meta(0, { id: uuid(2), cwd: "/tmp/demo/items/", git: { branch: "main" } }),
  ctx(1, { model: "gpt-5.5", turn_id: "t" }),
  item(2, {
    type: "reasoning",
    summary: [
      { type: "summary_text", text: "plan\nmore" },
      { type: "summary_text", text: "" },
      { type: "other", text: "x" },
      5,
    ],
  }),
  item(3, { type: "reasoning", summary: [{ type: "summary_text" }] }),
  call(4, "a", "{oops"),
  call(5, "b", ""),
  call(6, "c", { b: 1, 10: 2 }),
  call(7, "d", '{"b":1,"10":2}'),
  call(8, "e", "1e21"),
  call(9, "f", "12345678901234567890"),
  call(10, "g", '"str"'),
  item(11, {
    type: "custom_tool_call",
    name: "apply_patch",
    input: "*** Begin\n  x",
    call_id: "cc",
  }),
  item(12, { type: "custom_tool_call", name: 7, input: { k: 1 } }),
  item(13, { type: "tool_search_call", arguments: null, call_id: "ts" }),
  item(14, { type: "tool_search_call", arguments: { q: "x" } }),
  item(15, { type: "web_search_call", call_id: "ws" }),
  item(16, { type: "web_search_call", action: { query: "rust\tserde" } }),
  item(17, { type: "web_search_call", action: [1] }),
  output(18, "Exit code 0"),
  output(19, "Process exited with code 1\nboom"),
  output(20, "EXIT CODE:9"),
  output(21, { stdout: "x", exit_code: "exit code 10" }),
  item(22, {
    type: "custom_tool_call_output",
    call_id: "cc",
    output: "exitcode 1",
  }),
  item(23, { type: "tool_search_output", call_id: "ts", tools: null }),
  item(24, { type: "tool_search_output", tools: [{ name: "t" }] }),
  item(25, { type: "local_shell_call" }),
  msg(
    26,
    "developer",
    "<skills_instructions>\nuse skills",
    "<a b-c>x",
    "<h1>title",
    "  plain",
    "<1x>y",
    "   "
  ),
  msg(
    27,
    "user",
    "<environment_context>\n<cwd>/x</cwd>",
    "  <user_instructions>u",
    "# AGENTS.md instructions for /x",
    "<recommended_plugins>p",
    "<skill>s",
    "<turn_aborted>t"
  ),
  msg(28, "user", "<skill>s", "real prompt", "second line"),
  msg(29, "user"),
  msg(30, "assistant"),
  msg(31, "system", "sys text"),
  msg(32, undefined, "no role"),
  item(33, { type: "message", role: "user", content: "not an array" }),
  item(34, {
    type: "agent_message",
    author: "reviewer",
    content: [{ text: "looks good" }],
  }),
  item(35, { type: "agent_message", content: [{ text: "anon" }] }),
  item(36, { type: "agent_message", content: [] }),
  tokens(37, usage(1000, 900, 20), { a: 1 }),
  ev(38, { type: "agent_message", message: "ui only" }),
  row(39, "world_state", {}),
  row(40, "compacted", { message: "x" })
);

const codexFiles: Record<string, string | Uint8Array> = {
  [`2026/01/02/rollout-2026-01-02T10-00-00-${uuid(1)}.jsonl`]: codexTokens,
  [`2026/01/02/rollout-2026-01-02T10-00-01-${uuid(2)}.jsonl`]: codexItems,
  [`2026/01/02/rollout-2026-01-02T10-00-02-${uuid(10, "019F00AB")}.jsonl`]: doc(
    ctx(1, { cwd: "/tmp/upper" }),
    msg(2, "user", "hi")
  ),
  [`2026/01/02/rollout-${uuid(3)}-${uuid(4)}.jsonl`]: doc(
    meta(0, { id: "", cwd: "/tmp/fork" }),
    meta(1, { id: "fork-child", forked_from_id: "parent" }),
    ctx(2, { model: "gpt-5.5" }),
    msg(3, "assistant", "replayed"),
    tokens(4, usage(100, 0, 1), { a: 1 }),
    call(5, "own", "{}"),
    tokens(6, usage(150, 0, 2), { a: 2 })
  ),
  "2026/01/02/rollout-nometa.jsonl": doc(
    ctx(1, { model: "", cwd: "" }),
    ctx(2, { model: "gpt-5.5", cwd: "/tmp/later" }),
    msg(3, "user", "hello"),
    msg(4, "assistant", "hi")
  ),
  "2026/01/02/rollout-two-meta.jsonl": doc(
    meta(0, {
      id: 123,
      cwd: "",
      git: { branch: "first" },
      cli_version: "1",
      base_instructions: { text: "You are Codex.\nBe brief." },
      dynamic_tools: [
        { name: "t", description: "d", input_schema: { type: "object" } },
      ],
    }),
    meta(1, {
      id: "second",
      cwd: "/tmp/demo",
      git: { branch: "last" },
      cli_version: "2",
      base_instructions: { text: 5 },
    }),
    meta(2, { git: [], base_instructions: "plain" }),
    ctx(3, { model: "" })
  ),
  "2026/01/02/rollout-prims.jsonl": `5\n"x"\n[1]\ntrue\n${L(row(1, "mystery", {}))}\n${L({ type: 5, payload: [] })}\nnot json\n${L(msg(2, "user", "ok"))}\n{"cut`,
  "2026/01/02/rollout-nullrow.jsonl": doc(
    meta(0, { id: "", cwd: "/tmp/demo" }),
    "null",
    msg(1, "user", "after null")
  ),
  "2026/01/02/rollout-bom.jsonl": `${BOM}${[
    L(meta(0, { id: "bom", cwd: "/tmp/bom" })),
    L(msg(1, "user", "crlf")),
  ].join("\r\n")}\r\n`,
  "2026/01/02/rollout-long.jsonl": doc(
    meta(0, { cwd: "/tmp/demo" }),
    msg(1, "user", `${"x".repeat(LONG_LINE)}😀 tail`),
    msg(2, "assistant", `${"y".repeat(LONG_LINE + 1)}`)
  ),
  "2026/01/02/rollout-cwd-empty.jsonl": doc(meta(0, { cwd: "" })),
  "2026/01/02/notrollout-x.jsonl": doc(meta(0, { id: "never" })),
  "2026/01/02/rollout-empty.jsonl": "",
  "2026/01/02/rollout-ws.jsonl": "  \n\n",
  "2026/01/02/rollout-d.jsonl/rollout-inner.jsonl": doc(
    meta(0, { id: "inner", cwd: "/tmp/inner" })
  ),
  "rollout-top.jsonl": doc(meta(0, { cwd: "/tmp/top" })),
};

const CODEX_LOCKED = "2026/01/02/rollout-locked.jsonl";

// Pi rows: a chain of entries, each the child of the one before unless it
// names its own `id` / `parentId`.
interface PiRow {
  readonly [key: string]: unknown;
}
const chain = (...rows: PiRow[]) => {
  let parent: unknown = null;
  return rows.map((r, i) => {
    if (r.type === "session") {
      return r;
    }
    const out = { id: `e${i}`, parentId: parent, timestamp: at(i), ...r };
    parent = out.id;
    return out;
  });
};
const session = (extra = {}) => ({
  type: "session",
  version: 3,
  id: "pi-session",
  timestamp: at(0),
  cwd: "/tmp/demo",
  ...extra,
});
const pmsg = (message: object) => ({ type: "message", message });
const said = (content: unknown) => pmsg({ role: "user", content });
const reply = (content: unknown, model: unknown = "claude-opus-4-8", u = {}) =>
  pmsg({
    role: "assistant",
    model,
    content,
    usage: { input: 10, cacheRead: 100, cacheWrite: 5, output: 7, ...u },
  });
const text = (t: string) => ({ type: "text", text: t });
const tc = (id: string, name: unknown, args: unknown) => ({
  type: "toolCall",
  id,
  name,
  arguments: args,
});
const res = (content: unknown, extra = {}) =>
  pmsg({
    role: "toolResult",
    toolCallId: "t1",
    toolName: "read",
    content,
    ...extra,
  });
const edit = (targetId: string, replacement: unknown) => ({
  type: "context_edit",
  targetId,
  replacement,
});

const piFork = doc(
  session(),
  {
    type: "model_change",
    id: "m1",
    parentId: null,
    timestamp: at(1),
    modelId: "gpt-5.5",
  },
  {
    type: "message",
    id: "u1",
    parentId: "m1",
    timestamp: at(2),
    message: { role: "user", content: "start" },
  },
  {
    type: "message",
    id: "a1",
    parentId: "u1",
    timestamp: at(3),
    message: {
      role: "assistant",
      model: "dead-model",
      content: [text("dead")],
      usage: { input: 50, output: 1 },
    },
  },
  {
    type: "session_info",
    id: "i1",
    parentId: "a1",
    timestamp: at(4),
    name: "Off branch title",
  },
  {
    type: "model_change",
    id: "m2",
    parentId: "a1",
    timestamp: at(5),
    modelId: "claude-sonnet-4-5",
  },
  {
    type: "context_edit",
    id: "x1",
    parentId: "a1",
    timestamp: at(6),
    targetId: "u1",
    replacement: null,
  },
  {
    type: "message",
    id: "a2",
    parentId: "u1",
    timestamp: at(7),
    message: {
      role: "assistant",
      model: "gpt-5.5",
      content: [text("live")],
      usage: { input: 60, cacheRead: 10, output: 2 },
    },
  }
);

const untrusted = (rows: PiRow[]) =>
  doc(session(), ...rows.map((r, i) => ({ timestamp: at(i + 1), ...r })));
const live = (model: unknown, content = [text("x")]) => ({
  type: "message",
  message: {
    role: "assistant",
    model,
    content,
    usage: { input: 5, output: 1 },
  },
});

const piEdits = doc(
  ...chain(
    session(),
    { type: "message", id: "s1", message: { role: "system", content: "sys" } },
    {
      type: "message",
      id: "u1",
      message: { role: "user", content: "first prompt" },
    },
    {
      type: "message",
      id: "a1",
      message: {
        role: "assistant",
        model: "claude-opus-4-8",
        content: [
          { type: "thinking", thinking: "hmm" },
          text("answer one"),
          tc("t1", "read", { path: "/a" }),
        ],
        usage: { input: 10, cacheRead: 100, output: 5 },
      },
    },
    {
      type: "message",
      id: "r1",
      message: {
        role: "toolResult",
        toolCallId: "t1",
        content: [text("file body")],
      },
    },
    {
      type: "custom_message",
      id: "c1",
      customType: "note",
      content: "custom text",
    },
    { type: "compaction", id: "k1", summary: "sum" },
    {
      type: "message",
      id: "u2",
      message: {
        role: "user",
        content: [
          text("second"),
          { type: "image", mimeType: "image/png", data: "x" },
        ],
      },
    },
    {
      type: "message",
      id: "a2",
      message: {
        role: "assistant",
        model: "claude-opus-4-8",
        content: [text("answer two")],
        usage: { input: 20, cacheRead: 150, output: 5 },
      },
    },
    edit("u1", null),
    edit("a1", "short answer"),
    edit("r1", { content: "trimmed" }),
    edit("r1", { content: [text("again")] }),
    edit("c1", { content: "" }),
    edit("u2", { content: "replaced prompt" }),
    edit("u2", null),
    edit("u2", { content: [text("final")] }),
    edit("a2", { content: [] }),
    edit("s1", null),
    edit("k1", null),
    edit("a2", { nope: 1 }),
    edit("missing", null),
    {
      type: "message",
      id: "a3",
      message: {
        role: "assistant",
        model: "claude-opus-4-8",
        content: [text("after edits")],
        usage: { input: 30, cacheRead: 160, output: 5 },
      },
    }
  )
);

const piContext = doc(
  ...chain(
    session({ version: 7 }),
    pmsg({
      role: "system",
      content: [text("You are Pi."), { type: "image" }],
      sections: { base: "Rules", gone: null, 10: "ten" },
      toolsAdded: [
        { name: "read", description: "Read", parameters: { type: "object" } },
      ],
    }),
    pmsg({
      role: "system",
      sections: "ab",
      toolsRemoved: ["x", { name: "bash" }, { name: "" }, null],
    }),
    pmsg({
      role: "system",
      sections: [5, "arr"],
      toolsAdded: [{ name: "w" }],
      toolsRemoved: [{ name: "edit" }],
    }),
    pmsg({ role: "system" }),
    {
      type: "compaction",
      summary: "first summary",
      systemMessage: { content: "after compaction" },
    },
    { type: "compaction", summary: { o: 1 }, systemMessage: [1] },
    pmsg({ role: "compactionSummary", summary: "role summary" }),
    pmsg({ role: "branchSummary" }),
    { type: "branch_summary", summary: "came back <summary>x</summary>" },
    {
      type: "custom_message",
      customType: "ext",
      content: [text("injected"), { type: "image", mimeType: "image/jpeg" }],
    },
    { type: "custom_message", content: "" },
    { type: "custom_message", content: "no type" },
    pmsg({ role: "custom", customType: 5, content: "custom role" }),
    pmsg({
      role: "bashExecution",
      command: "ls -la\nsecond",
      output: "a\nb",
      exitCode: 0,
    }),
    pmsg({
      role: "bashExecution",
      command: "make",
      output: "",
      exitCode: 2.5,
      truncated: true,
      fullOutputPath: "/tmp/full.txt",
    }),
    pmsg({
      role: "bashExecution",
      command: "sleep",
      cancelled: true,
      exitCode: 1,
    }),
    pmsg({
      role: "bashExecution",
      command: "secret",
      excludeFromContext: true,
    }),
    pmsg({ role: "bashExecution" }),
    said("plain string prompt"),
    said([
      text("a"),
      null,
      5,
      { type: "image", source: { media_type: "image/gif" } },
      { type: "image", mimeType: "image/png", media_type: "image/webp" },
      { type: "other" },
    ]),
    said(7),
    reply([
      { type: "thinking", thinking: "" },
      { type: "thinking", thinking: "deep" },
      text("hi"),
      tc("t1", "", null),
      tc("t2", undefined, "s"),
      { type: "toolCall", arguments: { b: 1, 10: 2 } },
      null,
      { type: "other" },
    ]),
    res([text("a"), { type: "image", mimeType: "image/png" }, text("b"), 5]),
    res("plain result", { isError: true }),
    res({ k: 1 }),
    res(null, { toolName: 5, toolCallId: 6 }),
    pmsg({ role: "mystery", content: "x" }),
    pmsg(5),
    { type: "thinking_level_change", thinkingLevel: "high" },
    { type: "usage", input: 5 },
    { type: "session_info", name: "  " },
    { type: "session_info", name: " Real title " }
  )
);

const piUsage = doc(
  ...chain(
    session(),
    reply([text("partial")], "gpt-5.5", {
      input: undefined,
      cacheRead: 30,
      output: undefined,
    }),
    reply([text("zero")], "gpt-5.5", { input: 0, cacheRead: 0, cacheWrite: 0 }),
    {
      type: "message",
      id: "dup",
      message: {
        role: "assistant",
        model: "gpt-5.5",
        content: [text("first")],
        usage: { input: 1, output: 1 },
      },
    },
    {
      type: "message",
      id: "dup2",
      message: {
        role: "assistant",
        model: "gpt-5.5",
        content: [text("no usage")],
      },
    },
    {
      type: "message",
      id: "e5",
      message: {
        role: "assistant",
        content: [text("no model")],
        usage: { input: 3 },
      },
    }
  ),
  {
    type: "message",
    id: "dup",
    parentId: "e5",
    timestamp: at(9),
    message: {
      role: "assistant",
      model: "gpt-5.5",
      content: [text("again")],
      usage: { input: 999, output: 9 },
    },
  }
);

const WINDOW_MODELS = [
  "claude-opus-4-8",
  "Claude-Sonnet-4-5",
  "claude-haiku-4-5",
  "claude-3",
  "openai/gpt-5.5",
  "gpt-oss-120b",
  "z-ai/glm-5.2",
  "glm-4.6",
  "qwen/qwen3.6-27b",
  "qwen-max",
  "gemma-3",
  "gemini-2.5-pro",
  "llama-4",
  "moonshotai/Kimi-K3",
];

const piFiles: Record<string, string> = {
  [`--tmp-demo--/2026-01-02T10-00-00-000Z_${uuid(1, "019f1111")}.jsonl`]:
    piFork,
  "--tmp-demo--/plain.jsonl": doc(
    ...chain(
      session({ id: "plain-id", cwd: "/tmp/plain" }),
      said("hi"),
      reply([text("yo")])
    )
  ),
  "--tmp-demo--/2026_nouuid.jsonl": doc(
    ...chain(session({ id: undefined }), said("no id"))
  ),
  "--tmp-demo--/x_.jsonl": doc(
    ...chain(session(), said("trailing underscore"))
  ),
  "--tmp-demo--/edits.jsonl": piEdits,
  "--tmp-demo--/context.jsonl": piContext,
  "--tmp-demo--/usage.jsonl": piUsage,
  "--tmp-demo--/noparent.jsonl": untrusted([
    {
      type: "message",
      id: "a",
      parentId: null,
      message: { role: "user", content: "x" },
    },
    {
      type: "message",
      id: "b",
      parentId: "zz",
      message: { role: "user", content: "y" },
    },
  ]),
  "--tmp-demo--/nokey.jsonl": untrusted([
    { type: "message", id: "a", message: { role: "user", content: "x" } },
    {
      type: "context_edit",
      id: "c",
      parentId: "a",
      targetId: "a",
      replacement: null,
    },
  ]),
  "--tmp-demo--/dupid.jsonl": untrusted([
    {
      type: "message",
      id: "a",
      parentId: null,
      message: { role: "user", content: "x" },
    },
    {
      type: "message",
      id: "a",
      parentId: "a",
      message: { role: "user", content: "y" },
    },
  ]),
  "--tmp-demo--/cycle.jsonl": untrusted([
    {
      type: "message",
      id: "a",
      parentId: "b",
      message: { role: "user", content: "x" },
    },
    {
      type: "message",
      id: "b",
      parentId: "a",
      message: { role: "user", content: "y" },
    },
  ]),
  "--tmp-demo--/prims.jsonl": `${L(session())}\n5\n"x"\n[1]\n${L({ type: "message", id: "a", parentId: null, message: { role: "user", content: "x" } })}\n`,
  "--tmp-demo--/pi-nullrow.jsonl": doc(...chain(session(), said("x")), "null", {
    type: "message",
    id: "z",
    parentId: "e1",
    message: { role: "user", content: "y" },
  }),
  "--tmp-demo--/nested/deep.jsonl": doc(session()),
  "--tmp-demo--/empty.jsonl": "",
  "--other--/win-over.jsonl": doc(
    ...chain(
      session({ id: undefined, cwd: "/tmp/other" }),
      reply([text("big")], "glm-5", { input: WINDOW_TOKENS })
    )
  ),
  "root.jsonl": doc(session()),
};

for (const [i, model] of WINDOW_MODELS.entries()) {
  piFiles[`--win--/win-${i}.jsonl`] = doc(
    ...chain(
      session({ id: undefined, cwd: "/tmp/win" }),
      { type: "model_change", modelId: "first" },
      live(model)
    )
  );
}

const PI_LOCKED = "--tmp-demo--/locked.jsonl";

const write = (path: string, body: string | Uint8Array) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, body);
};

/** Codex and Pi edge trees under `home`; returns the files made unreadable,
 * which the caller restores. */
export const buildAgentsEdge = (home: string, extra: string) => {
  const codex = join(home, ".codex", "sessions");
  const pi = join(home, ".pi", "agent", "sessions");
  for (const [rel, body] of Object.entries(codexFiles)) {
    write(join(codex, rel), body);
  }
  for (const [rel, body] of Object.entries(piFiles)) {
    write(join(pi, rel), body);
  }
  mkdirSync(join(pi, "--tmp-demo--", "dir.jsonl"), { recursive: true });
  symlinkSync(join(codex, "2026", "01"), join(codex, "2026", "02"));
  symlinkSync(join(pi, "--other--"), join(pi, "--link--"));
  write(
    join(extra, "codex", "sessions", "rollout-extra.jsonl"),
    doc(meta(0, { id: "extra-codex", cwd: "/tmp/demo" }))
  );
  write(
    join(extra, "pi", "agent", "sessions", "--x--", "extra.jsonl"),
    doc(...chain(session({ id: undefined, cwd: "/tmp/demo" }), said("x")))
  );
  const locked = [join(codex, CODEX_LOCKED), join(pi, PI_LOCKED)];
  for (const path of locked) {
    write(path, doc(meta(0, { id: "locked" })));
    chmodSync(path, NO_ACCESS);
  }
  return locked;
};

/** `settings.roots` entries for Codex and Pi: the default again, a labelled
 * extra and a missing dir. */
export const agentsSettings = (extra: string, missing: string) => ({
  codex: [
    { path: "~/.codex" },
    { path: join(extra, "codex"), label: "cx" },
    { path: missing },
  ],
  pi: [
    { path: "~/.pi" },
    { path: join(extra, "pi"), label: "px" },
    { path: missing },
  ],
});

/** The TS Pi fixtures copied under one slug dir of a new root. */
export const buildPiRoot = (fixtures: string, root: string) => {
  const slug = join(root, "--tmp-pi--");
  mkdirSync(slug, { recursive: true });
  const src = join(fixtures, "sessions-pi");
  for (const name of readdirSync(src)) {
    copyFileSync(join(src, name), join(slug, name));
  }
  return root;
};
