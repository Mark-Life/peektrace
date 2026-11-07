/** Stats parses with `skipSystemPrompt`; the facts must not notice. */
import { describe, expect, test } from "bun:test";
import { parseClaudeSession } from "../../src/services/sessions/parse";
import { extractFacts } from "../../src/services/stats/extract";
import { makeRedactor } from "../../src/services/stats/redact";
import { refOf, transcriptOf } from "./fixture-session";

const snapshot = (tools: readonly string[]) =>
  JSON.stringify({
    type: "attachment",
    timestamp: "2026-08-01T09:59:59.000Z",
    cwd: "/work/proj",
    attachment: {
      type: "prompt_snapshot",
      systemPrompt: ["You are a test agent."],
      tools: tools.map((name) => ({ name, description: `Runs ${name}.` })),
    },
  });

/** A transcript with a snapshot first and another one mid-session. */
const TEXT = (() => {
  const lines = transcriptOf([
    { name: "Bash", input: { command: "ls" }, output: "a\nb", isError: false },
    {
      name: "Bash",
      input: { command: "bun test" },
      output: " 1 pass\n 2 fail\n",
      isError: false,
    },
    { name: "Read", input: { file_path: "x.ts" }, output: "", isError: true },
  ])
    .trimEnd()
    .split("\n");
  return `${[
    snapshot(["Bash"]),
    ...lines.slice(0, 3),
    snapshot(["Bash", "Read"]),
    ...lines.slice(3),
  ].join("\n")}\n`;
})();

const factsWith = (skipSystemPrompt: boolean) =>
  extractFacts({
    ref: refOf(),
    parsed: parseClaudeSession({
      text: TEXT,
      path: refOf().path,
      sessionId: refOf().id,
      skipSystemPrompt,
    }),
    project: "proj",
    redact: makeRedactor({ enabled: true }),
  });

describe("stats facts with skipSystemPrompt", () => {
  test("are identical with and without the system-prompt events", () => {
    const lean = factsWith(true);
    expect(lean).toEqual(factsWith(false));
    expect(lean.map((f) => f.row)).toEqual(["session", "bash", "bash", "tool"]);
  });
});
