/** Claude Code writes one line per content block of a call, and later lines
 * can carry larger `output_tokens` (seen in subagent transcripts). A turn's
 * output and thinking must reflect the final, largest count. */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { analyze } from "../../src/services/sessions/analyze";
import { parseClaudeSession } from "../../src/services/sessions/parse";

const path = join(
  import.meta.dir,
  "../fixtures/sessions-claude/streamed-output.jsonl"
);
const parsed = () =>
  parseClaudeSession({
    text: readFileSync(path, "utf8"),
    path,
    sessionId: "s1",
    includeSidechainTurns: true,
  });

describe("Claude streamed output usage", () => {
  test("a turn keeps the last line's output and thinking tokens", () => {
    const p = parsed();
    expect(p.turns.map((t) => t.outputTokens)).toEqual([300, 15]);
    expect(p.turns.map((t) => t.thinkingTokens)).toEqual([200, undefined]);
  });

  test("a server tool count reported on a later line is kept", () => {
    const p = parsed();
    expect(p.turns.map((t) => t.webSearchRequests)).toEqual([1, undefined]);
    expect(p.turns.map((t) => t.webFetchRequests)).toEqual([
      undefined,
      undefined,
    ]);
  });

  test("input and cache usage are unchanged across lines", () => {
    const p = parsed();
    expect(p.turns.map((t) => t.contextTokens)).toEqual([4010, 4340]);
    expect(p.turns.map((t) => t.cacheReadTokens)).toEqual([0, 4010]);
  });

  test("the thinking block is sized from the final count", () => {
    const thinking = parsed().events.find(
      (e) => e.kind === "assistant-thinking"
    );
    expect(thinking?.tokensMeasured).toBe(200);
  });

  test("the budget splits the final output into thinking and visible text", () => {
    const next = analyze(parsed()).snapshots[1];
    expect(next?.slices.thinking).toBe(200);
  });
});
