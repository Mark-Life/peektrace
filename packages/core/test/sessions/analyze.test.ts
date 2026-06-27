import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { analyze } from "../../src/services/sessions/analyze";
import { parseClaudeSession } from "../../src/services/sessions/parse";
import { redactSession, redactText } from "../../src/services/sessions/redact";

const FIXTURE_DIR = join(import.meta.dir, "../fixtures/sessions");
const SESSION_ID = "11111111-1111-4111-8111-111111111111";
const transcriptPath = join(
  FIXTURE_DIR,
  "projects",
  "-Users-demo-proj",
  `${SESSION_ID}.jsonl`
);

const golden = JSON.parse(
  readFileSync(join(FIXTURE_DIR, "golden.json"), "utf8")
) as {
  peakContextTokens: number;
  peakTurnIndex: number;
  systemOverheadTokens: number;
  finalContextTokens: number;
  totalOutputTokens: number;
  turnCount: number;
  models: string[];
  budget: Array<{ key: string; tokens: number }>;
  snapshots: Array<{ turnIndex: number; ctx: number }>;
};

const parsed = () =>
  parseClaudeSession({
    text: readFileSync(transcriptPath, "utf8"),
    path: transcriptPath,
    sessionId: SESSION_ID,
  });

describe("analyze (golden parity with session-report)", () => {
  test("peak context excludes sidechain turns", () => {
    const a = analyze(parsed());
    expect(a.peakContextTokens).toBe(golden.peakContextTokens);
    expect(a.peakContextTokens).toBe(1150);
    expect(a.peakTurnIndex).toBe(golden.peakTurnIndex);
    expect(a.turnCount).toBe(golden.turnCount);
    expect(a.turnCount).toBe(3);
  });

  test("system overhead and totals match golden", () => {
    const a = analyze(parsed());
    expect(a.systemOverheadTokens).toBe(golden.systemOverheadTokens);
    expect(a.finalContextTokens).toBe(golden.finalContextTokens);
    expect(a.totalOutputTokens).toBe(golden.totalOutputTokens);
    expect(a.totalOutputTokens).toBe(1400);
  });

  test("budget partition at peak matches golden exactly", () => {
    const a = analyze(parsed());
    const budget = a.budget.map((b) => ({ key: b.key, tokens: b.tokens }));
    expect(budget).toEqual(golden.budget);
  });

  test("thinking band is recovered from output_tokens", () => {
    const a = analyze(parsed());
    const thinking = a.budget.find((b) => b.key === "thinking");
    const goldenThinking = golden.budget.find((b) => b.key === "thinking");
    expect(thinking?.tokens).toBe(goldenThinking?.tokens);
    expect(thinking?.tokens).toBeGreaterThan(0);
  });

  test("per-turn snapshots track real context size", () => {
    const a = analyze(parsed());
    expect(a.snapshots.map((s) => s.ctx)).toEqual(
      golden.snapshots.map((s) => s.ctx)
    );
  });

  test("default window is 1M and inferred", () => {
    const a = analyze(parsed());
    expect(a.contextWindow).toBe(1_000_000);
    expect(a.contextWindowInferred).toBe(true);
    const overridden = analyze(parsed(), { window: 200_000 });
    expect(overridden.contextWindow).toBe(200_000);
    expect(overridden.contextWindowInferred).toBe(false);
  });
});

describe("redaction", () => {
  test("redactText masks a planted anthropic key", () => {
    const secret =
      'API_KEY = "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234"';
    const out = redactText(secret);
    expect(out).toContain("[REDACTED:anthropic-key]");
    expect(out).not.toContain("sk-ant-api03-AAAA");
  });

  test("redactSession masks the secret in the tool-result body", () => {
    const a = redactSession(analyze(parsed()));
    const toolResult = a.events.find((e) => e.kind === "tool-result");
    expect(toolResult?.body).toContain("[REDACTED:anthropic-key]");
    expect(toolResult?.body).not.toContain("sk-ant-api03");
  });
});
