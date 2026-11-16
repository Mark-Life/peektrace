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
import { join } from "node:path";

import { pickTargets, stage } from "./build-npm";
import { TARGETS, type Target, variantName } from "./npm-meta";

const README = "# Peektrace\n\nUse it.\n\n# Develop\n\ndev only\n";

const dirs: string[] = [];
const scratch = () => {
  const dir = mkdtempSync(join(tmpdir(), "build-npm-"));
  dirs.push(dir);
  return dir;
};

afterEach(() => {
  for (const dir of dirs.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

const assetsDir = (targets: readonly Target[] = TARGETS) => {
  const dir = scratch();
  for (const t of targets) {
    writeFileSync(join(dir, t.asset), `bin ${t.asset}`);
  }
  return dir;
};

const readJson = (path: string) => JSON.parse(readFileSync(path, "utf8"));

describe("stage", () => {
  test("stages every variant and a wrapper pinning them", () => {
    const out = join(scratch(), "dist-npm");
    stage({
      version: "1.2.3",
      assets: assetsDir(),
      out,
      targets: TARGETS,
      readme: README,
    });
    const wrapper = readJson(join(out, "peektrace/package.json"));
    expect(wrapper.version).toBe("1.2.3");
    expect(Object.keys(wrapper.optionalDependencies).sort()).toEqual(
      TARGETS.map(variantName).sort()
    );
    expect(Object.values(wrapper.optionalDependencies)).toEqual(
      TARGETS.map(() => "1.2.3")
    );
    expect(
      readFileSync(join(out, "peektrace/README.md"), "utf8")
    ).not.toContain("dev only");
    const win = join(out, "@peektrace/cli-win32-x64");
    expect(readFileSync(join(win, "bin/peektrace.exe"), "utf8")).toBe(
      "bin peektrace-windows-x64.exe"
    );
    const mac = join(out, "@peektrace/cli-darwin-arm64");
    expect(readJson(join(mac, "package.json"))).toMatchObject({
      name: "@peektrace/cli-darwin-arm64",
      version: "1.2.3",
      os: ["darwin"],
      cpu: ["arm64"],
    });
    expect(statSync(join(mac, "bin/peektrace")).mode % 0o1000).toBe(0o755);
  });

  test("a subset stages only those variants", () => {
    const [host] = pickTargets("linux-arm64");
    if (!host) {
      throw new Error("no target");
    }
    const out = join(scratch(), "dist-npm");
    stage({
      version: "0.1.0",
      assets: assetsDir([host]),
      out,
      targets: [host],
      readme: README,
    });
    expect(
      existsSync(join(out, "@peektrace/cli-linux-arm64/bin/peektrace"))
    ).toBe(true);
    expect(existsSync(join(out, "@peektrace/cli-darwin-arm64"))).toBe(false);
  });

  test("fails on a missing binary or a bad version", () => {
    const out = join(scratch(), "dist-npm");
    const base = { out, targets: TARGETS, readme: README };
    expect(() =>
      stage({ ...base, version: "1.0.0", assets: scratch() })
    ).toThrow("Missing release binary");
    expect(() =>
      stage({ ...base, version: "cli-v1.0.0", assets: assetsDir() })
    ).toThrow("Not a semver version");
  });
});

describe("pickTargets", () => {
  test("all by default, rejects unknown keys", () => {
    expect(pickTargets(undefined)).toEqual(TARGETS);
    expect(() => pickTargets("linux-riscv")).toThrow("Unknown target");
  });
});
