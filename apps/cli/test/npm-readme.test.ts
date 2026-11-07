import { describe, expect, test } from "bun:test";
import { join } from "node:path";
import { npmReadme } from "../scripts/npm-readme";

const RELATIVE_MD_LINK = /\]\((?!https?:|#)/;
const REF = "cli-v0.0.0-test";

describe("npmReadme (synthetic input)", () => {
  const input = [
    "# Title",
    "",
    "![shot](assets/a.webp)",
    "",
    "See [cli docs](apps/cli/README.md) and [site](https://example.com).",
    "",
    "# Develop",
    "",
    "dev only",
    "",
  ].join("\n");
  const out = npmReadme(input, REF);

  test("cuts at Develop", () => {
    expect(out).not.toContain("dev only");
    expect(out).not.toContain("# Develop");
  });
  test("drops images", () => {
    expect(out).not.toContain("assets/");
    expect(out).not.toContain(".webp");
  });
  test("pins relative links to ref, keeps absolute", () => {
    expect(out).toContain(
      `](https://github.com/Mark-Life/peektrace/blob/${REF}/apps/cli/README.md)`
    );
    expect(out).toContain("](https://example.com)");
  });
  test("appends source and license footer", () => {
    expect(out).toContain("Source: https://github.com/Mark-Life/peektrace");
    expect(out).toContain("License: MIT");
  });
});

describe("npmReadme (root README)", async () => {
  const root = await Bun.file(
    join(import.meta.dirname, "../../../README.md")
  ).text();
  const out = npmReadme(root, REF);

  test("has no repo-relative links or images", () => {
    expect(out).not.toMatch(RELATIVE_MD_LINK);
    expect(out).not.toContain("assets/");
    expect(out).not.toContain(".webp");
  });
  test("keeps the Use half only", () => {
    expect(out).not.toContain("# Develop");
    expect(out).toContain("npx peektrace");
    expect(out).toContain("PEEKTRACE_BIN_PATH");
  });
  test("ends with a single rule before the footer", () => {
    expect(out).not.toContain("---\n\n---");
    expect(out).toContain("\n\n---\n\nSource: ");
  });
  test("names all four agents", () => {
    for (const name of ["Claude Code", "Codex", "Pi", "OpenCode"]) {
      expect(out).toContain(name);
    }
  });
});
