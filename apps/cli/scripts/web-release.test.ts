import { expect, test } from "bun:test";
import { join } from "node:path";

const WEB_DIR = join(import.meta.dir, "../../web");

test("web readLatestRelease finds a cli tag when cwd is the web app", () => {
  const script =
    'import { readLatestRelease } from "./src/lib/release"; console.log(JSON.stringify(readLatestRelease()));';
  const result = Bun.spawnSync([process.execPath, "-e", script], {
    cwd: WEB_DIR,
  });
  expect(result.exitCode).toBe(0);
  const release = JSON.parse(result.stdout.toString().trim());
  expect(release.tag).toStartWith("cli-v");
});
