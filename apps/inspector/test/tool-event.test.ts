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

  test("unwraps the payload arg so its newlines survive", () => {
    const { panes } = callView(bashCall());
    expect(panes[0]).toEqual({
      code: "bun test\necho done",
      label: "command",
      language: "bash",
    });
  });

  test("keeps every other argument in a pane of its own", () => {
    const { panes } = callView(bashCall());
    expect(panes).toHaveLength(2);
    expect(panes[1]?.label).toBe("Other parameters");
    expect(JSON.parse(panes[1]?.code ?? "")).toEqual({
      description: "Run the suite",
      timeout: 120_000,
    });
  });

  test("summarizes the call by its payload, not its JSON envelope", () => {
    expect(callView(bashCall()).summary).toBe("bun test echo done");
  });

  test("a call with no payload arg falls back to the whole argument object", () => {
    const a = sessionFrom(OUTCOMES, "outcomes");
    const { panes, summary } = callView(eventAt(a, "call-bad", "tool-call"));
    expect(panes).toHaveLength(1);
    expect(panes[0]?.label).toBe("Parameters");
    expect(JSON.parse(panes[0]?.code ?? "")).toEqual({ pattern: "TODO" });
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

  /** Every string this module would put on screen for a session. */
  const rendered = (a: AnalyzedSession): string =>
    a.events
      .filter(isToolEvent)
      .flatMap((e) =>
        e.kind === "tool-call"
          ? [callView(e).summary, ...callView(e).panes.map((p) => p.code)]
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
