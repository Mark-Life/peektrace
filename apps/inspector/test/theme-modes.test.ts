/** The toggle shows two states but stores three: "system" means no override. */
import { describe, expect, test } from "bun:test";
import { nextThemePress } from "../src/lib/theme-modes";

const press = (resolvedTheme?: string, systemTheme?: string) =>
  nextThemePress({ resolvedTheme, systemTheme });

describe("nextThemePress", () => {
  test("pins dark on a light OS", () => {
    expect(press("light", "light")).toEqual({
      store: "dark",
      appearance: "dark",
    });
  });

  test("drops the override when the flip lands on a light OS", () => {
    expect(press("dark", "light")).toEqual({
      store: "system",
      appearance: "light",
    });
  });

  test("pins light on a dark OS", () => {
    expect(press("dark", "dark")).toEqual({
      store: "light",
      appearance: "light",
    });
  });

  test("drops the override when the flip lands on a dark OS", () => {
    expect(press("light", "dark")).toEqual({
      store: "system",
      appearance: "dark",
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
      store: "system",
      appearance: "dark",
    });
    expect(press(undefined, undefined)).toEqual({
      store: "system",
      appearance: "light",
    });
  });
});
