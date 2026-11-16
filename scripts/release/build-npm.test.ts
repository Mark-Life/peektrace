import { afterEach, describe, expect, test } from "bun:test";
import {
  existsSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import nodePath from "node:path";

import { pickTargets, stage } from "./build-npm";
import { TARGETS, variantName } from "./npm-meta";
import type { Target } from "./npm-meta";

const README = "# Peektrace\n\nUse it.\n\n# Develop\n\ndev only\n";

const dirs: string[] = [];

const scratch = () => {
  const dir = mkdtempSync(nodePath.join(tmpdir(), "build-npm-"));
  dirs.push(dir);

  return dir;
};

afterEach(() => {
  for (const dir of dirs.splice(0)) {
    rmSync(dir, { force: true, recursive: true });
  }
});

const assetsDir = (targets: readonly Target[] = TARGETS) => {
  const dir = scratch();

  for (const t of targets) {
    writeFileSync(nodePath.join(dir, t.asset), `bin ${t.asset}`);
  }

  return dir;
};

const expectJson = (path: string) =>
  expect(JSON.parse(readFileSync(path, "utf-8")));

describe("stage", () => {
  test("stages every variant and a wrapper pinning them", () => {
    const out = nodePath.join(scratch(), "dist-npm");
    stage({
      assets: assetsDir(),
      out,
      readme: README,
      targets: TARGETS,
      version: "1.2.3",
    });
    const wrapper = expectJson(nodePath.join(out, "peektrace/package.json"));
    wrapper.toHaveProperty("version", "1.2.3");
    wrapper.toHaveProperty(
      "optionalDependencies",
      Object.fromEntries(TARGETS.map((t) => [variantName(t), "1.2.3"]))
    );
    expect(
      readFileSync(nodePath.join(out, "peektrace/README.md"), "utf-8")
    ).not.toContain("dev only");
    const win = nodePath.join(out, "@peektrace/cli-win32-x64");
    expect(readFileSync(nodePath.join(win, "bin/peektrace.exe"), "utf-8")).toBe(
      "bin peektrace-windows-x64.exe"
    );
    const mac = nodePath.join(out, "@peektrace/cli-darwin-arm64");
    expectJson(nodePath.join(mac, "package.json")).toMatchObject({
      cpu: ["arm64"],
      name: "@peektrace/cli-darwin-arm64",
      os: ["darwin"],
      version: "1.2.3",
    });
    expect(statSync(nodePath.join(mac, "bin/peektrace")).mode % 0o1000).toBe(
      0o755
    );
  });

  test("a subset stages only those variants", () => {
    const [host] = pickTargets("linux-arm64");

    const out = nodePath.join(scratch(), "dist-npm");
    stage({
      assets: assetsDir([host]),
      out,
      readme: README,
      targets: [host],
      version: "0.1.0",
    });
    expect(
      existsSync(nodePath.join(out, "@peektrace/cli-linux-arm64/bin/peektrace"))
    ).toBe(true);
    expect(existsSync(nodePath.join(out, "@peektrace/cli-darwin-arm64"))).toBe(
      false
    );
  });

  test("fails on a missing binary or a bad version", () => {
    const out = nodePath.join(scratch(), "dist-npm");
    const base = { out, readme: README, targets: TARGETS };
    expect(() =>
      stage({ ...base, assets: scratch(), version: "1.0.0" })
    ).toThrow("Missing release binary");
    expect(() =>
      stage({ ...base, assets: assetsDir(), version: "cli-v1.0.0" })
    ).toThrow("Not a semver version");
  });
});

describe("pickTargets", () => {
  test("all by default, rejects unknown keys", () => {
    expect(pickTargets(undefined)).toEqual(TARGETS);
    expect(() => pickTargets("linux-riscv")).toThrow("Unknown target");
  });
});
