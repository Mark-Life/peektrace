/** Defensive-parsing guarantees (Phase 10.2).
 *
 * Undocumented agent formats drift, files get truncated mid-write, and users
 * hand-edit memory frontmatter. The parsers must skip the bad bits and keep the
 * good ones — never throw. These tests feed deliberately broken fixture lines
 * through every parse entry point and assert it degrades instead of crashing.
 */
import { describe, expect, test } from "bun:test";
import {
  composeFile,
  parseFrontmatter,
} from "../../src/services/memory/frontmatter";
import { buildHeader } from "../../src/services/sessions/header";
import {
  parseClaudeSession,
  parseJsonl,
} from "../../src/services/sessions/parse";

const GOOD_LINE = JSON.stringify({
  type: "assistant",
  requestId: "req-1",
  timestamp: "2026-06-01T10:00:05.000Z",
  message: {
    model: "claude-opus-4",
    usage: { input_tokens: 100, cache_read_input_tokens: 0, output_tokens: 50 },
    content: [{ type: "text", text: "hello" }],
  },
});

/** A transcript that mixes valid lines with several kinds of broken ones. */
const BROKEN_TRANSCRIPT = [
  '{"type":"user","message":{"content":"first"}}',
  "this is not json at all",
  '{"type":"assistant","message":{', // truncated mid-object
  "", // blank line
  "   ", // whitespace-only line
  '{"type":"user"', // unterminated
  GOOD_LINE,
  '{"type":"unknown-future-kind","weird":true}', // undocumented type
  '{"type":"assistant","message":{"content":[{"type":"text"', // truncated array
].join("\n");

describe("defensive JSONL parsing", () => {
  test("parseJsonl skips malformed lines and keeps valid ones", () => {
    const rows = parseJsonl(BROKEN_TRANSCRIPT);
    // 3 well-formed objects survive: first user, GOOD_LINE, unknown-future-kind.
    expect(rows).toHaveLength(3);
    expect(rows.every((r) => typeof r === "object")).toBe(true);
  });

  test("parseClaudeSession never throws on a broken transcript", () => {
    const parsed = parseClaudeSession({
      text: BROKEN_TRANSCRIPT,
      path: "/tmp/broken.jsonl",
      sessionId: "broken",
    });
    // The one valid assistant turn is still recovered.
    expect(parsed.turns).toHaveLength(1);
    expect(parsed.models).toContain("claude-opus-4");
    // The good user + assistant text events survive; garbage produced none.
    expect(parsed.events.some((e) => e.kind === "user-prompt")).toBe(true);
    expect(parsed.events.some((e) => e.kind === "assistant-text")).toBe(true);
  });

  test("buildHeader tolerates a fully garbage transcript", () => {
    const header = buildHeader({
      text: "garbage\n{broken\n\n!!!",
      id: "h",
      slug: "-tmp-x",
      path: "/tmp/x.jsonl",
      sizeBytes: 20,
      mtimeMs: 0,
    });
    expect(header.id).toBe("h");
    // No parseable model/title, but the header is still well-formed.
    expect(header.model).toBeUndefined();
    expect(header.messageCount).toBe(3);
  });
});

describe("defensive frontmatter parsing", () => {
  test("unterminated frontmatter fence falls back to whole-text body", () => {
    const text = "---\nname: thing\ndescription: no closing fence\nstill body";
    const { frontmatter, body } = parseFrontmatter(text);
    expect(frontmatter.shape).toBe("none");
    expect(body).toBe(text);
  });

  test("garbage frontmatter lines are tolerated, not thrown", () => {
    const text =
      "---\n: : :\nname value with no colon\n\ttab-indented junk\ntype: project\n---\nbody";
    const { frontmatter, body } = parseFrontmatter(text);
    expect(frontmatter.type).toBe("project");
    expect(body).toBe("body");
  });

  test("round-trip of malformed-but-fenced frontmatter is byte-stable", () => {
    const text = "---\nname value with no colon\ntype: project\n---\nbody";
    const { frontmatter, body } = parseFrontmatter(text);
    expect(composeFile({ frontmatter, body })).toBe(text);
  });
});
