/**
 * Codex edge rollouts for the parity harness, keyed by path under
 * `.codex/sessions`. Each file targets one rule of discovery, the list
 * header or the full parse.
 */
import { at, doc, L, uuid } from "./parity-agents-rows";

const BOM = "\uFEFF";
const LONG_LINE = 199;

// Codex rollout rows.
const row = (n: number, type: string, payload: unknown) => ({
  timestamp: at(n),
  type,
  payload,
});
export const meta = (n: number, payload: object) =>
  row(n, "session_meta", payload);
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

export const codexFiles: Record<string, string | Uint8Array> = {
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

export const CODEX_LOCKED = "2026/01/02/rollout-locked.jsonl";
