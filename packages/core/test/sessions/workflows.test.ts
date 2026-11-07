import { describe, expect, test } from "bun:test";
import {
  parseWorkflowRun,
  toAgentRef,
  toBareAgentId,
  workflowCallsByRun,
} from "../../src/services/sessions/workflows";

const result = (callId: string, runId: unknown) =>
  JSON.stringify({
    type: "user",
    message: {
      role: "user",
      content: [{ type: "tool_result", tool_use_id: callId, content: "ok" }],
    },
    toolUseResult: { runId },
  });

describe("workflowCallsByRun", () => {
  test("maps a run id to its Workflow call", () => {
    expect([...workflowCallsByRun(result("toolu_1", "wf_a"))]).toEqual([
      ["wf_a", "toolu_1"],
    ]);
  });

  test("the first duplicate run id wins", () => {
    const text = [result("toolu_1", "wf_a"), result("toolu_2", "wf_a")].join(
      "\n"
    );
    expect(workflowCallsByRun(text).get("wf_a")).toBe("toolu_1");
  });

  test("bad lines are skipped", () => {
    const text = [
      '{"runId": broken',
      "not json",
      result("toolu_2", 42),
      result("toolu_3", "wf_b"),
    ].join("\n");
    expect([...workflowCallsByRun(text)]).toEqual([["wf_b", "toolu_3"]]);
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

describe("parseWorkflowRun", () => {
  test("reads name and per-agent phases from workflow_agent entries", () => {
    const run = parseWorkflowRun(
      JSON.stringify({
        workflowName: "demo",
        workflowProgress: [
          { type: "workflow_phase", index: 0, title: "survey" },
          {
            type: "workflow_agent",
            agentId: "a1",
            phaseIndex: 0,
            phaseTitle: "survey",
          },
          { type: "workflow_agent", agentId: "a2" },
          { type: "workflow_agent", phaseIndex: 1 },
        ],
      })
    );
    expect(run.workflowName).toBe("demo");
    expect([...run.agents]).toEqual([
      ["a1", { phaseIndex: 0, phaseTitle: "survey" }],
      ["a2", {}],
    ]);
  });

  test("bad JSON or missing keys give an empty result", () => {
    expect(parseWorkflowRun("{nope").agents.size).toBe(0);
    expect(parseWorkflowRun("[]").agents.size).toBe(0);
    const empty = parseWorkflowRun("{}");
    expect(empty.agents.size).toBe(0);
    expect(empty.workflowName).toBeUndefined();
  });
});

describe("agent ids", () => {
  test("toAgentRef adds the agent- prefix once", () => {
    expect(toAgentRef("plain")).toBe("agent-plain");
    expect(toAgentRef("agent-plain")).toBe("agent-plain");
  });

  test("toBareAgentId strips the agent- prefix", () => {
    expect(toBareAgentId("agent-plain")).toBe("plain");
    expect(toBareAgentId("plain")).toBe("plain");
  });
});
