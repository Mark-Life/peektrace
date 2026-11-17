/** Session marks: logos for known makers, neutral monograms otherwise, and
 * one logo entry per committed SVG. */
import { expect, test } from "bun:test";
import { readdirSync } from "node:fs";

import type { AnalyzedSession } from "@workspace/view-core/api-types.gen";
import { renderToStaticMarkup } from "react-dom/server";

import { SessionMarks } from "../src/components/session-marks";
import { VerdictHeader } from "../src/components/verdict-header";
import { PROVIDER_LOGO } from "../src/lib/provider-logos";
import { MOCK_SESSION } from "../src/mock/session";

const LOGO_DIR = new URL("../../../assets/logos/providers/", import.meta.url);

test("every committed provider SVG has a logo entry", () => {
  const ids = readdirSync(LOGO_DIR)
    .filter((f) => f.endsWith(".svg"))
    .map((f) => f.slice(0, -".svg".length))
    .toSorted();

  expect([...PROVIDER_LOGO.keys()].toSorted()).toEqual(ids);
});

test("an unknown maker shows a neutral mark naming the model", () => {
  const html = renderToStaticMarkup(
    <SessionMarks
      harness="pi"
      launcher={undefined}
      models={["mystery-1"]}
      provider={undefined}
    />
  );

  expect(html).toContain('aria-label="Unknown provider · mystery-1"');
  expect(html).toContain(">?<");
  expect(html).not.toContain("session-launcher");
  expect(html).not.toContain("mask-image");
});

test("a known maker without a logo falls back to its monogram", () => {
  const html = renderToStaticMarkup(
    <SessionMarks
      harness="codex"
      launcher={undefined}
      models={["new-1"]}
      provider={{ id: "newco", name: "NewCo" }}
    />
  );

  expect(html).toContain(">Ne<");
});

test("the detail header names each mark, the worktree and the parent", () => {
  const a: AnalyzedSession = {
    ...MOCK_SESSION,
    gitBranch: "HEAD",
    launcher: { name: "t3code", parentTitle: "Plan it" },
    modelProvider: { id: "anthropic", name: "Anthropic" },
    models: ["claude-opus-5-5"],
    provider: "claude-code",
    worktree: { branch: "feat/x", path: "/wt/app/t3-1", project: "app" },
  };

  const html = renderToStaticMarkup(<VerdictHeader a={a} />);

  expect(html).toContain(">t3code<");
  expect(html).toContain(">Claude Code<");
  expect(html).toContain(">Anthropic · claude-opus-5-5<");
  expect(html).toContain("app worktree");
  expect(html).toContain("· Started by: Plan it");
  expect(html).toContain(">feat/x<");
});
