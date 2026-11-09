/** Theme provider + toggle, built on `next-themes` (shipped in ui). */
import { Button } from "@workspace/ui/components/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@workspace/ui/components/tooltip";
import { MoonIcon, SunIcon } from "lucide-react";
import { ThemeProvider as NextThemeProvider, useTheme } from "next-themes";
import type { ReactNode } from "react";
import { ShortcutKeys } from "./shortcut-keys";
import { ariaKeys, useShortcut } from "./shortcuts";
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

/** What the next flip shows, and a `flip` that applies it. The flip goes
 *  light <-> dark from what is on screen, storing "system" when it lands on
 *  the OS appearance. */
const useThemeFlip = () => {
  // Client-only app: next-themes reads storage and the OS in its initial state,
  // so both appearances are already known on the first render.
  const { resolvedTheme, systemTheme, setTheme } = useTheme();
  const { appearance, store } = nextThemePress({ resolvedTheme, systemTheme });
  return { appearance, flip: () => setTheme(store) };
};

/** Binds the `theme` shortcut. Call it from a component that is always
 *  mounted: the toggle button unmounts with the mobile sidebar sheet. */
export const useThemeShortcut = () => {
  const { flip } = useThemeFlip();
  useShortcut("theme", flip);
};

/** The light / dark button. */
export const ThemeToggle = ({
  iconOnly = false,
}: {
  /** Drop the label: for narrow rails. */
  readonly iconOnly?: boolean;
}) => {
  const { appearance, flip } = useThemeFlip();
  // Icon and label name the appearance the next press produces.
  const Icon = ICONS[appearance];
  const label = `Switch to ${appearance} theme`;

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          aria-keyshortcuts={ariaKeys("theme")}
          aria-label={label}
          onClick={flip}
          size={iconOnly ? "icon-sm" : "sm"}
          variant="ghost"
        >
          <Icon />
          {iconOnly ? null : <span className="capitalize">{appearance}</span>}
        </Button>
      </TooltipTrigger>
      <TooltipContent className="flex items-center gap-1.5" side="right">
        {label}
        <ShortcutKeys id="theme" />
      </TooltipContent>
    </Tooltip>
  );
};
