/**
 * Pi edge sessions for the parity harness, keyed by path under
 * `.pi/agent/sessions`. Each file targets one rule of discovery, the list
 * header or the full parse.
 */
import { at, doc, L, uuid } from "./parity-agents-rows";

const WINDOW_TOKENS = 300_000;

// Pi rows: a chain of entries, each the child of the one before unless it
// names its own `id` / `parentId`.
interface PiRow {
  readonly [key: string]: unknown;
}
export const chain = (...rows: PiRow[]) => {
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
export const session = (extra = {}) => ({
  type: "session",
  version: 3,
  id: "pi-session",
  timestamp: at(0),
  cwd: "/tmp/demo",
  ...extra,
});
const pmsg = (message: object) => ({ type: "message", message });
export const said = (content: unknown) => pmsg({ role: "user", content });
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

export const piFiles: Record<string, string> = {
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

export const PI_LOCKED = "--tmp-demo--/locked.jsonl";
