import { describe, expect, test } from "bun:test";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { changelogSection, latestRelease } from "./changelog";

const FIXTURE = `# Changelog

## Unreleased

### Added

- pending thing

## cli-v0.2.0 — 2026-02-02

### Added

- second

\`\`\`md
## not a heading
\`\`\`

See [guide](docs/guide.md) and [site](https://example.com) and [top](#top).

## cli-v0.1.10 – 2026-01-10

- ten

## cli-v0.1.1 - 2026-01-01

- one

## desktop-v0.0.1

- desktop
`;

const REPO_CHANGELOG = join(import.meta.dir, "../../../CHANGELOG.md");
const SCRIPT = join(import.meta.dir, "release-notes.ts");

describe("changelogSection", () => {
  test("returns a middle section with subheadings and fenced headings", () => {
    const body = changelogSection(FIXTURE, "cli-v0.2.0");
    expect(body).toStartWith("### Added\n\n- second");
    expect(body).toContain("## not a heading");
    expect(body).not.toContain("- ten");
  });

  test("reads the section after Unreleased and ends at the next heading", () => {
    expect(changelogSection(FIXTURE, "Unreleased")).toBe(
      "### Added\n\n- pending thing"
    );
  });

  test("reads the last section to end of file, trimmed", () => {
    expect(changelogSection(FIXTURE, "desktop-v0.0.1")).toBe("- desktop");
  });

  test("matches tags exactly across prefixes and dash styles", () => {
    expect(changelogSection(FIXTURE, "cli-v0.1.1")).toBe("- one");
    expect(changelogSection(FIXTURE, "cli-v0.1.10")).toBe("- ten");
    expect(changelogSection(FIXTURE, "cli-v0.2.0-rc.1")).toBeUndefined();
    expect(changelogSection(FIXTURE, "cli-v9.9.9")).toBeUndefined();
  });

  test("rewrites only relative links when given a repo URL", () => {
    const body = changelogSection(FIXTURE, "cli-v0.2.0", {
      repoUrl: "https://github.com/acme/widget",
    });
    expect(body).toContain(
      "](https://github.com/acme/widget/blob/cli-v0.2.0/docs/guide.md)"
    );
    expect(body).toContain("](https://example.com)");
    expect(body).toContain("](#top)");
  });
});

describe("changelogSection edge cases", () => {
  test("mixed fence markers do not close each other", () => {
    const md =
      "## cli-v1.0.0 — 2026-01-01\n~~~md\n```\n## not\n~~~\n\nafter\n\n## cli-v0.9.0 — 2025-01-01\nold\n";
    expect(changelogSection(md, "cli-v1.0.0")).toBe(
      "~~~md\n```\n## not\n~~~\n\nafter"
    );
    const swallow =
      "## cli-v1.0.0 — 2026-01-01\n~~~md\n```\n~~~\n\n## cli-v0.9.0\nold\n";
    expect(changelogSection(swallow, "cli-v1.0.0")).toBe("~~~md\n```\n~~~");
  });

  test("heading with trailing text keeps a clean tag", () => {
    const md = "## cli-v1.0.0 — 2026-01-01 (hotfix)\nx\n";
    expect(changelogSection(md, "cli-v1.0.0")).toBe("x");
    expect(latestRelease("## cli-v1.0.0 (2026-01-01)\n")).toEqual({
      tag: "cli-v1.0.0",
      version: "1.0.0",
      date: undefined,
    });
    expect(latestRelease(md)?.date).toBe("2026-01-01");
  });

  test("leaves code fences and inline code alone, images use raw", () => {
    const md =
      "## cli-v1.0.0\n```ts\nconst x = a[0](b)\n```\nuse `[x](y)` and ![img](shot.png) and [doc](d.md)\n";
    expect(
      changelogSection(md, "cli-v1.0.0", { repoUrl: "https://github.com/o/r" })
    ).toBe(
      "```ts\nconst x = a[0](b)\n```\nuse `[x](y)` and ![img](https://github.com/o/r/raw/cli-v1.0.0/shot.png) and [doc](https://github.com/o/r/blob/cli-v1.0.0/d.md)"
    );
  });
});

describe("link rewriting", () => {
  test("rewrites links whose text is inline code, skips code spans", () => {
    const md =
      "## cli-v1.0.0\nsee [`docs/a.md`](docs/a.md), [b](b.md) and `[x](y)`\n";
    expect(
      changelogSection(md, "cli-v1.0.0", { repoUrl: "https://github.com/o/r" })
    ).toBe(
      "see [`docs/a.md`](https://github.com/o/r/blob/cli-v1.0.0/docs/a.md), [b](https://github.com/o/r/blob/cli-v1.0.0/b.md) and `[x](y)`"
    );
  });
});

describe("latestRelease", () => {
  test("skips Unreleased and returns tag, version and date", () => {
    expect(latestRelease(FIXTURE)).toEqual({
      tag: "cli-v0.2.0",
      version: "0.2.0",
      date: "2026-02-02",
    });
  });

  test("honors the prefix", () => {
    expect(latestRelease(FIXTURE, "desktop-v")?.version).toBe("0.0.1");
  });

  test("is undefined with only Unreleased", () => {
    expect(
      latestRelease("# Changelog\n\n## Unreleased\n\n- x\n")
    ).toBeUndefined();
  });
});

const run = (args: string[], repo?: string) => {
  const { GITHUB_REPOSITORY: _repo, ...env } = process.env;
  return Bun.spawnSync([process.execPath, SCRIPT, ...args], {
    env: repo ? { ...env, GITHUB_REPOSITORY: repo } : env,
  });
};

describe("release-notes script", () => {
  test("prints the section for the latest repo release", () => {
    const markdown = readFileSync(REPO_CHANGELOG, "utf8");
    const latest = latestRelease(markdown);
    expect(latest).toBeDefined();
    const result = run([latest?.tag ?? ""]);
    expect(result.exitCode).toBe(0);
    const expected = changelogSection(markdown, latest?.tag ?? "", {
      repoUrl: "https://github.com/Mark-Life/peektrace",
    });
    expect(expected).toBeTruthy();
    expect(result.stdout.toString().trim()).toBe(expected ?? "");
  });

  test("prints nothing for an unknown tag", () => {
    const result = run(["cli-v999.0.0"]);
    expect(result.exitCode).toBe(0);
    expect(result.stdout.toString()).toBe("");
  });

  test("exits 1 without a tag", () => {
    expect(run([]).exitCode).toBe(1);
  });

  test("uses GITHUB_REPOSITORY for rewritten links", () => {
    const dir = mkdtempSync(join(tmpdir(), "release-notes-"));
    try {
      const file = join(dir, "CHANGELOG.md");
      writeFileSync(file, "## cli-v1.0.0\n[`a.md`](a.md)\n");
      const result = run(["cli-v1.0.0", file], "acme/widget");
      expect(result.stdout.toString().trim()).toBe(
        "[`a.md`](https://github.com/acme/widget/blob/cli-v1.0.0/a.md)"
      );
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
