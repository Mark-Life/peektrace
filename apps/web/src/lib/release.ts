import { readFileSync } from "node:fs";
import { join } from "node:path";
import { latestRelease } from "../../../cli/scripts/changelog";

// `next build` runs with the web app as cwd.
const CHANGELOG_PATH = join(
  /* turbopackIgnore: true */ process.cwd(),
  "../../CHANGELOG.md"
);

/**
 * Latest released CLI version, read from the repo CHANGELOG.md. Server-only:
 * import it from server components, never from a client component. The value
 * is fixed at build time.
 */
export const readLatestRelease = () => {
  try {
    return latestRelease(readFileSync(CHANGELOG_PATH, "utf8"));
  } catch {
    return;
  }
};
