import { describe, expect, test } from "bun:test";
import {
  parseWorkflowRun,
  pickWorkflowCall,
  toAgentRef,
  workflowCallsByRun,
} from "../../src/services/sessions/workflows";

const result = (callId: string, runId: unknown, timestamp?: string) =>
  JSON.stringify({
    type: "user",
    ...(timestamp ? { timestamp } : {}),
    message: {
      role: "user",
      content: [{ type: "tool_result", tool_use_id: callId, content: "ok" }],
    },
    toolUseResult: { runId },
  });

const T1 = "2026-06-01T10:00:00.000Z";
const T2 = "2026-06-01T10:05:00.000Z";

describe("workflowCallsByRun", () => {
  test("maps a run id to its Workflow call and result time", () => {
    expect([...workflowCallsByRun(result("toolu_1", "wf_a", T1))]).toEqual([
      ["wf_a", [{ callId: "toolu_1", at: Date.parse(T1) }]],
    ]);
  });

  test("a resumed run keeps every call in transcript order", () => {
    const text = [
      result("toolu_1", "wf_a", T1),
      result("toolu_2", "wf_a", T2),
      result("toolu_2", "wf_a", T2),
    ].join("\n");
    expect(workflowCallsByRun(text).get("wf_a")).toEqual([
      { callId: "toolu_1", at: Date.parse(T1) },
      { callId: "toolu_2", at: Date.parse(T2) },
    ]);
  });

  test("bad lines are skipped", () => {
    const text = [
      '{"runId": broken',
      "not json",
      result("toolu_2", 42),
      result("toolu_3", "wf_b"),
    ].join("\n");
    expect([...workflowCallsByRun(text)]).toEqual([
      ["wf_b", [{ callId: "toolu_3" }]],
    ]);
  });

  test("lines with runId but no tool_result are ignored", () => {
    const text = [
      JSON.stringify({ type: "user", toolUseResult: { runId: "wf_a" } }),
      JSON.stringify({
        type: "assistant",
        message: { content: [{ type: "text", text: "x" }] },
        toolUseResult: { runId: "wf_b" },
      }),
      JSON.stringify({ type: "queue-operation", runId: "wf_c" }),
    ].join("\n");
    expect(workflowCallsByRun(text).size).toBe(0);
  });
});

describe("pickWorkflowCall", () => {
  const calls = [
    { callId: "start", at: Date.parse(T1) },
    { callId: "resume", at: Date.parse(T2) },
  ];

  test("picks the last call that returned before the agent started", () => {
    expect(pickWorkflowCall(calls, Date.parse(T1) + 1)).toBe("start");
    expect(pickWorkflowCall(calls, Date.parse(T2) + 1)).toBe("resume");
  });

  test("falls back to the first call", () => {
    expect(pickWorkflowCall(calls)).toBe("start");
    expect(pickWorkflowCall(calls, Date.parse(T1) - 1)).toBe("start");
    expect(pickWorkflowCall([{ callId: "only" }], Date.parse(T2))).toBe("only");
    expect(pickWorkflowCall([], Date.parse(T2))).toBeUndefined();
  });
});

describe("parseWorkflowRun", () => {
  test("reads per-agent phases from workflow_agent entries", () => {
    const run = parseWorkflowRun(
      JSON.stringify({
        workflowProgress: [
          { type: "workflow_phase", index: 0, title: "survey" },
          {
            type: "workflow_agent",
            agentId: "a1",
            phaseIndex: 0,
            phaseTitle: "survey",
            startedAt: 1000,
          },
          { type: "workflow_agent", agentId: "a2" },
          { type: "workflow_agent", phaseIndex: 1 },
        ],
      })
    );
    expect([...run.agents]).toEqual([
      ["agent-a1", { phaseIndex: 0, phaseTitle: "survey", startedAt: 1000 }],
      ["agent-a2", {}],
    ]);
  });

  test("bad JSON or missing keys give an empty result", () => {
    expect(parseWorkflowRun("{nope").agents.size).toBe(0);
    expect(parseWorkflowRun("[]").agents.size).toBe(0);
    expect(parseWorkflowRun("{}").agents.size).toBe(0);
  });
});

describe("agent ids", () => {
  test("toAgentRef adds the agent- prefix once", () => {
    expect(toAgentRef("plain")).toBe("agent-plain");
    expect(toAgentRef("agent-plain")).toBe("agent-plain");
  });
});
