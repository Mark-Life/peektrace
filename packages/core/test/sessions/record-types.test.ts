/** Claude record types and fields the parser used to drop (#111). Synthetic fixture only. */
import { describe, expect, test } from "bun:test";
import { Schema } from "effect";
import { analyze } from "../../src/services/sessions/analyze";
import { buildHeader } from "../../src/services/sessions/header";
import { parseClaudeSession } from "../../src/services/sessions/parse";
import { ParsedSession } from "../../src/services/sessions/schema";

const line = (o: Record<string, unknown>) => JSON.stringify(o);

const assistant = (o: {
  readonly requestId: string;
  readonly model: string;
  readonly usage: Record<string, unknown>;
  readonly ts: string;
  readonly text?: string;
  readonly extra?: Record<string, unknown>;
}) =>
  line({
    type: "assistant",
    timestamp: o.ts,
    requestId: o.requestId,
    message: {
      model: o.model,
      usage: o.usage,
      content: [{ type: "text", text: o.text ?? "ok" }],
    },
    ...o.extra,
  });

const LINES = [
  line({
    type: "user",
    timestamp: "2026-01-01T00:00:00Z",
    message: { content: "hi" },
  }),
  // API error turn: zero usage, fake model id.
  assistant({
    requestId: "r0",
    model: "<synthetic>",
    ts: "2026-01-01T00:00:01Z",
    text: "API Error: synthetic",
    usage: {
      input_tokens: 0,
      cache_read_input_tokens: 0,
      cache_creation_input_tokens: 0,
      output_tokens: 0,
    },
    extra: { isApiErrorMessage: true },
  }),
  assistant({
    requestId: "r1",
    model: "claude-opus-9",
    ts: "2026-01-01T00:00:02Z",
    usage: {
      input_tokens: 10,
      cache_read_input_tokens: 0,
      cache_creation_input_tokens: 3000,
      cache_creation: {
        ephemeral_1h_input_tokens: 2000,
        ephemeral_5m_input_tokens: 1000,
      },
      output_tokens: 50,
    },
  }),
  line({
    type: "system",
    subtype: "compact_boundary",
    uuid: "b1",
    timestamp: "2026-01-01T00:00:03Z",
    content: "Conversation compacted",
    compactMetadata: {
      trigger: "manual",
      preTokens: 3010,
      postTokens: 400,
      durationMs: 1000,
    },
  }),
  line({
    type: "user",
    parentUuid: "b1",
    isCompactSummary: true,
    timestamp: "2026-01-01T00:00:04Z",
    message: { content: "Summary of the earlier conversation." },
  }),
  // Known record types the parser ignores on purpose.
  line({ type: "api-request", timestamp: "2026-01-01T00:00:05Z" }),
  line({ type: "last-prompt" }),
  line({ type: "queue-operation", operation: "enqueue" }),
  line({ type: "file-history-snapshot" }),
  line({ type: "permission-mode" }),
  line({
    type: "cost-state",
    modelUsage: { "claude-opus-9[1m]": { inputTokens: 10, outputTokens: 50 } },
  }),
  // Truly new formats.
  line({ type: "brand-new-record" }),
  line({ type: "brand-new-record" }),
  line({ type: "system", subtype: "brand_new_subtype", content: "x" }),
];
const TEXT = `${LINES.join("\n")}\n`;

const parseText = (text: string) =>
  parseClaudeSession({ text, path: "/fixture/s.jsonl", sessionId: "s1" });
const parsed = () => parseText(TEXT);
const header = (text: string) =>
  buildHeader({
    text,
    id: "s1",
    slug: "p",
    path: "/fixture/s.jsonl",
    sizeBytes: text.length,
    mtimeMs: 0,
  });
const joined = (lines: readonly string[]) => `${lines.join("\n")}\n`;

describe("Claude record types (#111)", () => {
  test("1h vs 5m cache-write split lands on the turn", () => {
    const turn = parsed().turns.find((t) => t.requestId === "r1");
    expect(turn?.cacheCreationTokens).toBe(3000);
    expect(turn).toMatchObject({
      cacheCreation1hTokens: 2000,
      cacheCreation5mTokens: 1000,
    });
  });

  test("compact_boundary preTokens/postTokens land on the compaction event", () => {
    const compaction = parsed().events.find((e) => e.kind === "compaction");
    expect(compaction).toMatchObject({
      compactPreTokens: 3010,
      compactPostTokens: 400,
    });
  });

  test("<synthetic> is not listed as a model", () => {
    const p = parsed();
    expect(p.models).toEqual(["claude-opus-9"]);
    expect(p.turns.map((t) => t.model)).not.toContain("<synthetic>");
  });

  test("<synthetic> is not the session header model", () => {
    expect(header(TEXT).model).toBe("claude-opus-9");
  });

  test("only truly new record types and system subtypes count as unknown", () => {
    expect(parsed()).toMatchObject({
      unknownRecordTypes: { "brand-new-record": 2 },
      unknownSystemSubtypes: { brand_new_subtype: 1 },
    });
  });

  test("a [1m] modelUsage key sets a 1M native window", () => {
    const p = parsed();
    expect(p.nativeContextWindow).toBe(1_000_000);
    expect(analyze(p).contextWindowInferred).toBe(false);
  });

  test("a session with only <synthetic> has no model", () => {
    const text = joined([LINES[0] ?? "", LINES[1] ?? ""]);
    expect(parseText(text).models).toEqual([]);
    expect(header(text).model).toBeUndefined();
  });

  test("a compaction with no boundary carries no pre/post tokens", () => {
    const p = parseText(joined([LINES[4] ?? ""]));
    const compaction = p.events.find((e) => e.kind === "compaction");
    expect(compaction).toBeDefined();
    expect(compaction?.compactPreTokens).toBeUndefined();
    expect(compaction?.compactPostTokens).toBeUndefined();
  });

  test("a boundary is used by one summary only", () => {
    const p = parseText(
      joined([LINES[3] ?? "", LINES[4] ?? "", LINES[4] ?? ""])
    );
    const compactions = p.events.filter((e) => e.kind === "compaction");
    expect(compactions.map((e) => e.compactPreTokens)).toEqual([
      3010,
      undefined,
    ]);
  });

  test("cost-state without a [1m] key leaves the window inferred", () => {
    const p = parseText(
      joined([
        LINES[2] ?? "",
        line({
          type: "cost-state",
          modelUsage: { "claude-opus-9": { inputTokens: 10 } },
        }),
      ])
    );
    expect(p.nativeContextWindow).toBeUndefined();
    expect(analyze(p).contextWindowInferred).toBe(true);
  });

  test("a streamed duplicate line keeps the first-seen cache split", () => {
    const dup = assistant({
      requestId: "r1",
      model: "claude-opus-9",
      ts: "2026-01-01T00:00:02Z",
      usage: {
        input_tokens: 10,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 3000,
        cache_creation: {
          ephemeral_1h_input_tokens: 0,
          ephemeral_5m_input_tokens: 3000,
        },
        output_tokens: 80,
      },
    });
    const turn = parseText(joined([LINES[2] ?? "", dup])).turns[0];
    expect(turn).toMatchObject({
      cacheCreation1hTokens: 2000,
      cacheCreation5mTokens: 1000,
      outputTokens: 80,
    });
  });

  test("a reported 0 split is kept; a missing split stays absent", () => {
    const usage = {
      input_tokens: 10,
      cache_creation_input_tokens: 5,
      output_tokens: 1,
    };
    const p = parseText(
      joined([
        assistant({ requestId: "a", model: "m", ts: "t", usage }),
        assistant({
          requestId: "b",
          model: "m",
          ts: "t",
          usage: {
            ...usage,
            cache_creation: {
              ephemeral_1h_input_tokens: 0,
              ephemeral_5m_input_tokens: 5,
            },
          },
        }),
      ])
    );
    expect(p.turns[0]?.cacheCreation1hTokens).toBeUndefined();
    expect(p.turns[1]?.cacheCreation1hTokens).toBe(0);
  });

  test("lines with no type and known subtypes are not counted", () => {
    const p = parseText(
      joined([
        line({ foo: 1 }),
        line({ type: 7 }),
        line({ type: "system", subtype: "turn_duration" }),
        line({ type: "custom-title" }),
      ])
    );
    expect(p.unknownRecordTypes).toBeUndefined();
    expect(p.unknownSystemSubtypes).toBeUndefined();
  });

  test("new fields survive a ParsedSession schema round trip", () => {
    const decoded = Schema.decodeUnknownSync(ParsedSession)(parsed());
    expect(decoded).toMatchObject({
      unknownRecordTypes: { "brand-new-record": 2 },
      nativeContextWindow: 1_000_000,
    });
    expect(decoded.turns[0]?.cacheCreation1hTokens).toBe(2000);
    expect(
      decoded.events.find((e) => e.kind === "compaction")?.compactPreTokens
    ).toBe(3010);
  });
});
