/** Claude `attachment` lines: which budget category each type loads, which
 * types never reach the model (metadata only), and queued prompts the user
 * typed while the agent was busy. Synthetic lines only.
 */
import { describe, expect, test } from "bun:test";
import { analyze } from "../../src/services/sessions/analyze";
import { attachmentEvent } from "../../src/services/sessions/claude-attachments";
import { parseClaudeSession } from "../../src/services/sessions/parse";
import { estTokens } from "../../src/services/sessions/tokens";

const base = { index: 0, isSidechain: false, ts: undefined };

const rendered = (text: string) => [{ content: text }];

const line = (attachment: Record<string, unknown>, text?: string) => ({
  type: "attachment",
  attachment,
  ...(text === undefined ? {} : { rendered: rendered(text) }),
});

const ev = (attachment: Record<string, unknown>, text?: string) =>
  attachmentEvent({ o: line(attachment, text), base, seen: {} });

const ts = "2026-01-01T00:00:00.000Z";

const parse = (lines: readonly Record<string, unknown>[]) =>
  parseClaudeSession({
    path: "synthetic.jsonl",
    text: lines
      .map((o) => JSON.stringify({ timestamp: ts, sessionId: "s", ...o }))
      .join("\n"),
    sessionId: "s",
  });

describe("metadata-only attachments", () => {
  // Claude Code maps these to no API messages, so they never carry `rendered`.
  const metadataOnly = [
    { type: "structured_output", data: { answer: 42 }, toolUseID: "toolu_1" },
    { type: "deferred_tools_record", entries: [{ name: "ToolA" }] },
    { type: "credential_org", organizationUuid: "org-0000" },
    { type: "command_permissions", allowedTools: ["Bash"] },
    { type: "max_turns_reached", maxTurns: 5, turnCount: 5 },
    { type: "thinking_drop", reason: "org" },
    { type: "already_read_file", filename: "src/a.ts" },
    { type: "hook_non_blocking_error", stderr: "boom" },
    { type: "goals", items: ["g"] },
  ];

  for (const a of metadataOnly) {
    test(`${a.type} costs 0 tokens and loads no category`, () => {
      const e = ev(a);
      expect(e?.kind).toBe("attachment");
      expect(e?.tokensEst).toBe(0);
      expect(e?.loadedCategory).toBeUndefined();
      expect(e?.attachmentType).toBe(a.type);
      expect(e?.body.length).toBeGreaterThan(0);
    });
  }

  test("a metadata type with rendered text counts that text", () => {
    const text = "<system-reminder>goal: ship it</system-reminder>";
    const e = ev({ type: "goals", items: ["ship it"] }, text);
    expect(e?.tokensEst).toBe(estTokens(text));
    expect(e?.loadedCategory).toBeDefined();
  });

  const hook = (extra: Record<string, unknown>) => ({
    type: "hook_success",
    command: "echo done",
    stdout: "x".repeat(400),
    stderr: "",
    content: "",
    ...extra,
  });

  test("a Stop hook_success is not sent to the model", () => {
    const e = ev(hook({ hookEvent: "Stop", hookName: "Stop" }));
    expect(e?.tokensEst).toBe(0);
    expect(e?.loadedCategory).toBeUndefined();
  });

  test("a SessionStart hook_success is context", () => {
    const e = ev(
      hook({ hookEvent: "SessionStart" }),
      "<system-reminder>SessionStart hook output</system-reminder>"
    );
    expect(e?.tokensEst).toBeGreaterThan(0);
    expect(e?.loadedCategory).toBe("reminder");
  });

  test("a hook_success without hookEvent still counts", () => {
    const e = ev(hook({}));
    expect(e?.tokensEst).toBeGreaterThan(0);
    expect(e?.loadedCategory).toBe("reminder");
  });

  test("a metadata event takes no share of the measured delta", () => {
    const usage = (cacheRead: number, output: number) => ({
      input_tokens: 0,
      cache_read_input_tokens: cacheRead,
      cache_creation_input_tokens: 0,
      output_tokens: output,
    });
    const p = parse([
      { type: "user", message: { role: "user", content: "run it" } },
      {
        type: "assistant",
        requestId: "req_1",
        message: {
          model: "claude-opus-4-5",
          usage: usage(1000, 10),
          content: [
            { type: "tool_use", id: "toolu_1", name: "Bash", input: {} },
          ],
        },
      },
      {
        type: "user",
        message: {
          role: "user",
          content: [
            { type: "tool_result", tool_use_id: "toolu_1", content: "ok" },
          ],
        },
      },
      line({ type: "deferred_tools_record", entries: [{ name: "ToolA" }] }),
      {
        type: "assistant",
        requestId: "req_2",
        message: {
          model: "claude-opus-4-5",
          usage: usage(1100, 10),
          content: [{ type: "text", text: "done" }],
        },
      },
    ]);
    const meta = p.events.find(
      (e) => e.attachmentType === "deferred_tools_record"
    );
    const result = p.events.find((e) => e.kind === "tool-result");
    expect(meta?.tokensMeasured).toBeUndefined();
    // 1100 - 1000 - 10, all to the tool result
    expect(result?.tokensMeasured).toBe(90);
  });
});

describe("attachment categories", () => {
  const cases: [Record<string, unknown>, string][] = [
    [{ type: "total_tokens_reminder" }, "reminder"],
    [{ type: "silent_turn_reminder" }, "reminder"],
    [{ type: "output_style", style: "concise" }, "reminder"],
    [{ type: "output_style_instructions" }, "reminder"],
    [{ type: "environment", snapshot: {} }, "reminder"],
    [{ type: "date" }, "reminder"],
    [{ type: "model", identity: {} }, "reminder"],
    [{ type: "session_context", context: {} }, "reminder"],
    [{ type: "auto_mode" }, "reminder"],
    [{ type: "auto_mode_exit" }, "reminder"],
    [{ type: "remote_session_change" }, "reminder"],
    [{ type: "read_truncation_notice" }, "reminder"],
    [{ type: "plan_mode" }, "reminder"],
    [{ type: "plan_mode_exit", planExists: false }, "reminder"],
    [{ type: "plan_mode_reentry" }, "reminder"],
    [{ type: "todo_reminder" }, "reminder"],
    [{ type: "queued_command", commandMode: "task-notification" }, "reminder"],
    [{ type: "compact_file_reference", filename: "src/a.ts" }, "file"],
    [{ type: "skill_listing", content: "s" }, "skills"],
    [{ type: "nested_memory", content: {} }, "claude-md"],
    [{ type: "file", filename: "a/CLAUDE.md" }, "claude-md"],
    [{ type: "edited_text_file", filename: "a.ts" }, "ide"],
    [{ type: "task_reminder" }, "reminder"],
    [{ type: "something_new" }, "other"],
  ];
  for (const [a, category] of cases) {
    test(`${a.type} loads ${category}`, () => {
      const e = ev(a, `<system-reminder>${a.type}</system-reminder>`);
      expect(e?.loadedCategory).toBe(category);
      expect(e?.tokensEst).toBeGreaterThan(0);
    });
  }
});

describe("queued prompts", () => {
  const queuedText = "also do the second thing";
  const shown = `<system-reminder>\nThe user sent a new message while you were working:\n${queuedText}\n</system-reminder>`;
  const user = { type: "user", message: { role: "user", content: "first" } };
  const queued = (
    attachment: Record<string, unknown>,
    text?: string
  ): Record<string, unknown> => ({
    ...line(
      { type: "queued_command", commandMode: "prompt", ...attachment },
      text
    ),
    renderedRole: "system",
  });

  test("a queued prompt is a user message; a task notification is not", () => {
    const p = parse([
      user,
      queued({ prompt: queuedText }, shown),
      {
        ...line(
          {
            type: "queued_command",
            commandMode: "task-notification",
            prompt: "<task-notification>done</task-notification>",
          },
          "<system-reminder>notification</system-reminder>"
        ),
        renderedRole: "system",
      },
    ]);
    expect(analyze(p).userMessageCount).toBe(2);
    const [, prompt, note] = p.events;
    expect(prompt?.kind).toBe("user-prompt");
    expect(prompt?.promptOrigin).toBe("human");
    expect(prompt?.attachmentType).toBe("queued_command");
    expect(prompt?.body).toBe(queuedText);
    expect(prompt?.tokensEst).toBe(estTokens(shown));
    expect(note?.kind).toBe("attachment");
  });

  test("a prompt rendered by its batch head costs 0 and still counts", () => {
    const p = parse([
      user,
      queued({ prompt: queuedText, renderedByBatchHead: true }),
    ]);
    expect(p.events[1]?.tokensEst).toBe(0);
    expect(analyze(p).userMessageCount).toBe(2);
  });

  test("an isMeta queued prompt is not the user's", () => {
    const p = parse([
      user,
      queued({ prompt: queuedText, isMeta: true }, shown),
    ]);
    expect(p.events[1]?.promptOrigin).toBe("harness");
    expect(analyze(p).userMessageCount).toBe(1);
  });

  test("a queued prompt keeps its promptSource", () => {
    const e = attachmentEvent({
      o: queued({ prompt: queuedText, promptSource: "sdk" }, shown),
      base,
      seen: {},
    });
    expect(e?.promptOrigin).toBe("sdk");
    expect(e?.promptSource).toBe("sdk");
  });

  const image = {
    type: "image",
    source: {
      type: "base64",
      media_type: "image/png",
      data: "QUJD".repeat(500),
    },
  };

  test("an array prompt joins its text blocks, images as markers", () => {
    const e = attachmentEvent({
      o: queued({
        prompt: [
          { type: "text", text: "part one" },
          image,
          { type: "text", text: "part two" },
        ],
      }),
      base,
      seen: {},
    });
    const body = "part one\n[image: image/png]\npart two";
    expect(e?.kind).toBe("user-prompt");
    expect(e?.body).toBe(body);
    expect(e?.tokensEst).toBe(estTokens(body));
  });

  test("an image-only prompt is a user message without base64 data", () => {
    const e = attachmentEvent({
      o: queued({ prompt: [image] }),
      base,
      seen: {},
    });
    expect(e?.kind).toBe("user-prompt");
    expect(e?.body).toBe("[image: image/png]");
    expect(e?.body).not.toContain("QUJD");
    expect(e?.tokensEst).toBe(estTokens("[image: image/png]"));
    expect(e?.loadedCategory).toBeUndefined();
  });
});
