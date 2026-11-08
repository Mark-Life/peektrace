/** The session verdict header shows the session cost as readable text. Synthetic fixtures only. */
import { expect, test } from "bun:test";
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
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
      totalUsd: 1.23,
      source: "estimate",
      unpricedModels: ["mystery-model"],
    },
  });
  expect(html).toContain("Est. cost");
  expect(html).toContain("~$1.23");
  expect(html).toContain("unpriced: mystery-model");
  expect(html).toContain('title="Estimate at list price"');
});

test("agent-reported OpenCode cost is labelled as the agent's figure", () => {
  const html = render({
    ...base,
    provider: "opencode",
    cost: { totalUsd: 0.5, source: "agent" },
  });
  expect(html).toContain("~$0.50");
  expect(html).toContain("reported by OpenCode");
  expect(html).not.toContain("list price");
});

test("subagents share and unreadable subagents show as notes", () => {
  const html = render({
    ...base,
    cost: {
      totalUsd: 2,
      subagentsUsd: 0.75,
      source: "estimate",
      unreadableSubagents: 1,
    },
  });
  expect(html).toContain("subagents ~$0.75");
  expect(html).toContain("1 subagent(s) unreadable");
});

test("no cost means no cost cell", () => {
  expect(render(base)).not.toContain("Est. cost");
});

test("a fully unpriced session shows a dash, never $0.00", () => {
  const html = render({
    ...base,
    cost: { source: "estimate", unpricedModels: ["mystery-model"] },
  });
  expect(html).toContain("Est. cost");
  expect(html).toContain("—");
  expect(html).not.toContain("$0.00");
});
