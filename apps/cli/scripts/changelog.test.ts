import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
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

const run = (...args: string[]) => {
  const { GITHUB_REPOSITORY: _repo, ...env } = process.env;
  return Bun.spawnSync([process.execPath, SCRIPT, ...args], { env });
};

describe("release-notes script", () => {
  test("prints the section for the latest repo release", () => {
    const markdown = readFileSync(REPO_CHANGELOG, "utf8");
    const latest = latestRelease(markdown);
    expect(latest).toBeDefined();
    const result = run(latest?.tag ?? "");
    expect(result.exitCode).toBe(0);
    const expected = changelogSection(markdown, latest?.tag ?? "");
    expect(expected).toBeTruthy();
    expect(result.stdout.toString().trim()).toBe(expected ?? "");
  });

  test("prints nothing for an unknown tag", () => {
    const result = run("cli-v999.0.0");
    expect(result.exitCode).toBe(0);
    expect(result.stdout.toString()).toBe("");
  });

  test("exits 1 without a tag", () => {
    expect(run().exitCode).toBe(1);
  });
});
