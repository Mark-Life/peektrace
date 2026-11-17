/** A row with very long texts keeps its fixed parts (cost, date, count,
 * marks) whole and lets the flexible parts truncate. No DOM here, so the test
 * checks the class contract that does this in a browser. */
import { describe, expect, test } from "bun:test";

import type { SessionHeader } from "@workspace/view-core/api-types.gen";
import { renderToStaticMarkup } from "react-dom/server";

import { SessionList } from "../src/routes/sessions/session-list";

const LONG = "x".repeat(400);

const row: SessionHeader = {
  agent: "claude",
  costSource: "estimate",
  costUsd: 12.34,
  cwd: `/h/.t3/worktrees/${LONG}/t3-1a2b3c4d`,
  gitBranch: "HEAD",
  id: "long-1",
  launcher: { name: "t3code", parentTitle: "Plan the long thing" },
  messageCount: 42,
  model: "nvidia/GLM-5.3-Flash-NVFP4",
  modelProvider: { id: "nvidia", name: "NVIDIA" },
  models: ["nvidia/GLM-5.3-Flash-NVFP4", "moonshotai/Kimi-K3"],
  path: "/h/s.jsonl",
  project: "slug",
  sizeBytes: 2048,
  sourceLabel: `source ${LONG}`,
  startedAt: "2026-11-01T08:00:00.000Z",
  title: `title ${LONG}`,
  updatedAt: "2026-11-01T09:00:00.000Z",
  worktree: {
    branch: `feat/${LONG}`,
    path: `/h/.t3/worktrees/${LONG}/t3-1a2b3c4d`,
    project: `project-${LONG}`,
    projectRoot: `/home/dev/project-${LONG}`,
  },
};

const html = renderToStaticMarkup(
  <SessionList headers={[row]} selectedId={null} />
);

/** The opening tag of the element with `testId`. */
const tagOf = (testId: string) =>
  new RegExp(`<[a-z]+[^>]*data-testid="${testId}"[^>]*>`, "u").exec(
    html
  )?.[0] ?? "";

/** Where the element with `testId` starts in the markup. */
const at = (testId: string) => html.indexOf(`data-testid="${testId}"`);

const classesOf = (testId: string) =>
  new Set(/class="(?<c>[^"]*)"/u.exec(tagOf(testId))?.groups?.c?.split(" "));

describe("a row with very long title, project and branch", () => {
  test("fixed parts never shrink", () => {
    for (const id of [
      "session-row-cost",
      "session-row-date",
      "session-row-count",
      "session-marks",
      "session-worktree",
    ]) {
      expect(classesOf(id).has("shrink-0")).toBe(true);
    }

    expect(classesOf("session-row-count").has("ml-auto")).toBe(true);
  });

  test("project and branch truncate, and the branch gives way first", () => {
    for (const id of ["session-row-project", "session-row-branch"]) {
      const c = classesOf(id);
      expect(c.has("truncate")).toBe(true);
      expect(c.has("shrink-0")).toBe(false);
    }

    expect(classesOf("session-row-project").has("min-w-0")).toBe(true);
    // Shrink is weighted by content width: a far larger factor makes the
    // branch give way before a short project, down to a few visible chars.
    const branch = classesOf("session-row-branch");
    expect(branch.has("shrink-[100]")).toBe(true);
    expect(branch.has("min-w-[3ch]")).toBe(true);
  });

  test("the title truncates with its full text as a tooltip", () => {
    expect(html).toContain(
      `class="min-w-0 flex-1 truncate text-sm font-medium" title="title ${LONG}"`
    );
  });

  test("line two runs marks, worktree, project, branch, count", () => {
    const order = [
      "session-marks",
      "session-worktree",
      "session-row-project",
      "session-row-branch",
      "session-row-count",
    ].map(at);

    expect(order.every((i) => i > -1)).toBe(true);
    expect(order.toSorted((a, b) => a - b)).toEqual(order);
  });

  test("shows the parent project and the worktree branch, not the folder", () => {
    expect(tagOf("session-row-project")).toContain(
      `title="/home/dev/project-${LONG}"`
    );
    expect(html).toContain(`>feat/${LONG}<`);
    expect(html).not.toContain(">t3-1a2b3c4d<");
  });

  test("the worktree marker names its path and the parent thread", () => {
    expect(tagOf("session-worktree")).toContain(
      "Started by: Plan the long thing"
    );
  });

  test("the worktree marker keeps the path out of the row's spoken name", () => {
    expect(tagOf("session-worktree")).toContain('aria-label="Worktree"');
  });

  test("marks run launcher, harness, model maker, each with a name", () => {
    expect(tagOf("session-launcher")).toContain(
      'aria-label="Started from t3code"'
    );
    expect(tagOf("session-harness")).toContain('aria-label="Claude Code"');

    const maker = html.slice(
      at("session-model-provider"),
      at("session-row-count")
    );

    expect(tagOf("session-model-provider")).toContain(
      'aria-label="NVIDIA · nvidia/GLM-5.3-Flash-NVFP4"'
    );
    expect(maker).toContain("mask-image:url(");
    expect(maker).toContain("moonshotai/Kimi-K3");
  });

  test("no text badge names the launcher or the harness", () => {
    expect(html).not.toContain(">t3code<");
    expect(html).not.toContain(">Claude<");
  });
});
