/** Theme provider + toggle, built on `next-themes` (shipped in ui). */
import { Button } from "@workspace/ui/components/button";
import { Kbd } from "@workspace/ui/components/kbd";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@workspace/ui/components/tooltip";
import { MoonIcon, SunIcon } from "lucide-react";
import { ThemeProvider as NextThemeProvider, useTheme } from "next-themes";
import type { ReactNode } from "react";
import { ariaKeys, keyLabel, shortcutOf, useShortcut } from "./shortcuts";
import { type Appearance, nextThemePress } from "./theme-modes";

/**
 * Wrap the app in `next-themes`. Default to the OS preference, but honor an
 * explicit user choice and persist it.
 */
export const ThemeProvider = ({
  children,
}: {
  readonly children: ReactNode;
}) => (
  <NextThemeProvider
    attribute="class"
    defaultTheme="system"
    disableTransitionOnChange
    enableSystem
  >
    {children}
  </NextThemeProvider>
);

const ICONS: Record<Appearance, typeof SunIcon> = {
  light: SunIcon,
  dark: MoonIcon,
};

/**
 * Flips light <-> dark from what is on screen, storing "system" when the flip
 * lands on the OS appearance. Also bound to the `theme` shortcut.
 */
export const ThemeToggle = ({
  iconOnly = false,
}: {
  /** Drop the label: for narrow rails. */
  readonly iconOnly?: boolean;
}) => {
  // Client-only app: next-themes reads storage and the OS in its initial state,
  // so both appearances are already known on the first render.
  const { resolvedTheme, systemTheme, setTheme } = useTheme();
  const { appearance, store } = nextThemePress({ resolvedTheme, systemTheme });

  useShortcut("theme", () => setTheme(store));

  // Icon and label name the appearance the next press produces.
  const Icon = ICONS[appearance];
  const label = `Switch to ${appearance} theme`;

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          aria-keyshortcuts={ariaKeys("theme")}
          aria-label={label}
          onClick={() => setTheme(store)}
          size={iconOnly ? "icon-sm" : "sm"}
          variant="ghost"
        >
          <Icon />
          {iconOnly ? null : <span className="capitalize">{appearance}</span>}
        </Button>
      </TooltipTrigger>
      <TooltipContent className="flex items-center gap-1.5" side="right">
        {label}
        {shortcutOf("theme").keys.map((key) => (
          <Kbd key={key}>{keyLabel({ key, mac: false })}</Kbd>
        ))}
      </TooltipContent>
    </Tooltip>
  );
};
