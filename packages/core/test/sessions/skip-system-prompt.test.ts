/** `skipSystemPrompt` drops only the system-prompt events. Every string is made up. */
import { describe, expect, test } from "bun:test";
import { parseClaudeSession } from "../../src/services/sessions/parse";
import { claudeParser } from "../../src/services/sessions/parsers/claude";
import type { TimelineEvent } from "../../src/services/sessions/schema";

const tool = (name: string) => ({
  name,
  description: `Runs ${name}.`,
  schema: {
    name,
    input_schema: {
      type: "object",
      properties: { arg: { type: "string", description: "An argument" } },
      required: ["arg"],
    },
  },
});

const snapshot = (at: string, tools: readonly unknown[]) =>
  JSON.stringify({
    type: "attachment",
    timestamp: at,
    cwd: "/work/proj",
    attachment: {
      type: "prompt_snapshot",
      systemPrompt: ["You are a test agent.", "# Env\n - cwd: /work/proj"],
      tools,
    },
  });

const TEXT = `${[
  snapshot("2026-08-01T10:00:00.000Z", [tool("Alpha")]),
  JSON.stringify({
    type: "user",
    cwd: "/work/proj",
    timestamp: "2026-08-01T10:00:01.000Z",
    message: { role: "user", content: "do the task" },
  }),
  JSON.stringify({
    type: "assistant",
    requestId: "req_1",
    timestamp: "2026-08-01T10:00:02.000Z",
    message: {
      model: "claude-test",
      usage: {
        input_tokens: 5,
        cache_read_input_tokens: 900,
        cache_creation_input_tokens: 100,
        output_tokens: 10,
      },
      content: [
        { type: "text", text: "on it" },
        { type: "tool_use", id: "toolu_1", name: "Alpha", input: { arg: "a" } },
      ],
    },
  }),
  JSON.stringify({
    type: "user",
    timestamp: "2026-08-01T10:00:03.000Z",
    message: {
      role: "user",
      content: [
        { type: "tool_result", tool_use_id: "toolu_1", content: "done" },
      ],
    },
  }),
  snapshot("2026-08-01T10:00:04.000Z", [tool("Alpha"), tool("Beta")]),
  JSON.stringify({
    type: "user",
    timestamp: "2026-08-01T10:00:05.000Z",
    message: { role: "user", content: "and again" },
  }),
].join("\n")}\n`;

const parse = (skipSystemPrompt?: boolean) =>
  parseClaudeSession({
    text: TEXT,
    path: "/fixture/proj/sess-1.jsonl",
    sessionId: "sess-1",
    ...(skipSystemPrompt === undefined ? {} : { skipSystemPrompt }),
  });

/** An event without the size that depends on its neighbours. */
const shape = ({ tokensMeasured: _, ...rest }: TimelineEvent) => rest;

describe("skipSystemPrompt", () => {
  test("by default both snapshots render, tools included", () => {
    const prompts = parse().events.filter((e) => e.kind === "system-prompt");
    expect(prompts).toHaveLength(2);
    expect(prompts[0]?.body).toContain("Alpha");
    expect(prompts[1]?.body).toContain("Beta");
    expect(parse(false)).toEqual(parse());
  });

  test("with the flag no system-prompt event is built", () => {
    const parsed = parse(true);
    expect(parsed.events.some((e) => e.kind === "system-prompt")).toBe(false);
    expect(parsed.events.length).toBeGreaterThan(0);
  });

  test("every other part of the session is unchanged", () => {
    const full = parse();
    const lean = parse(true);
    expect(lean.events.map(shape)).toEqual(
      full.events.filter((e) => e.kind !== "system-prompt").map(shape)
    );
    // turns point into the events array, so compare the lines they point at
    const turnsOf = (p: typeof full) =>
      p.turns.map(({ eventIndexes, ...turn }) => ({
        ...turn,
        lines: eventIndexes.map((i) => p.events[i]?.index),
      }));
    expect(turnsOf(lean)).toEqual(turnsOf(full));
    expect(lean.cwd).toBe(full.cwd);
    expect(lean.models).toEqual(full.models);
  });

  test("the Claude parser passes the flag through", () => {
    const parsed = claudeParser.parseSession({
      text: TEXT,
      path: "/fixture/proj/sess-1.jsonl",
      sessionId: "sess-1",
      slug: "proj",
      skipSystemPrompt: true,
    });
    expect(parsed.events.some((e) => e.kind === "system-prompt")).toBe(false);
  });
});
