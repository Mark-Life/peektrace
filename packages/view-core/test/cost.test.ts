import { describe, expect, test } from "bun:test";

import { costSummary, fmtRecordedUsd, fmtUsd, headerCost } from "../src/cost";

describe("cost formatting", () => {
  test("fmtUsd rounds to cents and marks sub-cent spend", () => {
    expect(fmtUsd(0)).toBe("~$0.00");
    expect(fmtUsd(0.001)).toBe("<$0.01");
    expect(fmtUsd(1.234)).toBe("~$1.23");
    expect(fmtRecordedUsd(464.0249)).toBe("$464.02");
    expect(fmtRecordedUsd(0.001)).toBe("<$0.01");
  });

  test("costSummary lists only the qualifiers that are set", () => {
    expect(
      costSummary({ source: "estimate", totalUsd: 1 }, "codex").notes
    ).toEqual(["estimated from tokens"]);
    expect(
      costSummary(
        {
          source: "agent",
          subagentsUsd: 0.25,
          totalUsd: 1,
          unpricedModels: ["mystery-a", "mystery-b"],
          unreadableSubagents: 2,
        },
        "opencode"
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
      costSummary(
        { source: "estimate", unpricedModels: ["mystery-a"] },
        "codex"
      )
    ).toEqual({
      estimateNote: undefined,
      notes: ["estimated from tokens", "unpriced: mystery-a"],
      total: "—",
      warning: undefined,
    });
    expect(costSummary({ source: "agent", totalUsd: 0.5 }, "opencode")).toEqual(
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
      "claude-code"
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
      "claude-code"
    );

    expect(s.warning).toBeUndefined();
    expect(s.estimateNote).toBe(
      "Estimate from transcript: ~$8.00. Transcripts miss some billed calls."
    );
  });

  test("only Claude Code's note says transcripts miss calls", () => {
    const cost = {
      estimateUsd: 0.4,
      recordedUsd: 0.42,
      source: "agent",
      totalUsd: 0.42,
    } as const;

    expect(costSummary(cost, "opencode").estimateNote).toBe(
      "Estimate at list price: ~$0.40."
    );
    expect(costSummary(cost, "pi").estimateNote).toBe(
      "Estimate at list price: ~$0.40."
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
        "claude-code"
      )
    ).toEqual({
      estimateNote: undefined,
      notes: [
        "estimated from tokens",
        "Claude Code recorded $74.42 for part of the session",
      ],
      total: "~$95.23",
      warning: undefined,
    });
  });

  test("headerCost marks a list row's cost as the detail does", () => {
    expect(headerCost({ agent: "claude" })).toBeUndefined();
    expect(
      headerCost({ agent: "claude", costSource: "agent", costUsd: 12.5 })
    ).toEqual({
      label: "cost $12.50, recorded",
      text: "$12.50",
      title: "Cost: $12.50 · recorded by Claude Code",
    });
    expect(
      headerCost({
        agent: "codex",
        costSource: "estimate",
        costUsd: 3,
        subagentsUsd: 1,
        unpricedModels: ["mystery"],
      })
    ).toEqual({
      label: "cost about $3.00, estimated, some models unpriced",
      text: "~$3.00*",
      title:
        "Cost: ~$3.00 · estimated from tokens · incl. subagents ~$1.00 · unpriced: mystery",
    });
    expect(headerCost({ agent: "pi", unpricedModels: ["mystery"] })?.text).toBe(
      "—*"
    );
  });
});
