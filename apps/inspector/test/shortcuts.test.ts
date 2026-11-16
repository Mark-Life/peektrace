/** The shortcut table and the guards every handler goes through. */

import { describe, expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

import {
  ariaKeys,
  isTypingTarget,
  keyLabel,
  keyOf,
  matchesShortcut,
  SHORTCUTS,
  shortcutAllowed,
} from "../src/lib/shortcuts";
import type { KeyEventLike } from "../src/lib/shortcuts";

const press = (over: Partial<KeyEventLike> = {}): KeyEventLike => ({
  altKey: false,
  code: "KeyJ",
  ctrlKey: false,
  defaultPrevented: false,
  key: "j",
  metaKey: false,
  shiftKey: false,
  target: { closest: () => null, tagName: "BODY" },
  ...over,
});

describe("keyOf", () => {
  test("Latin keys read as typed, letters lowercased", () => {
    expect(keyOf(press())).toBe("j");
    expect(keyOf(press({ code: "KeyD", key: "D", shiftKey: true }))).toBe("d");
    expect(keyOf(press({ code: "ArrowDown", key: "ArrowDown" }))).toBe(
      "ArrowDown"
    );
  });

  test("a Cyrillic layout falls back to the physical key", () => {
    expect(keyOf(press({ code: "KeyJ", key: "о" }))).toBe("j");
    expect(keyOf(press({ code: "KeyD", key: "в" }))).toBe("d");
    expect(keyOf(press({ code: "BracketLeft", key: "х" }))).toBe("[");
    expect(keyOf(press({ code: "BracketRight", key: "ъ" }))).toBe("]");
  });

  test("? is the shifted slash", () => {
    expect(keyOf(press({ code: "Slash", key: "?", shiftKey: true }))).toBe("?");
    expect(matchesShortcut("help", press({ code: "Slash", key: "?" }))).toBe(
      true
    );
    expect(matchesShortcut("search", press({ code: "Slash", key: "/" }))).toBe(
      true
    );
  });
});

describe("layouts that type other characters on the bound keys", () => {
  test("Russian: the Slash key types . and , but still reads / and ?", () => {
    expect(keyOf(press({ code: "Slash", key: "." }))).toBe("/");
    expect(keyOf(press({ code: "Slash", key: ",", shiftKey: true }))).toBe("?");
    expect(matchesShortcut("search", press({ code: "Slash", key: "." }))).toBe(
      true
    );
  });

  test("German: BracketRight types + but reads ]", () => {
    expect(keyOf(press({ code: "BracketRight", key: "+" }))).toBe("]");
  });

  test("a bound character typed elsewhere wins over the physical key", () => {
    expect(keyOf(press({ code: "Digit6", key: "]" }))).toBe("]");
    expect(keyOf(press({ code: "Digit7", key: "/", shiftKey: true }))).toBe(
      "/"
    );
  });

  test("Dvorak: a letter on the Slash key stays a letter", () => {
    expect(keyOf(press({ code: "Slash", key: "z" }))).toBe("z");
    expect(keyOf(press({ code: "Slash", key: "Z", shiftKey: true }))).toBe("z");
    expect(matchesShortcut("search", press({ code: "Slash", key: "z" }))).toBe(
      false
    );
    expect(matchesShortcut("help", press({ code: "Slash", key: "Z" }))).toBe(
      false
    );
  });

  test("an unbound character off the remapped keys reads as typed", () => {
    expect(keyOf(press({ code: "Period", key: "." }))).toBe(".");
  });
});

describe("shortcutAllowed", () => {
  test("a plain key on the page runs", () => {
    expect(shortcutAllowed(press())).toBe(true);
    expect(shortcutAllowed(press({ key: "?", shiftKey: true }))).toBe(true);
  });

  test("modifier chords belong to the browser and the OS", () => {
    // Off a Mac: there, Option that types a bound key is allowed (below).
    expect(shortcutAllowed(press({ metaKey: true }), false)).toBe(false);
    expect(shortcutAllowed(press({ ctrlKey: true }), false)).toBe(false);
    expect(shortcutAllowed(press({ altKey: true }), false)).toBe(false);
  });

  test("AltGr, which some layouts need for [ and ], still runs", () => {
    expect(
      shortcutAllowed(
        press({
          altKey: true,
          ctrlKey: true,
          getModifierState: (k) => k === "AltGraph",
          key: "[",
        })
      )
    ).toBe(true);
  });

  test("Mac Option that types a bound key runs, other Option chords do not", () => {
    const option = (key: string, code: string) =>
      press({ altKey: true, code, key });

    expect(shortcutAllowed(option("]", "Digit6"), true)).toBe(true);
    expect(shortcutAllowed(option("[", "Digit5"), true)).toBe(true);
    expect(shortcutAllowed(option("∂", "KeyD"), true)).toBe(false);
    expect(shortcutAllowed(option("]", "Digit6"), false)).toBe(false);
    expect(
      shortcutAllowed(
        press({ altKey: true, code: "Digit6", ctrlKey: true, key: "]" }),
        true
      )
    ).toBe(false);
  });

  test("a handled event, a field, or a popup owns the key", () => {
    expect(shortcutAllowed(press({ defaultPrevented: true }))).toBe(false);

    for (const tagName of ["INPUT", "TEXTAREA", "SELECT"]) {
      expect(shortcutAllowed(press({ target: { tagName } }))).toBe(false);
    }

    expect(
      shortcutAllowed(
        press({ target: { isContentEditable: true, tagName: "DIV" } })
      )
    ).toBe(false);

    const inPopup = {
      closest: (s: string) => s.includes("listbox"),
      tagName: "DIV",
    };

    expect(shortcutAllowed(press({ target: inPopup }))).toBe(false);
  });

  test("isTypingTarget ignores non-elements", () => {
    expect(isTypingTarget(null)).toBe(false);
    expect(isTypingTarget({ addEventListener: () => {} })).toBe(false);
    expect(isTypingTarget({ tagName: "BUTTON" })).toBe(false);
  });
});

describe("labels", () => {
  test("Mod reads as ⌘ on Mac and Ctrl elsewhere", () => {
    expect(keyLabel({ key: "Mod+b", mac: true })).toBe("⌘B");
    expect(keyLabel({ key: "Mod+b", mac: false })).toBe("Ctrl+B");
    expect(keyLabel({ key: "ArrowUp", mac: false })).toBe("↑");
    expect(keyLabel({ key: "d", mac: false })).toBe("D");
  });

  test("aria-keyshortcuts lists every key", () => {
    expect(ariaKeys("listDown")).toBe("j ArrowDown");
    expect(ariaKeys("sidebar")).toBe("Control+b");
  });
});

const TS_FILE = /\.tsx?$/;

const sources = (dir: string): string[] =>
  readdirSync(dir, { withFileTypes: true }).flatMap((d) => {
    const path = join(dir, d.name);

    if (d.isDirectory()) {
      return sources(path);
    }

    return TS_FILE.test(d.name) ? [readFileSync(path, "utf-8")] : [];
  });

test("every shortcut the app handles has a handler that names it", () => {
  const src = sources(join(import.meta.dir, "../src")).join("\n");
  const ids = SHORTCUTS.map((s) => s.id);
  expect(new Set(ids).size).toBe(ids.length);
  const reader = { builtin: null, global: "useShortcut", list: "has" };

  const missing = SHORTCUTS.filter((s) => {
    const call = reader[s.scope];

    return call !== null && !src.includes(`${call}("${s.id}"`);
  }).map((s) => s.id);

  expect(missing).toEqual([]);
});
