import { describe, expect, test } from "bun:test";
import { resolveWork } from "./parity-lib";

const REPO = "/r/repo";
const TMP = "/t";
const work = (...args: string[]) =>
  resolveWork(["bun", "parity.ts", ...args], REPO, TMP);

describe("resolveWork", () => {
  test("defaults under .scratch", () => {
    expect(work()).toEqual({ dir: "/r/repo/.scratch/parity" });
  });

  test("accepts dirs under .scratch or the temp dir", () => {
    expect(work("--work", ".scratch/p2")).toEqual({
      dir: "/r/repo/.scratch/p2",
    });
    expect(work("--work", "/t/p2")).toEqual({ dir: "/t/p2" });
  });

  test("refuses a missing value", () => {
    expect(work("--work")).toEqual({ error: "Missing value for --work." });
    expect(work("--work", "")).toEqual({ error: "Missing value for --work." });
    expect(work("--work", "--real-only")).toEqual({
      error: "Missing value for --work.",
    });
  });

  test("refuses the repo, its parents and dirs outside the safe roots", () => {
    for (const value of [".", "..", "/", ".scratch", "src", "/r", "/t"]) {
      expect(work("--work", value)).toHaveProperty("error");
    }
    expect(resolveWork(["--work", "/t/x"], "/t/x/repo", TMP)).toHaveProperty(
      "error"
    );
  });
});
