import { describe, expect, test } from "bun:test";
import {
  isDevVersion,
  isNpmInstall,
  updateHint,
} from "../src/upgrade/update-hint";

describe("isDevVersion", () => {
  test("matches dev pre-releases only", () => {
    expect(isDevVersion("0.0.0-dev")).toBe(true);
    expect(isDevVersion("1.2.0-dev.3")).toBe(true);
    expect(isDevVersion("0.11.0")).toBe(false);
    expect(isDevVersion("1.0.0-devious")).toBe(false);
  });
});

describe("isNpmInstall", () => {
  test("detects a node_modules path segment", () => {
    expect(
      isNpmInstall("/opt/x/node_modules/@peektrace/cli/bin/peektrace")
    ).toBe(true);
    expect(
      isNpmInstall("C:\\x\\node_modules\\@peektrace\\cli\\bin\\peektrace.exe")
    ).toBe(true);
    expect(isNpmInstall("/usr/local/bin/peektrace")).toBe(false);
    expect(isNpmInstall("/opt/node_modules_backup/peektrace")).toBe(false);
  });
});

describe("updateHint", () => {
  test("picks the command for the install kind", () => {
    expect(
      updateHint({ latest: "cli-v1.0.0", execPath: "/usr/local/bin/peektrace" })
    ).toBe(
      "A newer version (cli-v1.0.0) is available — run 'peektrace upgrade'"
    );
    expect(
      updateHint({
        latest: "cli-v1.0.0",
        execPath: "/a/node_modules/b/peektrace",
      })
    ).toContain("npm i -g peektrace@latest");
  });
});
