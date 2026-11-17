import { describe, expect, test } from "bun:test";

import {
  harnessMark,
  launcherHint,
  launcherMark,
  providerLine,
  providerMark,
  providerTooltip,
  sessionBranch,
  sessionModels,
  worktreeHint,
} from "../src/session-marks";

const NVIDIA = { id: "nvidia", name: "NVIDIA" };

describe("launcher", () => {
  test("hint names only the parts the launcher knows", () => {
    expect(launcherHint({ name: "t3code" })).toBe("Started from t3code");
    expect(
      launcherHint({ branch: "feat/a", name: "t3code", worktreePath: "/wt/a" })
    ).toBe("Started from t3code · branch feat/a · worktree /wt/a");
  });

  test("t3code draws T3, others their first two letters", () => {
    expect(launcherMark({ name: "t3code" }).text).toBe("T3");
    expect(launcherMark({ name: "zed" }).text).toBe("Ze");
    expect(launcherMark({ name: "t3code" }).label).toBe("Started from t3code");
  });
});

describe("harnessMark", () => {
  test("one monogram and full name per harness", () => {
    expect(harnessMark("claude-code")).toEqual({
      label: "Claude Code",
      text: "CC",
      title: "Claude Code",
    });
    expect(harnessMark("codex").text).toBe("Cx");
    expect(harnessMark("pi").text).toBe("π");
    expect(harnessMark("opencode").text).toBe("OC");
  });
});

describe("provider", () => {
  test("line names the maker and the exact model id", () => {
    expect(providerLine(NVIDIA, "nvidia/GLM-5.3-Flash-NVFP4")).toBe(
      "NVIDIA · nvidia/GLM-5.3-Flash-NVFP4"
    );
    expect(providerLine(undefined, "mystery")).toBe(
      "Unknown provider · mystery"
    );
  });

  test("tooltip puts the main model first, then every other model", () => {
    expect(
      providerTooltip({
        models: ["nvidia/GLM-5.3-Flash-NVFP4", "moonshotai/Kimi-K3"],
        provider: NVIDIA,
      })
    ).toBe("NVIDIA · nvidia/GLM-5.3-Flash-NVFP4\nmoonshotai/Kimi-K3");
  });

  test("an unknown maker draws a neutral mark; no model draws none", () => {
    expect(providerMark({ models: ["x-1"], provider: undefined })).toEqual({
      label: "Unknown provider · x-1",
      text: "?",
      title: "Unknown provider · x-1",
    });
    expect(providerMark({ models: [], provider: NVIDIA })).toBeUndefined();
  });

  test("models list the main model first, each once, no empty ids", () => {
    expect(sessionModels({ model: "b", models: ["a", "b", ""] })).toEqual([
      "b",
      "a",
    ]);
    expect(sessionModels({})).toEqual([]);
  });
});

describe("worktree", () => {
  const worktree = { path: "/h/.t3/worktrees/app/t3-1", project: "app" };

  test("hint names the path and the parent thread when known", () => {
    expect(worktreeHint({ parentTitle: "Plan it", worktree })).toBe(
      "Worktree: /h/.t3/worktrees/app/t3-1\nStarted by: Plan it"
    );
    expect(worktreeHint({ parentTitle: undefined, worktree })).toBe(
      "Worktree: /h/.t3/worktrees/app/t3-1"
    );
  });

  test("branch falls back to the worktree's for an empty or detached one", () => {
    const wt = { ...worktree, branch: "feat/x" };
    expect(sessionBranch({ gitBranch: "HEAD", worktree: wt })).toBe("feat/x");
    expect(sessionBranch({ worktree: wt })).toBe("feat/x");
    expect(sessionBranch({ gitBranch: "main", worktree: wt })).toBe("main");
    expect(sessionBranch({ gitBranch: "HEAD", worktree })).toBeUndefined();
  });
});
