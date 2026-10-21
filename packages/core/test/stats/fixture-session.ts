/** Build a synthetic Claude transcript from a list of tool calls, and parse it
 * the way the stats service does. Every string here is made up. */
import { parseClaudeSession } from "../../src/services/sessions/parse";
import type { ParsedSession } from "../../src/services/sessions/schema";
import { makeRedactor, type Redactor } from "../../src/services/stats/redact";
import { CauseId } from "../../src/services/stats/schema";
import type { TranscriptRef } from "../../src/services/stats/walk";

/** Every catalog id, in declaration order. */
export const CAUSE_IDS = CauseId.literals;

/** One tool call and its result. */
export interface Step {
  /** Seconds after the session start; default 10 per step. */
  readonly at?: number;
  /** Context tokens the model call carried; default 10,000. */
  readonly ctx?: number;
  readonly input: Record<string, unknown>;
  readonly isError: boolean;
  readonly name: string;
  readonly output: string;
  /** Request id; a step reusing the previous one is a parallel sibling. */
  readonly req?: string;
  /** Sidechain call inside the main transcript. */
  readonly side?: boolean;
}

const START = Date.parse("2026-08-01T10:00:00.000Z");
const STEP_SEC = 10;
const DEFAULT_CTX = 10_000;
const MS = 1000;
const ID_WIDTH = 3;

/** The transcript text for `steps`, one assistant and one user line each. */
export const transcriptOf = (steps: readonly Step[]): string => {
  const lines: string[] = [
    JSON.stringify({
      type: "user",
      cwd: "/work/proj",
      timestamp: new Date(START).toISOString(),
      message: { role: "user", content: "do the task" },
    }),
  ];
  steps.forEach((step, i) => {
    const at = START + (step.at ?? (i + 1) * STEP_SEC) * MS;
    const id = `toolu_${String(i).padStart(ID_WIDTH, "0")}`;
    const side = step.side === true;
    lines.push(
      JSON.stringify({
        type: "assistant",
        requestId: step.req ?? `req_${i}`,
        isSidechain: side,
        cwd: "/work/proj",
        timestamp: new Date(at).toISOString(),
        message: {
          model: "claude-test",
          usage: {
            input_tokens: 1,
            cache_read_input_tokens: (step.ctx ?? DEFAULT_CTX) - 1,
            cache_creation_input_tokens: 0,
            output_tokens: 10,
          },
          content: [
            { type: "tool_use", id, name: step.name, input: step.input },
          ],
        },
      }),
      JSON.stringify({
        type: "user",
        isSidechain: side,
        timestamp: new Date(at + MS).toISOString(),
        message: {
          role: "user",
          content: [
            {
              type: "tool_result",
              tool_use_id: id,
              content: step.output,
              is_error: step.isError,
            },
          ],
        },
      })
    );
  });
  return `${lines.join("\n")}\n`;
};

/** A main-transcript ref for session `sid`. */
export const refOf = (sid = "sess-1"): TranscriptRef => ({
  id: sid,
  path: `/fixture/proj/${sid}.jsonl`,
  slug: "proj",
  agent: "claude",
  kind: "main",
  parent: null,
  run: null,
});

/** Everything `extractFacts` takes, for one synthetic session. */
export const parsedOf = (
  steps: readonly Step[],
  sid = "sess-1"
): {
  readonly parsed: ParsedSession;
  readonly project: string;
  readonly redact: Redactor;
  readonly ref: TranscriptRef;
} => ({
  parsed: parseClaudeSession({
    text: transcriptOf(steps),
    path: `/fixture/proj/${sid}.jsonl`,
    sessionId: sid,
  }),
  project: "proj",
  redact: makeRedactor({ enabled: true }),
  ref: refOf(sid),
});
