/** Thinking sized from usage, not guessed.
 *
 * Claude Code often keeps only a signature for a thinking block, so its text
 * says nothing about its size. Usage reports the thinking share of each call
 * (`output_tokens_details.thinking_tokens`); the block and the budget use it.
 */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { analyze } from "../../src/services/sessions/analyze";
import { parseClaudeSession } from "../../src/services/sessions/parse";

const path = join(
  import.meta.dir,
  "../fixtures/sessions-claude/thinking-tokens.jsonl"
);
const parsed = () =>
  parseClaudeSession({
    text: readFileSync(path, "utf8"),
    path,
    sessionId: "s1",
  });

describe("Claude thinking tokens", () => {
  test("a call's reported thinking tokens land on its thinking block", () => {
    const p = parsed();
    const thinking = p.events.find((e) => e.kind === "assistant-thinking");
    expect(thinking?.body).toBe("");
    expect(thinking?.tokensMeasured).toBe(400);
    expect(thinking?.thinkingDurationMs).toBe(4200);
    expect(p.turns.map((t) => t.thinkingTokens)).toEqual([400, undefined]);
    expect(p.turns.map((t) => t.effort)).toEqual(["high", undefined]);
  });

  test("the thinking slice is the reported count, the rest of output is visible", () => {
    const a = analyze(parsed());
    const last = a.snapshots.at(-1);
    expect(last?.slices.thinking).toBe(400);
    expect(last?.slices.assistant_text).toBe(100);
  });
});
