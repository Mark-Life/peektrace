import { describe, expect, test } from "bun:test";
import {
  DESCRIPTION,
  KEYWORDS,
  TARGETS,
  variantName,
} from "../scripts/npm-meta";

describe("npm metadata", () => {
  test("description names all four agents", () => {
    for (const name of ["Claude Code", "Codex", "Pi", "OpenCode"]) {
      expect(DESCRIPTION).toContain(name);
    }
    expect(DESCRIPTION).not.toContain("memories & sessions");
  });
  test("keywords cover every agent", () => {
    for (const k of ["codex", "pi", "opencode", "context-window"]) {
      expect(KEYWORDS).toContain(k as (typeof KEYWORDS)[number]);
    }
  });
  test("variants are scoped", () => {
    for (const t of TARGETS) {
      expect(variantName(t).startsWith("@peektrace/cli-")).toBe(true);
    }
  });
  test("targets include linux-arm64", () => {
    expect(variantName({ platform: "linux", arch: "arm64" })).toBe(
      "@peektrace/cli-linux-arm64"
    );
    expect(
      TARGETS.some((t) => t.platform === "linux" && t.arch === "arm64")
    ).toBe(true);
  });
});
