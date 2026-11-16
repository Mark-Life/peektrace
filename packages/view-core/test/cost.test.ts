import { describe, expect, test } from "bun:test";

import { costSummary, fmtUsd } from "../src/cost";

describe("cost formatting", () => {
  test("fmtUsd rounds to cents and marks sub-cent spend", () => {
    expect(fmtUsd(0)).toBe("~$0.00");
    expect(fmtUsd(0.001)).toBe("<$0.01");
    expect(fmtUsd(1.234)).toBe("~$1.23");
  });

  test("costSummary lists only the qualifiers that are set", () => {
    expect(costSummary({ source: "estimate", totalUsd: 1 }, "X").notes).toEqual(
      []
    );
    expect(
      costSummary(
        {
          source: "agent",
          subagentsUsd: 0.25,
          totalUsd: 1,
          unpricedModels: ["mystery-a", "mystery-b"],
          unreadableSubagents: 2,
        },
        "OpenCode"
      ).notes
    ).toEqual([
      "reported by OpenCode",
      "subagents ~$0.25",
      "unpriced: mystery-a, mystery-b",
      "2 subagent(s) unreadable",
    ]);
  });

  test("costSummary dashes an unpriced total and names an agent's own figure", () => {
    expect(
      costSummary({ source: "estimate", unpricedModels: ["mystery-a"] }, "X")
    ).toEqual({ notes: ["unpriced: mystery-a"], total: "—" });
    expect(costSummary({ source: "agent", totalUsd: 0.5 }, "OpenCode")).toEqual(
      { notes: ["reported by OpenCode"], total: "~$0.50" }
    );
  });
});
