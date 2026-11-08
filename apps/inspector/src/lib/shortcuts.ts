/** Keyboard shortcuts: one table that the handlers, the help dialog and
 * `aria-keyshortcuts` all read, so the docs cannot drift from the bindings.
 *
 * `scope` says who handles a key: `global` keys go through `useShortcut` on
 * the window, `list` keys are handled by the session list while a row has
 * focus, and `builtin` keys belong to a ui primitive and are only documented.
 */
import { useEffect, useRef } from "react";

export interface Shortcut {
  readonly group: "General" | "Sessions" | "Transcript";
  readonly id: string;
  /** `KeyboardEvent.key` values, as `keyOf` normalizes them. `Mod+` means
   *  ⌘ on Mac and Ctrl elsewhere. */
  readonly keys: readonly string[];
  readonly label: string;
  readonly scope: "builtin" | "global" | "list";
}

export const SHORTCUTS = [
  {
    id: "listDown",
    keys: ["j", "ArrowDown"],
    label: "Next session",
    group: "Sessions",
    scope: "list",
  },
  {
    id: "listUp",
    keys: ["k", "ArrowUp"],
    label: "Previous session",
    group: "Sessions",
    scope: "list",
  },
  {
    id: "listFirst",
    keys: ["Home"],
    label: "First session",
    group: "Sessions",
    scope: "list",
  },
  {
    id: "listLast",
    keys: ["End"],
    label: "Last session",
    group: "Sessions",
    scope: "list",
  },
  {
    id: "listOpen",
    keys: ["Enter"],
    label: "Open the focused session",
    group: "Sessions",
    scope: "builtin",
  },
  {
    id: "search",
    keys: ["/"],
    label: "Focus search",
    group: "Sessions",
    scope: "global",
  },
  {
    id: "prevTurn",
    keys: ["["],
    label: "Previous turn",
    group: "Transcript",
    scope: "global",
  },
  {
    id: "nextTurn",
    keys: ["]"],
    label: "Next turn",
    group: "Transcript",
    scope: "global",
  },
  {
    id: "theme",
    keys: ["d"],
    label: "Switch light / dark theme",
    group: "General",
    scope: "global",
  },
  {
    id: "sidebar",
    keys: ["Mod+b"],
    label: "Collapse or expand the sidebar",
    group: "General",
    scope: "builtin",
  },
  {
    id: "help",
    keys: ["?"],
    label: "Show keyboard shortcuts",
    group: "General",
    scope: "global",
  },
] as const satisfies readonly Shortcut[];

export type ShortcutId = (typeof SHORTCUTS)[number]["id"];

export const shortcutOf = (id: ShortcutId): Shortcut => {
  const found = SHORTCUTS.find((s) => s.id === id);
  if (!found) {
    throw new Error(`unknown shortcut ${id}`);
  }
  return found;
};

const KEY_LABEL: Record<string, string> = {
  ArrowDown: "↓",
  ArrowUp: "↑",
};

/** How one key reads on a keycap, e.g. `Mod+b` as `⌘B` or `Ctrl+B`. */
export const keyLabel = ({ key, mac }: { key: string; mac: boolean }) => {
  const mod = key.startsWith("Mod+");
  const base = mod ? key.slice("Mod+".length) : key;
  const shown =
    KEY_LABEL[base] ?? (base.length === 1 ? base.toUpperCase() : base);
  if (!mod) {
    return shown;
  }
  return mac ? `⌘${shown}` : `Ctrl+${shown}`;
};

/** The `aria-keyshortcuts` value for a shortcut. */
export const ariaKeys = (id: ShortcutId) =>
  shortcutOf(id)
    .keys.map((k) => k.replace("Mod+", "Control+"))
    .join(" ");

const APPLE = /Mac|iPhone|iPad/;

export const isMacPlatform = () =>
  typeof navigator !== "undefined" &&
  APPLE.test(navigator.platform || navigator.userAgent);

/** The bits of a key event the helpers read; plain objects work in tests. */
export interface KeyEventLike {
  readonly altKey: boolean;
  readonly code: string;
  readonly ctrlKey: boolean;
  readonly defaultPrevented: boolean;
  readonly getModifierState?: (key: string) => boolean;
  readonly key: string;
  readonly metaKey: boolean;
  readonly shiftKey: boolean;
  readonly target: unknown;
}

/** Physical keys read when the layout types a non-Latin character. */
const CODE_KEY: Record<string, readonly [string, string]> = {
  BracketLeft: ["[", "{"],
  BracketRight: ["]", "}"],
  Slash: ["/", "?"],
};

const ASCII = /^[ -~]$/;

/** The key as the table names it. Single letters are lowercased; a non-ASCII
 *  character (a Cyrillic layout, say) falls back to the physical key. */
export const keyOf = (ev: Pick<KeyEventLike, "code" | "key" | "shiftKey">) => {
  if (ev.key.length !== 1) {
    return ev.key;
  }
  if (ASCII.test(ev.key)) {
    return ev.key.toLowerCase();
  }
  if (ev.code.startsWith("Key") && ev.code.length === "KeyA".length) {
    return ev.code.slice("Key".length).toLowerCase();
  }
  const pair = CODE_KEY[ev.code];
  if (pair) {
    return ev.shiftKey ? pair[1] : pair[0];
  }
  return ev.key;
};

/** An element as far as the guards need it. */
export interface ElementLike {
  readonly closest?: (selector: string) => unknown;
  readonly isContentEditable?: boolean;
  readonly tagName?: string;
}

const asElement = (target: unknown): ElementLike | null =>
  typeof target === "object" && target !== null && "tagName" in target
    ? (target as ElementLike)
    : null;

const TYPING_TAGS = new Set(["INPUT", "TEXTAREA", "SELECT"]);

/** True when focus is inside a field where typing should win over shortcuts. */
export const isTypingTarget = (target: unknown) => {
  const el = asElement(target);
  return (
    el !== null &&
    (TYPING_TAGS.has(el.tagName ?? "") || el.isContentEditable === true)
  );
};

/** Popups that own their letter keys (typeahead, menus, the help dialog). */
const KEY_OWNERS =
  "[role=dialog],[role=alertdialog],[role=listbox],[role=menu]";

/** Whether a plain-key shortcut may run for this event. */
export const shortcutAllowed = (ev: KeyEventLike) => {
  const altGraph = ev.getModifierState?.("AltGraph") === true;
  if (ev.metaKey || ((ev.ctrlKey || ev.altKey) && !altGraph)) {
    return false;
  }
  if (ev.defaultPrevented || isTypingTarget(ev.target)) {
    return false;
  }
  const el = asElement(ev.target);
  return !el?.closest?.(KEY_OWNERS);
};

/** Whether `ev` presses one of the shortcut's keys. */
export const matchesShortcut = (id: ShortcutId, ev: KeyEventLike) =>
  (shortcutOf(id).keys as readonly string[]).includes(keyOf(ev));

/**
 * Run `handler` when the shortcut's key is pressed anywhere, unless typing or
 * a popup owns the key. The default action is cancelled, so `/` does not type
 * a slash or open the browser's quick find. The handler is read through a ref,
 * so a new closure on every render does not re-register the listener.
 */
export const useShortcut = (
  id: ShortcutId,
  handler: (ev: KeyboardEvent) => void,
  enabled = true
) => {
  const latest = useRef(handler);
  latest.current = handler;
  useEffect(() => {
    if (!enabled) {
      return;
    }
    const onKeyDown = (ev: KeyboardEvent) => {
      if (!(shortcutAllowed(ev) && matchesShortcut(id, ev))) {
        return;
      }
      ev.preventDefault();
      latest.current(ev);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [id, enabled]);
};
