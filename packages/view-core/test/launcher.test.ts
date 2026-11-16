import { describe, expect, test } from "bun:test";

import { launcherHint } from "../src/launcher";

describe("launcherHint", () => {
  test("names only the parts the launcher knows", () => {
    expect(launcherHint({ name: "t3code" })).toBe("Started from t3code");
    expect(
      launcherHint({ branch: "feat/a", name: "t3code", worktreePath: "/wt/a" })
    ).toBe("Started from t3code · branch feat/a · worktree /wt/a");
  });
});
