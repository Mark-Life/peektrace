/** Pure rules for the two-state theme toggle; no React, no DOM. */

/** Stored values. `"system"` is the absence of an override, not a third look. */
export type ThemeMode = "system" | Appearance;

/** The two appearances the toggle flips between. */
export type Appearance = "light" | "dark";

const isAppearance = (value: string | undefined): value is Appearance =>
  value === "light" || value === "dark";

export interface ThemePress {
  /** Appearance the press produces: what the icon and label announce. */
  readonly appearance: Appearance;
  /** Value to store; `"system"` keeps following the OS. */
  readonly store: ThemeMode;
}

/**
 * What the next toggle press does, from the appearance on screen rather than
 * the stored value.
 *
 * Flipping onto the appearance the OS already asks for stores `"system"`
 * instead of pinning a copy of it, so two presses never lock the user out of
 * following their OS. Evaluated only on a press: an OS change under a stored
 * override leaves that override alone.
 */
export const nextThemePress = ({
  resolvedTheme,
  systemTheme,
}: {
  readonly resolvedTheme: string | undefined;
  readonly systemTheme: string | undefined;
}): ThemePress => {
  const system: Appearance = isAppearance(systemTheme) ? systemTheme : "light";
  if (!isAppearance(resolvedTheme)) {
    return { store: "system", appearance: system };
  }
  const target: Appearance = resolvedTheme === "dark" ? "light" : "dark";
  return { store: target === system ? "system" : target, appearance: target };
};
