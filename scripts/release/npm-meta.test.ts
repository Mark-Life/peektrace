import { describe, expect, test } from "bun:test";

import { DESCRIPTION, KEYWORDS, TARGETS, variantName } from "./npm-meta";

describe("npm metadata", () => {
  test("description names all four agents", () => {
    for (const name of ["Claude Code", "Codex", "Pi", "OpenCode"]) {
      expect(DESCRIPTION).toContain(name);
    }
  });
  test("keywords cover every agent", () => {
    const keywords: readonly string[] = KEYWORDS;

    for (const k of ["codex", "pi", "opencode", "context-window"]) {
      expect(keywords).toContain(k);
    }
  });
  test("variants are scoped", () => {
    for (const t of TARGETS) {
      expect(variantName(t).startsWith("@peektrace/cli-")).toBe(true);
    }
  });
  test("targets include linux-arm64", () => {
    expect(
      TARGETS.some((t) => t.platform === "linux" && t.arch === "arm64")
    ).toBe(true);
  });
});
