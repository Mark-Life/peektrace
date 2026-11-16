import { describe, expect, test } from "bun:test";

import { costSummary, fmtRecordedUsd, fmtUsd } from "../src/cost";

describe("cost formatting", () => {
  test("fmtUsd rounds to cents and marks sub-cent spend", () => {
    expect(fmtUsd(0)).toBe("~$0.00");
    expect(fmtUsd(0.001)).toBe("<$0.01");
    expect(fmtUsd(1.234)).toBe("~$1.23");
    expect(fmtRecordedUsd(464.0249)).toBe("$464.02");
    expect(fmtRecordedUsd(0.001)).toBe("<$0.01");
  });

  test("costSummary lists only the qualifiers that are set", () => {
    expect(costSummary({ source: "estimate", totalUsd: 1 }, "X").notes).toEqual(
      ["estimated from tokens"]
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
      "recorded by OpenCode",
      "incl. subagents ~$0.25",
      "unpriced: mystery-a, mystery-b",
      "2 subagent(s) unreadable",
    ]);
  });

  test("costSummary dashes an unpriced total and names an agent's own figure", () => {
    expect(
      costSummary({ source: "estimate", unpricedModels: ["mystery-a"] }, "X")
    ).toEqual({
      notes: ["estimated from tokens", "unpriced: mystery-a"],
      total: "—",
      warning: undefined,
    });
    expect(costSummary({ source: "agent", totalUsd: 0.5 }, "OpenCode")).toEqual(
      {
        estimateNote: undefined,
        notes: ["recorded by OpenCode"],
        total: "$0.50",
        warning: undefined,
      }
    );
  });

  test("costSummary warns when core flags the estimate", () => {
    const s = costSummary(
      {
        estimateGap: -0.926,
        estimateUsd: 34.37,
        estimateWarning: true,
        recordedUsd: 464.02,
        source: "agent",
        totalUsd: 464.02,
      },
      "Claude Code"
    );

    expect(s.warning).toBe("estimate from tokens ~$34.37 (93% lower)");
    expect(s.estimateNote).toBeUndefined();
  });

  test("costSummary shows an unflagged estimate quietly", () => {
    const s = costSummary(
      {
        estimateGap: -0.2,
        estimateUsd: 8,
        recordedUsd: 10,
        source: "agent",
        totalUsd: 10,
      },
      "Claude Code"
    );

    expect(s.warning).toBeUndefined();
    expect(s.estimateNote).toBe(
      "Estimate from transcript: ~$8.00. Transcripts miss some billed calls."
    );
  });

  test("costSummary names a partial record next to the estimate", () => {
    expect(
      costSummary(
        {
          recordPartial: true,
          recordedUsd: 74.42,
          source: "estimate",
          totalUsd: 95.23,
        },
        "Claude Code"
      )
    ).toEqual({
      notes: [
        "estimated from tokens",
        "Claude Code recorded $74.42 for part of the session",
      ],
      total: "~$95.23",
      warning: undefined,
    });
  });
});
