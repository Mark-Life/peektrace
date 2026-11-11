/**
 * Generated edge-case Claude transcripts for `parity.ts`. Paths are relative
 * to a projects root (`<slug>/<file>`). Each file targets one rule of the
 * `sessions ls` header fold, so a parity diff names the rule that broke.
 */

const BOM = "\uFEFF";
/** `é` in Latin-1: not valid UTF-8 on its own. */
const LATIN1_E_ACUTE = Buffer.from("e9", "hex");
/** Bytes that never occur in UTF-8. */
const NOT_UTF8 = Buffer.from("fffe", "hex");
/** 1280 / 1024 = 1.25: `toFixed(1)` must round the tie up to `1.3 KB`. */
const KB_TIE_BYTES = 1280;
/** 1.5 MB exactly. */
const MB_BYTES = 1_572_864;
const line = (o: unknown) => JSON.stringify(o);
const lines = (...rows: unknown[]) => `${rows.map(line).join("\n")}\n`;

const assistant = (
  requestId: string | undefined,
  model: unknown,
  usage: unknown
) => ({
  type: "assistant",
  ...(requestId === undefined ? {} : { requestId }),
  timestamp: "2026-02-01T00:00:00.000Z",
  message: { model, usage },
});

const basic = [
  line({
    type: "user",
    timestamp: "2026-01-01T10:00:00.000Z",
    cwd: "",
    gitBranch: "main",
    message: { role: "user", content: "hi" },
  }),
  line({
    type: "user",
    cwd: "/tmp/edge/a",
    isMeta: true,
    message: { content: "meta" },
  }),
  line(assistant("r1", "<synthetic>", { input_tokens: 5 })),
  line(
    assistant("r2", "claude-opus-5-5", {
      input_tokens: 1000,
      cache_read_input_tokens: 2000,
      cache_creation_input_tokens: 3000,
      cache_creation: { ephemeral_1h_input_tokens: 1000 },
      output_tokens: 10,
    })
  ),
  line(
    assistant("r2", "claude-opus-5-5", { input_tokens: 1, output_tokens: 50 })
  ),
  line(assistant(undefined, "mystery-model", { input_tokens: 7 })),
  line({
    type: "user",
    message: { content: [{ type: "tool_result", content: "x" }] },
  }),
  "not json",
  line({ type: "ai-title", aiTitle: "First title" }),
  line({ type: "ai-title", aiTitle: "Second title" }),
  line({
    type: "user",
    timestamp: "2026-01-01T09:00:00.000Z",
    gitBranch: "feat",
  }),
  '{"type":"user","timestamp":',
].join("\n");

const usage = lines(
  assistant("u1", "claude-opus-5-5", []),
  assistant("u1", "claude-opus-5-5", { input_tokens: 100, output_tokens: 3 }),
  assistant("u2", "claude-opus-5-5", null),
  {
    type: "assistant",
    requestId: "u3",
    message: { usage: { input_tokens: 10 } },
  },
  assistant("u4", "claude-haiku-5-5", {
    input_tokens: 150_000,
    output_tokens: 10,
  }),
  assistant("u5", "claude-sonnet-4-5-20250929", {
    input_tokens: 123,
    output_tokens: 7,
  }),
  assistant("u6", "claude-opus-latest", { cache_read_input_tokens: 9999 }),
  assistant("u7", "anthropic.claude-haiku-4-5-20251001-v1:0", {
    input_tokens: 77,
  }),
  assistant("u8", "claude-3-opus-20240229", { input_tokens: 5 }),
  assistant("u9", "gpt-5.1-codex", { input_tokens: 1234, output_tokens: 99 }),
  assistant("u10", " Claude-Opus-4-1[1m] ", {
    input_tokens: 31,
    output_tokens: 1,
  }),
  assistant("u11", "us.anthropic.claude-sonnet-4-20250514-v1:0", {
    input_tokens: 3,
  }),
  assistant("u12", "claude-opus-5-5", {
    input_tokens: 1.5,
    output_tokens: 2.25,
  }),
  assistant("u13", "claude-opus-5-5", {
    cache_creation_input_tokens: 100,
    cache_creation: { ephemeral_1h_input_tokens: 500 },
  }),
  assistant("u14", "claude-opus-5-5", { input_tokens: 10, output_tokens: 40 }),
  assistant("u14", "claude-opus-5-5", {
    input_tokens: 99_999,
    output_tokens: 70,
  }),
  assistant("u14", "claude-opus-5-5", { output_tokens: 20 }),
  assistant("u15", "<synthetic>", { input_tokens: 10 }),
  assistant("u16", "claude-opus-5-5", { input_tokens: "10" }),
  assistant("u17", "claude-fable-5-1", { input_tokens: 17, output_tokens: 3 }),
  assistant("u18", "claude-opus-5-5", { input_tokens: 1, cache_creation: [] })
);

/** A file of exactly `size` bytes, so `1.3 KB`-style rounding ties show up. */
const padded = (size: number) => {
  const head = '{"type":"user","pad":"';
  const tail = '"}\n';
  return `${head}${"x".repeat(size - head.length - tail.length)}${tail}`;
};

const encoder = new TextEncoder();
const bytes = (...parts: (string | Uint8Array)[]) =>
  Buffer.concat(
    parts.map((p) => (typeof p === "string" ? encoder.encode(p) : p))
  );

const TIE_IDS = ["B", "a", "_x", "-y", "10", "9", "A1", "a2", "x.y", "x-y"];

export const edgeFiles: Record<string, string | Uint8Array> = {
  "-edge-a/basic.jsonl": basic,
  "-edge-a/empty.jsonl": "",
  "-edge-a/ws.jsonl": "\n \n\t\n",
  "-edge-a/bom.jsonl": `${BOM}${line({ type: "user", timestamp: "2026-02-01T00:00:00.000Z" })}\n`,
  "-edge-a/bom-only.jsonl": BOM,
  "-edge-a/crlf.jsonl": `${line({ type: "user", cwd: "/tmp/edge/crlf" })}\r\n\r\n${line({ type: "user" })}\r\n`,
  "-edge-a/bad-utf8.jsonl": bytes(
    '{"type":"ai-title","aiTitle":"caf',
    LATIN1_E_ACUTE,
    '!"}\n',
    NOT_UTF8,
    "\n",
    line({ type: "user" }),
    "\n"
  ),
  "-edge-a/nonobj.jsonl":
    '42\nnull\n[1,2]\n"s"\ntrue\n{"type":"assistant"}\n{"type":"assistant","requestId":"r"}\n{"type":"assistant","requestId":"r"}\n{"type":"assistant"}\n',
  "-edge-a/dup-keys.jsonl":
    '{"type":"assistant","type":"user","cwd":"/a","cwd":5}\n{"type":"user","cwd":"/b","gitBranch":"x"}\n{"gitBranch":""}\n',
  "-edge-a/rawtab.jsonl":
    '{"type":"ai-title","aiTitle":"a\tb"}\n{"type":"user"}\n',
  "-edge-a/surrogate.jsonl":
    '{"type":"assistant","requestId":"\\ud800"}\n{"type":"assistant","requestId":"\\udc00"}\n{"type":"ai-title","aiTitle":"t\\ud83d\\ude00"}\n',
  "-edge-a/surrogate-title.jsonl":
    '{"type":"user","cwd":"/tmp/edge/\\udc00"}\n{"type":"ai-title","aiTitle":"x\\ud800y"}\n',
  "-edge-a/unicode.jsonl": lines(
    { type: "user", cwd: "/tmp/edge/ü", timestamp: "2026-02-02T00:00:00.000Z" },
    { type: "ai-title", aiTitle: '日本語 🎉 title \u0001 "q" \\ end' }
  ),
  "-edge-a/timestamps.jsonl": lines(
    { timestamp: "" },
    { timestamp: "2026-03-01T00:00:00Z" },
    { timestamp: "2026-03-01T10:00" },
    { timestamp: 5 }
  ),
  "-edge-a/garbage-ts.jsonl": lines({ type: "user", timestamp: "not a date" }),
  "-edge-a/usage.jsonl": usage,
  "-edge-a/tiny-cost.jsonl": lines(
    assistant("t", "claude-haiku-5-5", { input_tokens: 1 })
  ),
  "-edge-a/half-cent.jsonl": lines(
    assistant("h", "claude-opus-5-5", { input_tokens: 31_250 })
  ),
  "-edge-a/size-1280.jsonl": padded(KB_TIE_BYTES),
  "-edge-a/size-mb.jsonl": padded(MB_BYTES),
  "-edge-a/notes.txt": "not a transcript",
  "-edge-a/basic.jsonl.bak": basic,
  "-edge-b/basic.jsonl": basic,
  ...Object.fromEntries(
    TIE_IDS.map((id) => [`-edge-b/${id}.jsonl`, lines({ type: "user" })])
  ),
};

/** `settings.json` with a real extra root, a missing one, a duplicate of the
 * default, an invalid entry and a duplicate path. */
export const edgeSettings = (work: string, missing: string) =>
  JSON.stringify({
    roots: {
      claude: [
        { path: work, label: "work" },
        { path: missing },
        { path: "~/.claude" },
        { path: 5 },
        { path: `${work}/`, label: "again" },
      ],
      codex: "nope",
    },
  });
