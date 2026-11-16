/**
 * OpenCode sessions for the parity data dirs: one builder per group of
 * cases, written with `bun:sqlite` in the on-disk schema.
 */
import { Database } from "bun:sqlite";
import { mkdirSync } from "node:fs";
import { dirname } from "node:path";
import { createOpencodeSchema } from "../../packages/core/test/fixtures/opencode-db";

type Row = Record<string, unknown>;

const LONG_LINE = 199;
const BIG_TEXT = 4000;
/** Past `serde_json`'s default 128-level limit. */
const DEEP = 200;

const deep = (levels: number): unknown =>
  Array.from({ length: levels }).reduce<unknown>((v) => ({ d: v }), 1);

export const open = (path: string) => {
  mkdirSync(dirname(path), { recursive: true });
  const d = new Database(path);
  createOpencodeSchema(d);
  return d;
};

const insert = (d: Database, table: string, row: Row) => {
  const keys = Object.keys(row);
  d.query(
    `INSERT INTO ${table} (${keys.join(",")}) VALUES (${keys.map(() => "?").join(",")})`
  ).run(...(keys.map((k) => row[k]) as never[]));
};

/** Strings are stored as they are, so raw JSON text (`1e400`) survives. */
export const J = (v: unknown) =>
  typeof v === "string" ? v : JSON.stringify(v);

/** Row ids: `msg_*`/`prt_*` as given, else prefixed with the session id. */
const uid = (sid: string, id: string) =>
  id.startsWith("msg_") || id.startsWith("prt_") ? id : `${sid.slice(4)}_${id}`;

export const session = (d: Database, row: Row) => insert(d, "session", row);
export const message = (
  d: Database,
  sid: string,
  id: string,
  tc: unknown,
  data: unknown
) =>
  insert(d, "message", {
    id: uid(sid, id),
    session_id: sid,
    time_created: tc,
    data: J(data),
  });
export const part = (
  d: Database,
  sid: string,
  mid: string,
  id: string,
  data: unknown
) =>
  insert(d, "part", {
    id: uid(sid, id),
    message_id: uid(sid, mid),
    session_id: sid,
    time_created: 1,
    data: J(data),
  });

/** Every part type, a tool in each state, ids and `kind` in part data. */
export const richSession = (a: Database) => {
  session(a, {
    id: "ses_rich",
    project_id: "p1",
    directory: "/Users/demo/proj",
    title: "Rich session",
    version: "1.18.2",
    model: J({ id: "gpt-5.6-terra-fast", providerID: "openai" }),
    agent: "build",
    cost: 0.01,
    tokens_input: 8305,
    tokens_output: 28,
    tokens_reasoning: 0,
    tokens_cache_read: 8192,
    tokens_cache_write: 128,
    time_created: 1_784_148_745_485,
    time_updated: 1_784_148_748_261,
  });
  const s = "ses_rich";
  message(a, s, "msg_u1", 1, {
    role: "user",
    time: { created: 1_784_148_745_485 },
    agent: "build",
    model: { providerID: "openai", modelID: "gpt-5.6-terra-fast" },
  });
  part(a, s, "msg_u1", "prt_u1", {
    type: "text",
    text: "hey there, list the files\nsecond line",
  });
  part(a, s, "msg_u1", "prt_u2", {
    type: "text",
    text: "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHH1234 in /Users/demo/proj",
    synthetic: true,
  });
  message(a, s, "msg_a1", 1, {
    role: "assistant",
    modelID: "gpt-5.6-terra-fast",
    providerID: "openai",
    cost: 0,
    tokens: {
      total: 8311,
      input: 8305,
      output: 6,
      reasoning: 0,
      cache: { write: 128, read: 8192 },
    },
    time: { created: 1_784_148_745_500 },
  });
  const parts: [string, unknown][] = [
    ["prt_a01", { type: "step-start", snapshot: "abc" }],
    [
      "prt_a02",
      {
        type: "reasoning",
        text: "The user wants a file listing.",
        time: { start: 1, end: 2 },
      },
    ],
    ["prt_a03", { type: "reasoning", text: "" }],
    [
      "prt_a04",
      '{"type":"tool","tool":"glob","callID":"call-abc","state":{"status":"completed","input":{"pattern":"README*","10":1.0,"2":12345678901234567890,"b":1e21},"output":"README.md\\npackages/README.md","metadata":{"count":2},"title":"","time":{"start":1,"end":2}}}',
    ],
    [
      "prt_a05",
      {
        type: "tool",
        tool: "bash",
        callID: "call-err",
        state: {
          status: "error",
          input: { command: "exit 1" },
          error: "command failed",
          time: { start: 3, end: 4 },
        },
      },
    ],
    [
      "prt_a06",
      {
        type: "tool",
        tool: "read",
        callID: "call-run",
        state: { status: "running", input: { path: "src/app.ts" } },
      },
    ],
    ["prt_a07", { type: "tool", callID: 5, state: { status: "pending" } }],
    [
      "prt_a08",
      {
        type: "tool",
        tool: "x",
        state: { status: "weird", output: "only output" },
      },
    ],
    [
      "prt_a09",
      {
        type: "file",
        filename: "src/app.ts",
        mime: "text/plain",
        url: "file:///Users/demo/proj/src/app.ts",
      },
    ],
    [
      "prt_a10",
      { type: "file", mime: "image/png", url: "data:image/png;base64,AAAA" },
    ],
    ["prt_a11", { type: "patch", hash: "abc123", files: ["src/app.ts"] }],
    ["prt_a12", { type: "snapshot", snapshot: "snap-1" }],
    [
      "prt_a13",
      { type: "subtask", tool: "task", prompt: "do it", agent: "general" },
    ],
    ["prt_a14", { type: "agent", name: "reviewer" }],
    ["prt_a15", { type: "retry", attempt: 1, error: { name: "APIError" } }],
    ["prt_a16", { type: "step-finish", reason: "stop", cost: 0 }],
    [
      "prt_a17",
      {
        type: "text",
        text: "Done.",
        time: { created: 1_784_148_746_000 },
        id: "IGNORED",
        kind: "zzz",
      },
    ],
    [
      "prt_a18",
      { kind: "x", type: "compaction", auto: true, "1": "one", id: "dup" },
    ],
    ["prt_a19", "not json"],
  ];
  for (const [id, data] of parts) {
    part(a, s, "msg_a1", id, data);
  }
  part(a, s, "msg_orphan", "prt_orphan", { type: "text", text: "orphan part" });
  message(a, s, "msg_u2", 1_784_148_747_000, { role: "user" });
  part(a, s, "msg_u2", "prt_u3", { type: "text", text: "thanks" });
  message(a, s, "msg_a2", 1, {
    role: "assistant",
    model: { providerID: "openai", modelID: "gpt-5.5" },
    tokens: { input: 0, output: 0, reasoning: 0, cache: { read: 0, write: 0 } },
    time: { created: 1_784_148_747_500 },
  });
  part(a, s, "msg_a2", "prt_a20", { type: "text", text: "You are welcome." });
};

/** Cost sources: message costs only, a mix, unpriced and reasoning-only. */
export const costSessions = (a: Database) => {
  session(a, {
    id: "ses_msgcost",
    directory: "/w/msgcost",
    model: "gpt-5.5",
    cost: 0,
    time_created: 1000,
    time_updated: 5000,
  });
  const usage = (input: number, output: number, reasoning: number) => ({
    input,
    output,
    reasoning,
    cache: { read: 0, write: 0 },
  });
  message(a, "ses_msgcost", "m1", 1, {
    role: "assistant",
    modelID: "gpt-5.5",
    cost: 0.5,
    tokens: usage(1000, 100, 900),
    time: { created: 2000 },
  });
  message(a, "ses_msgcost", "m2", 1, {
    role: "assistant",
    modelID: "gpt-5.5",
    cost: 0.25,
    tokens: { input: 1000, output: 100, reasoning: 0, cache: {} },
    time: { created: 3000 },
  });
  message(a, "ses_msgcost", "m3", 1, {
    role: "assistant",
    modelID: "gpt-5.5",
    tokens: { input: 0, output: 0 },
    time: { created: 4000 },
  });
  const m = "ses_mixed";
  session(a, {
    id: m,
    directory: "/w/mixed",
    model: "not json {",
    time_created: 1000,
    time_updated: 6000,
  });
  message(a, m, "m1", 1, { role: "user", time: { created: 1500 } });
  part(a, m, "m1", "p1", { type: "text", text: "a".repeat(BIG_TEXT) });
  message(a, m, "m2", 1, {
    role: "assistant",
    modelID: "gpt-5.5",
    cost: 0.5,
    tokens: usage(1000, 100, 900),
    time: { created: 2000 },
  });
  part(a, m, "m2", "p2", { type: "text", text: "b".repeat(400) });
  message(a, m, "m3", 1, { role: "user", time: { created: 2500 } });
  part(a, m, "m3", "p3", { type: "text", text: "c".repeat(2000) });
  message(a, m, "m4", 1, {
    role: "assistant",
    modelID: "gpt-5.5",
    tokens: {
      input: 2000,
      output: 100,
      reasoning: 0,
      cache: { read: 1500, write: 0 },
    },
    time: { created: 3000 },
  });
  part(a, m, "m4", "p4", { type: "text", text: "d" });
  message(a, m, "m5", 1, {
    role: "assistant",
    modelID: "mystery-model",
    tokens: usage(0, 0, 50),
    time: { created: 3500 },
  });
  message(a, m, "m6", 1, { role: "assistant", time: { created: 3600 } });
  part(a, m, "m6", "p6", { type: "text", text: "no usage" });
};

/** SQL order: JSON time vs column vs id, a text `created`, a real one. */
export const orderSession = (a: Database) => {
  const s = "ses_order";
  session(a, { id: s, directory: "/", time_created: 0, time_updated: 0 });
  const rows: [string, number, unknown, string, string][] = [
    ["m_c", 100, { created: 3000 }, "p_c", "third by json"],
    ["m_a", 9_999_999_999, { created: 1000 }, "p_a", "first by json"],
    ["m_b", 2000, {}, "p_b", "second by column"],
    ["m_s", 1, { created: "500" }, "p_s", "string created sorts last"],
    ["m_r", 1, { created: 2000.75 }, "p_r", "real created"],
  ];
  for (const [mid, tc, time, pid, text] of rows) {
    message(a, s, mid, tc, { role: "user", time });
    part(a, s, mid, pid, { type: "text", text });
  }
  message(a, s, "m_0", 2000, { role: "user", time: { created: 2000 } });
  part(a, s, "m_0", "p_0b", { type: "text", text: "tie by id (part b)" });
  part(a, s, "m_0", "p_0a", { type: "text", text: "tie by id (part a)" });
};

/** JSON text that only `JSON.stringify` rules size right. */
export const textSession = (a: Database) => {
  const s = "ses_text";
  session(a, {
    id: s,
    directory: "/w/tëxt/プロジェクト",
    title: "Ünïcode \u2028 title",
    time_created: 1000,
    time_updated: 9500,
  });
  message(
    a,
    s,
    "m1",
    1,
    '{"role":"user","time":{"created":1200},"big":1e400,"neg":-1e400,"z":-0}'
  );
  part(
    a,
    s,
    "m1",
    "p1",
    '{"type":"text","text":"lone \\ud800 high, é, 😀, line\\u2028sep, tab\\t, nul\\u0000, /slash"}'
  );
  part(a, s, "m1", "p2", {
    type: "text",
    text: `${"x".repeat(LONG_LINE)}😀 after the cut`,
  });
  message(
    a,
    s,
    "m2",
    1,
    '{"role":"assistant","modelID":"gpt-5.5","cost":1e400,"tokens":{"input":1e400,"output":7,"cache":{"read":3,"write":0}},"time":{"created":1300}}'
  );
  part(
    a,
    s,
    "m2",
    "p3",
    '{"type":"patch","files":["ü.ts"],"n":1e400,"s":"\\udc00x","t":1.5e-7}'
  );
};

/** Small sessions for header and load edge cases. */
export const edgeSessions = (a: Database) => {
  session(a, { id: "ses_empty", time_created: 1000, time_updated: 0 });
  session(a, {
    id: "ses_neg",
    directory: "C:\\Users\\x\\proj",
    title: "",
    time_created: -5,
    time_updated: -5,
  });
  session(a, {
    id: "ses_dup",
    directory: "/w/dup-db1",
    title: "from db1",
    time_created: 1,
    time_updated: 7000,
  });
  session(a, {
    id: "ses_badmsg",
    directory: "/w/badmsg",
    title: "bad message json",
    time_created: 1,
    time_updated: 8000,
  });
  message(a, "ses_badmsg", "m1", 1, "{not json");
  session(a, {
    id: "ses_types",
    directory: 42,
    title: 7,
    version: 1.5,
    model: J({ id: 5 }),
    cost: "1.5",
    time_created: "100",
    time_updated: 9000.9,
  });
  session(a, {
    id: "ses_deep",
    directory: "/w/deep",
    title: "deeply nested JSON",
    time_created: 1,
    time_updated: 8100,
  });
  message(a, "ses_deep", "m1", 1, { role: "user", time: { created: 10 } });
  part(a, "ses_deep", "m1", "p1", {
    type: "text",
    text: "deep user part",
    metadata: deep(DEEP),
  });
  message(a, "ses_deep", "m2", 2, {
    role: "assistant",
    modelID: "claude-sonnet-4-5",
    providerID: "anthropic",
    tokens: { input: 10, output: 5 },
    time: { created: 20, completed: 30 },
    deep: deep(DEEP),
  });
  part(a, "ses_deep", "m2", "p2", {
    type: "tool",
    tool: "mcp_config",
    callID: "c1",
    state: {
      status: "completed",
      input: deep(DEEP),
      output: "ok",
      time: { start: 21, end: 22 },
    },
  });
};
