#!/usr/bin/env bun
/**
 * Print the CHANGELOG.md section for a release tag.
 *
 *   bun apps/cli/scripts/release-notes.ts <tag> [changelog-path]
 *
 * A missing section prints nothing and exits 0 so the caller can fall back to
 * generated notes. Bad arguments or an unreadable file exit 1. When
 * GITHUB_REPOSITORY is set, relative links are rewritten to blob URLs at the tag.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { changelogSection } from "./changelog";

const [tag, file = join(import.meta.dir, "../../../CHANGELOG.md")] =
  process.argv.slice(2);

if (!tag) {
  console.error("usage: release-notes.ts <tag> [changelog-path]");
  process.exit(1);
}

try {
  const repo = process.env.GITHUB_REPOSITORY;
  const section = changelogSection(
    readFileSync(file, "utf8"),
    tag,
    repo ? { repoUrl: `https://github.com/${repo}` } : undefined
  );
  if (section) {
    process.stdout.write(`${section}\n`);
  }
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}
