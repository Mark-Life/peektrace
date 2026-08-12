/** Tool-row shaping, driven by real transcripts through the real parser.
 *
 * These are the parts of the AI Elements tool rows that carry meaning rather
 * than styling: which tool a row belongs to, what the transcript says happened,
 * and which bytes end up on screen. The last one is why redaction gets a test
 * here too — the rows render tool arguments, and a shaping bug that reached
 * around `redactSession` would leak secrets behind a banner promising it didn't.
 *
 * The component itself is not rendered: `@workspace/ui` resolves a different
 * copy of React than `inspector` does, so hooks blow up outside Vite (which
 * dedupes via `@vitejs/plugin-react`). Typecheck and the Vite build cover the JSX.
 */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { analyze } from "@workspace/core/services/sessions/analyze";
import { parseClaudeSession } from "@workspace/core/services/sessions/parse";
import { redactSession } from "@workspace/core/services/sessions/redact";
import type {
  AnalyzedSession,
  TimelineEvent,
} from "@workspace/core/services/sessions/schema";
import {
  callView,
  indexTools,
  isToolEvent,
  resultPane,
  type ToolPane,
  toolNameOf,
  toolState,
} from "../src/lib/tool-event";

const FIXTURE = join(
  import.meta.dir,
  "../../../packages/core/test/fixtures/sessions/projects/-Users-demo-proj/11111111-1111-4111-8111-111111111111.jsonl"
);

const sessionFrom = (text: string, sessionId: string): AnalyzedSession =>
  analyze(
    parseClaudeSession({ text, path: `/tmp/${sessionId}.jsonl`, sessionId })
  );

/** The shared Claude fixture: one `Read` call, one result carrying a secret. */
const golden = () =>
  sessionFrom(
    readFileSync(FIXTURE, "utf8"),
    "11111111-1111-4111-8111-111111111111"
  );

/** A transcript built to hit the outcomes the golden fixture has no example of. */
const OUTCOMES = [
  {
    type: "assistant",
    requestId: "req-1",
    message: {
      model: "claude-opus-4",
      usage: {
        input_tokens: 10,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
        output_tokens: 10,
      },
      content: [
        {
          type: "tool_use",
          id: "call-ok",
          name: "Bash",
          input: {
            command: "bun test\necho done",
            description: "Run the suite",
            timeout: 120_000,
          },
        },
        {
          type: "tool_use",
          id: "call-bad",
          name: "Grep",
          input: { pattern: "TODO" },
        },
        {
          type: "tool_use",
          id: "call-open",
          name: "Write",
          input: { file_path: "/a.ts" },
        },
        {
          type: "tool_use",
          id: "call-edit",
          name: "Edit",
          input: {
            file_path: "/src/a.ts",
            old_string: "const a = 1;\nconst b = 2;",
            new_string: "const a = 1;\nconst b = 3;",
            replace_all: false,
          },
        },
        {
          type: "tool_use",
          id: "call-multi",
          name: "MultiEdit",
          input: {
            file_path: "/docs/b.md",
            edits: [
              { old_string: "# One", new_string: "# Uno" },
              { old_string: "# Two", new_string: "# Dos" },
            ],
          },
        },
      ],
    },
  },
  {
    type: "user",
    message: {
      content: [
        { type: "tool_result", tool_use_id: "call-ok", content: "ok" },
        {
          type: "tool_result",
          tool_use_id: "call-bad",
          is_error: true,
          content: "boom",
        },
      ],
    },
  },
]
  .map((o) => JSON.stringify(o))
  .join("\n");

const eventAt = (a: AnalyzedSession, useId: string, kind: string) =>
  a.events.find(
    (e) => e.toolUseId === useId && e.kind === kind
  ) as TimelineEvent;

describe("pairing calls with results", () => {
  test("a result borrows the tool name its own event never records", () => {
    const a = golden();
    const tools = indexTools(a);
    const result = eventAt(a, "tool-1", "tool-result");
    expect(result.toolName).toBeUndefined();
    expect(toolNameOf(result, tools)).toBe("Read");
  });

  test("an unpaired event still names something renderable", () => {
    const a = sessionFrom(OUTCOMES, "outcomes");
    const tools = indexTools(a);
    const orphan: TimelineEvent = {
      index: 0,
      kind: "tool-result",
      title: "tool_result",
      preview: "",
      body: "",
      tokensEst: 0,
      toolUseId: "nothing-matches-this",
    };
    expect(toolNameOf(orphan, tools)).toBe("tool");
  });

  test("outcome follows the result, not the call", () => {
    const a = sessionFrom(OUTCOMES, "outcomes");
    const tools = indexTools(a);
    expect(toolState(eventAt(a, "call-ok", "tool-call"), tools)).toBe(
      "completed"
    );
    expect(toolState(eventAt(a, "call-bad", "tool-call"), tools)).toBe("error");
    expect(toolState(eventAt(a, "call-bad", "tool-result"), tools)).toBe(
      "error"
    );
  });

  test("a call whose result never arrived reads as unanswered", () => {
    const a = sessionFrom(OUTCOMES, "outcomes");
    expect(toolState(eventAt(a, "call-open", "tool-call"), indexTools(a))).toBe(
      "unanswered"
    );
  });
});

describe("shaping a call's arguments", () => {
  const bashCall = () =>
    eventAt(sessionFrom(OUTCOMES, "outcomes"), "call-ok", "tool-call");

  /** The first pane's code, for the panes a test expects to be a code pane. */
  const codeAt = (panes: readonly ToolPane[], i: number) => {
    const pane = panes[i];
    return pane?.type === "code" ? pane.code : "";
  };

  test("unwraps the payload arg so its newlines survive", () => {
    const { panes } = callView(bashCall());
    expect(panes[0]).toEqual({
      code: "bun test\necho done",
      label: "command",
      language: "bash",
      type: "code",
    });
  });

  test("keeps every other argument in a pane of its own", () => {
    const { panes } = callView(bashCall());
    expect(panes).toHaveLength(2);
    expect(panes[1]?.label).toBe("Other parameters");
    expect(JSON.parse(codeAt(panes, 1))).toEqual({
      description: "Run the suite",
      timeout: 120_000,
    });
  });

  test("summarizes the call by its payload, not its JSON envelope", () => {
    expect(callView(bashCall()).summary).toBe("bun test echo done");
  });

  const editCall = () =>
    eventAt(sessionFrom(OUTCOMES, "outcomes"), "call-edit", "tool-call");

  test("an edit becomes a diff, not two blocks to compare by eye", () => {
    const { panes } = callView(editCall());
    expect(panes.map((p) => p.label)).toEqual(["Edit", "Other parameters"]);
    const [edit] = panes;
    expect(edit?.type).toBe("diff");
    expect(edit?.type === "diff" && edit.diff.rows.map((r) => r.kind)).toEqual([
      "ctx",
      "del",
      "add",
    ]);
  });

  test("the collapsed row says which file changed and by how much", () => {
    expect(callView(editCall()).summary).toBe("a.ts +1 −1");
  });

  test("an edited file names the language its payloads are written in", () => {
    expect(callView(editCall()).panes[0]?.language).toBe("typescript");
  });

  test("the args that were not part of the edit stay visible", () => {
    const { panes } = callView(editCall());
    expect(JSON.parse(codeAt(panes, 1))).toEqual({
      file_path: "/src/a.ts",
      replace_all: false,
    });
  });

  test("a batch of edits gets a numbered diff each", () => {
    const { panes, summary } = callView(
      eventAt(sessionFrom(OUTCOMES, "outcomes"), "call-multi", "tool-call")
    );
    expect(panes.map((p) => p.label)).toEqual([
      "Edit 1",
      "Edit 2",
      "Other parameters",
    ]);
    expect(summary).toBe("b.md +2 −2");
  });

  test("a call with no payload arg falls back to the whole argument object", () => {
    const a = sessionFrom(OUTCOMES, "outcomes");
    const { panes, summary } = callView(eventAt(a, "call-bad", "tool-call"));
    expect(panes).toHaveLength(1);
    expect(panes[0]?.label).toBe("Parameters");
    expect(JSON.parse(codeAt(panes, 0))).toEqual({ pattern: "TODO" });
    expect(summary).toContain("TODO");
  });
});

describe("shaping a result", () => {
  test("plain text stays verbatim rather than becoming JSON", () => {
    const a = golden();
    const pane = resultPane(eventAt(a, "tool-1", "tool-result"));
    expect(pane.language).toBe("text");
    expect(pane.code).toContain("export function login");
  });

  test("an empty body says so instead of rendering nothing", () => {
    const empty: TimelineEvent = {
      index: 0,
      kind: "tool-result",
      title: "tool_result",
      preview: "",
      body: "",
      tokensEst: 0,
    };
    expect(resultPane(empty).code).toBe("(no content)");
  });
});

describe("redaction reaches every pane", () => {
  const SECRET = "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234";

  /** Every string a pane puts on screen, whichever kind of pane it is. */
  const paneText = (p: ToolPane) =>
    p.type === "code"
      ? [p.code]
      : [p.oldCode, p.newCode, ...p.diff.rows.map((r) => r.text)];

  /** Every string this module would put on screen for a session. */
  const rendered = (a: AnalyzedSession): string =>
    a.events
      .filter(isToolEvent)
      .flatMap((e) =>
        e.kind === "tool-call"
          ? [callView(e).summary, ...callView(e).panes.flatMap(paneText)]
          : [e.preview, resultPane(e).code]
      )
      .join("\n");

  test("nothing rendered from a redacted session contains the secret", () => {
    expect(rendered(redactSession(golden()))).not.toContain(SECRET);
  });

  test("and the raw session is what still does — so the test can tell", () => {
    expect(rendered(golden())).toContain(SECRET);
  });
});
