/** The toggle shows two states but stores three: "system" means no override. */
import { describe, expect, test } from "bun:test";

import { nextThemePress } from "../src/lib/theme-modes";

const press = (resolvedTheme?: string, systemTheme?: string) =>
  nextThemePress({ resolvedTheme, systemTheme });

describe("nextThemePress", () => {
  test("pins dark on a light OS", () => {
    expect(press("light", "light")).toEqual({
      appearance: "dark",
      store: "dark",
    });
  });

  test("drops the override when the flip lands on a light OS", () => {
    expect(press("dark", "light")).toEqual({
      appearance: "light",
      store: "system",
    });
  });

  test("pins light on a dark OS", () => {
    expect(press("dark", "dark")).toEqual({
      appearance: "light",
      store: "light",
    });
  });

  test("drops the override when the flip lands on a dark OS", () => {
    expect(press("light", "dark")).toEqual({
      appearance: "dark",
      store: "system",
    });
  });

  test("two presses from either default return to system", () => {
    for (const system of ["light", "dark"] as const) {
      const first = press(system, system);
      expect(first.store).toBe(system === "light" ? "dark" : "light");
      expect(press(first.appearance, system).store).toBe("system");
    }
  });

  test("unknown appearances fall back to system, announced as the OS look", () => {
    expect(press(undefined, "dark")).toEqual({
      appearance: "dark",
      store: "system",
    });
    expect(press(undefined, undefined)).toEqual({
      appearance: "light",
      store: "system",
    });
  });
});
