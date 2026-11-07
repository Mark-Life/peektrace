import { readFileSync } from "node:fs";
import { latestRelease } from "../../../cli/scripts/changelog";

const CHANGELOG_URL = new URL("../../../../CHANGELOG.md", import.meta.url);

/**
 * Latest released CLI version, read from the repo CHANGELOG.md. Server-only:
 * import it from server components, never from a client component. The value
 * is fixed at build time.
 */
export const readLatestRelease = () => {
  try {
    return latestRelease(readFileSync(CHANGELOG_URL, "utf8"));
  } catch {
    return;
  }
};
