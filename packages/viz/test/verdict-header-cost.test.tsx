/** The session verdict header shows the session cost as readable text. Synthetic fixtures only. */
import { expect, test } from "bun:test";

import type { AnalyzedSession } from "@workspace/view-core/api-types.gen";
import { renderToStaticMarkup } from "react-dom/server";

import { VerdictHeader } from "../src/components/verdict-header";
import { MOCK_SESSION } from "../src/mock/session";

const base: AnalyzedSession = MOCK_SESSION;

const render = (a: AnalyzedSession) =>
  renderToStaticMarkup(<VerdictHeader a={a} />);

test("estimate cost renders as text with the unpriced models named", () => {
  const html = render({
    ...base,
    cost: {
      source: "estimate",
      totalUsd: 1.23,
      unpricedModels: ["mystery-model"],
    },
  });

  expect(html).toContain("Cost");
  expect(html).toContain("~$1.23");
  expect(html).toContain("unpriced: mystery-model");
  expect(html).toContain('title="Estimate at list price"');
});

test("agent-reported OpenCode cost is labelled as the agent's figure", () => {
  const html = render({
    ...base,
    cost: { source: "agent", totalUsd: 0.5 },
    provider: "opencode",
  });

  expect(html).toContain("$0.50");
  expect(html).not.toContain("~$0.50");
  expect(html).toContain("recorded by OpenCode");
  expect(html).not.toContain("list price");
});

test("subagents share and unreadable subagents show as notes", () => {
  const html = render({
    ...base,
    cost: {
      source: "estimate",
      subagentsUsd: 0.75,
      totalUsd: 2,
      unreadableSubagents: 1,
    },
  });

  expect(html).toContain("incl. subagents ~$0.75");
  expect(html).toContain("1 subagent(s) unreadable");
});

test("a record far from the estimate shows a warning", () => {
  const html = render({
    ...base,
    cost: {
      estimateGap: -0.926,
      estimateUsd: 34.37,
      recordedUsd: 464.02,
      source: "agent",
      totalUsd: 464.02,
    },
  });

  expect(html).toContain("recorded by Claude Code");
  expect(html).toContain('data-testid="session-cost-warning"');
  expect(html).toContain("estimate from tokens ~$34.37 (93% lower)");
});

test("no cost means no cost cell", () => {
  expect(render(base)).not.toContain("Cost");
});

test("a fully unpriced session shows a dash, never $0.00", () => {
  const html = render({
    ...base,
    cost: { source: "estimate", unpricedModels: ["mystery-model"] },
  });

  expect(html).toContain("Cost");
  expect(html).toContain("—");
  expect(html).not.toContain("$0.00");
});
