/** Who wrote each `user` line.
 *
 * Claude Code logs task notifications, wake-ups and injected skill bodies as
 * `user` lines. Read as the person talking, they inflate the message count and
 * the prompts slice of the budget, and the chat puts them on the user's side.
 */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { analyze } from "../../src/services/sessions/analyze";
import { eventBadgeLabel } from "../../src/services/sessions/labels";
import { parseClaudeSession } from "../../src/services/sessions/parse";
import { isUserAuthored } from "../../src/services/sessions/prompt-origin";

const path = join(
  import.meta.dir,
  "../fixtures/sessions-claude/prompt-origin.jsonl"
);
const parsed = () =>
  parseClaudeSession({
    text: readFileSync(path, "utf8"),
    path,
    sessionId: "s1",
  });

describe("Claude prompt origin", () => {
  test("each user line is classified by what wrote it", () => {
    const events = parsed().events;
    expect(events.map((e) => [e.promptOrigin, e.promptSource])).toEqual([
      ["human", "typed"],
      ["sdk", "sdk"],
      ["task-notification", "system"],
      ["scheduled", "system"],
      ["harness", undefined],
      ["command", undefined],
      ["task-notification", undefined],
      ["harness", undefined],
      ["harness", undefined],
    ]);
  });

  test("harness prompts are titled after their source, with a summary preview", () => {
    const [, , note, wake, meta, command, bare, output] = parsed().events;
    expect(note?.title).toBe("Task notification (completed)");
    expect(note?.preview).toBe('Background command "bun test" completed');
    expect(wake?.title).toBe("Scheduled prompt");
    expect(meta?.title).toBe("Harness message");
    expect(command?.title).toBe("Command /compact");
    expect(command?.preview).toBe("/compact keep the plan");
    expect(bare?.title).toBe("Task notification (failed)");
    expect(output?.title).toBe("Command output");
  });

  test("a command counts once: its output and expanded skill body are the harness's", () => {
    const origins = parsed().events.map((e) => e.promptOrigin);
    expect(origins.filter((o) => o === "command")).toHaveLength(1);
  });

  test("only prompts a person sent count as user messages", () => {
    const p = parsed();
    expect(p.events.filter(isUserAuthored)).toHaveLength(3);
    expect(analyze(p).userMessageCount).toBe(3);
  });

  test("non-user prompts badge as their origin", () => {
    expect(parsed().events.map(eventBadgeLabel)).toEqual([
      "user-prompt",
      "user-prompt",
      "task notification",
      "scheduled",
      "harness",
      "command",
      "task notification",
      "harness",
      "harness",
    ]);
  });

  test("an event without an origin still counts as the user's", () => {
    expect(
      isUserAuthored({ kind: "user-prompt", promptOrigin: undefined })
    ).toBe(true);
    expect(isUserAuthored({ kind: "assistant-text" })).toBe(false);
  });
});
