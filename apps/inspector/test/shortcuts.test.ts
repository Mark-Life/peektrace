/** The shortcut table and the guards every handler goes through. */

import { describe, expect, test } from "bun:test";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import {
  ariaKeys,
  isTypingTarget,
  type KeyEventLike,
  keyLabel,
  keyOf,
  matchesShortcut,
  SHORTCUTS,
  shortcutAllowed,
} from "../src/lib/shortcuts";

const press = (over: Partial<KeyEventLike> = {}): KeyEventLike => ({
  altKey: false,
  code: "KeyJ",
  ctrlKey: false,
  defaultPrevented: false,
  key: "j",
  metaKey: false,
  shiftKey: false,
  target: { tagName: "BODY", closest: () => null },
  ...over,
});

describe("keyOf", () => {
  test("Latin keys read as typed, letters lowercased", () => {
    expect(keyOf(press())).toBe("j");
    expect(keyOf(press({ key: "D", code: "KeyD", shiftKey: true }))).toBe("d");
    expect(keyOf(press({ key: "ArrowDown", code: "ArrowDown" }))).toBe(
      "ArrowDown"
    );
  });

  test("a Cyrillic layout falls back to the physical key", () => {
    expect(keyOf(press({ key: "о", code: "KeyJ" }))).toBe("j");
    expect(keyOf(press({ key: "в", code: "KeyD" }))).toBe("d");
    expect(keyOf(press({ key: "х", code: "BracketLeft" }))).toBe("[");
    expect(keyOf(press({ key: "ъ", code: "BracketRight" }))).toBe("]");
  });

  test("? is the shifted slash", () => {
    expect(keyOf(press({ key: "?", code: "Slash", shiftKey: true }))).toBe("?");
    expect(matchesShortcut("help", press({ key: "?", code: "Slash" }))).toBe(
      true
    );
    expect(matchesShortcut("search", press({ key: "/", code: "Slash" }))).toBe(
      true
    );
  });
});

describe("shortcutAllowed", () => {
  test("a plain key on the page runs", () => {
    expect(shortcutAllowed(press())).toBe(true);
    expect(shortcutAllowed(press({ shiftKey: true, key: "?" }))).toBe(true);
  });

  test("modifier chords belong to the browser and the OS", () => {
    expect(shortcutAllowed(press({ metaKey: true }))).toBe(false);
    expect(shortcutAllowed(press({ ctrlKey: true }))).toBe(false);
    expect(shortcutAllowed(press({ altKey: true }))).toBe(false);
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

  test("a handled event, a field, or a popup owns the key", () => {
    expect(shortcutAllowed(press({ defaultPrevented: true }))).toBe(false);
    for (const tagName of ["INPUT", "TEXTAREA", "SELECT"]) {
      expect(shortcutAllowed(press({ target: { tagName } }))).toBe(false);
    }
    expect(
      shortcutAllowed(
        press({ target: { tagName: "DIV", isContentEditable: true } })
      )
    ).toBe(false);
    const inPopup = {
      tagName: "DIV",
      closest: (s: string) => s.includes("listbox"),
    };
    expect(shortcutAllowed(press({ target: inPopup }))).toBe(false);
  });

  test("isTypingTarget ignores non-elements", () => {
    expect(isTypingTarget(null)).toBe(false);
    expect(isTypingTarget({ addEventListener: () => undefined })).toBe(false);
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
    return TS_FILE.test(d.name) ? [readFileSync(path, "utf8")] : [];
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
