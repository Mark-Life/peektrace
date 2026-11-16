import { afterEach, describe, expect, test } from "bun:test";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  symlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { binPath, orderProblems } from "./goldens";
import { orderOf, rootProblem, takeLock } from "./goldens-host";
import { STAGES, stageFiles } from "./goldens-inputs";

const REPO = join(import.meta.dir, "../..");

describe("binPath", () => {
  test("keeps an absolute --bin", () => {
    expect(binPath("/opt/x/peektrace", "/r/repo")).toBe("/opt/x/peektrace");
  });

  test("resolves a relative --bin and the default against the repo", () => {
    expect(binPath("target/a", "/r/repo")).toBe("/r/repo/target/a");
    expect(binPath(undefined, "/r/repo")).toBe(
      "/r/repo/target/parity/peektrace"
    );
  });
});

describe("stageFiles", () => {
  const files = Object.fromEntries(
    STAGES.map((s) => [s, new Set(stageFiles(s, REPO))])
  );
  const stagesWith = (file: string) =>
    STAGES.filter((s) => files[s]?.has(file));

  test("inspector sources count for serve only", () => {
    expect(stagesWith("apps/inspector/src/fonts.css")).toEqual(["serve"]);
  });

  test("the helpers and chart the Rune app ports count for serve and ui", () => {
    for (const f of [
      "apps/inspector/src/lib/chat-lane.ts",
      "apps/inspector/src/routes/sessions/session-list-model.ts",
      "packages/viz/src/components/growth-timeline.tsx",
    ]) {
      expect(stagesWith(f)).toEqual(["serve", "ui"]);
    }
  });

  test("shared inputs count for every stage", () => {
    for (const f of [
      "scripts/rust/parity.ts",
      "scripts/rust/parity-oracle.ts",
      "apps/cli/src/index.ts",
      "bun.lock",
    ]) {
      expect(stagesWith(f)).toEqual([...STAGES]);
    }
  });

  test("a stage script counts for the stages that import it", () => {
    expect(stagesWith("scripts/rust/parity-tui.ts")).toEqual(["tui", "ui"]);
    expect(stagesWith("scripts/rust/parity-ui-growth.ts")).toEqual(["ui"]);
    expect(stagesWith("scripts/rust/parity-stats-cases.ts")).toEqual([
      "stats",
      "serve",
      "cli",
    ]);
  });

  test("leaves out tests and the goldens scripts", () => {
    const all = STAGES.flatMap((s) => [...(files[s] ?? [])]);
    expect(all.filter((f) => f.endsWith(".test.ts"))).toEqual([]);
    expect(all.filter((f) => f.includes("/goldens"))).toEqual([]);
  });
});

describe("orderOf", () => {
  test("names the listing order of a fresh dir", () => {
    expect(orderOf(["c", "a", "b"], ["c", "a", "b"])).toBe("creation");
    expect(orderOf(["c", "a", "b"], ["b", "a", "c"])).toBe("newest-first");
    expect(orderOf(["c", "a", "b"], ["a", "b", "c"])).toBe("other");
  });
});

describe("orderProblems", () => {
  test("a stage with no recorded order is unknown, not a match", () => {
    expect(
      orderProblems(
        [
          ["base", "creation"],
          ["serve", undefined],
          ["tui", "newest-first"],
        ],
        "creation"
      )
    ).toEqual({ other: ["tui"], unknown: ["serve"] });
  });

  test("every checked-in stage records its dir order", () => {
    const dir = join(REPO, "tests/goldens/linux-x64");
    // A stage not recorded yet fails the check itself (no goldens).
    const recorded = STAGES.filter((s) =>
      existsSync(join(dir, `${s}.json`))
    ).map((s) => {
      const meta = JSON.parse(readFileSync(join(dir, `${s}.json`), "utf8")) as {
        dirOrder?: "creation" | "newest-first" | "other";
      };
      return [s, meta.dirOrder] as const;
    });
    expect(orderProblems(recorded, "creation").unknown).toEqual([]);
  });
});

describe("goldens root", () => {
  const dirs: string[] = [];
  const scratch = () => {
    const dir = mkdtempSync(join(tmpdir(), "goldens-test-"));
    dirs.push(dir);
    return dir;
  };
  afterEach(() => {
    for (const d of dirs.splice(0)) {
      rmSync(d, { recursive: true, force: true });
    }
  });

  test("refuses a symlink, a file or another user's dir", () => {
    const base = scratch();
    const real = join(base, "real");
    mkdirSync(real);
    symlinkSync(real, join(base, "link"));
    writeFileSync(join(base, "file"), "");
    const uid = process.getuid?.() ?? 0;
    expect(rootProblem(join(base, "link"), uid)).toContain("symlink");
    expect(rootProblem(join(base, "file"), uid)).toContain("not a directory");
    expect(rootProblem(real, uid + 1)).toContain("belongs to uid");
    expect(rootProblem(real, uid)).toBeUndefined();
    expect(rootProblem(join(base, "missing"), uid)).toBeUndefined();
  });

  test("one run holds the lock; a dead holder's lock is taken over", () => {
    const root = join(scratch(), "root");
    const first = takeLock(root);
    expect(first.release).toBeDefined();
    expect(takeLock(root)).toEqual({ holder: process.pid });
    first.release?.();
    expect(existsSync(join(root, ".lock"))).toBe(false);

    const gone = Bun.spawnSync(["true"]).pid;
    writeFileSync(join(root, ".lock"), String(gone));
    const second = takeLock(root);
    expect(second.release).toBeDefined();
    second.release?.();
  });
});
