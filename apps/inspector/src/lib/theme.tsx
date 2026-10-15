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
import { type ReactNode, useEffect, useState } from "react";
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

/** Returns true when focus is inside a field where typing should win over shortcuts. */
const isTypingTarget = (target: EventTarget | null) => {
  if (!(target instanceof HTMLElement)) {
    return false;
  }
  const tag = target.tagName;
  return (
    tag === "INPUT" ||
    tag === "TEXTAREA" ||
    tag === "SELECT" ||
    target.isContentEditable
  );
};

/**
 * Flips light <-> dark from what is on screen, storing "system" when the flip
 * lands on the OS appearance. Also bound to the `D` shortcut.
 */
export const ThemeToggle = ({
  iconOnly = false,
}: {
  /** Drop the label: for narrow rails. */
  readonly iconOnly?: boolean;
}) => {
  const { resolvedTheme, systemTheme, setTheme } = useTheme();
  const [mounted, setMounted] = useState(false);

  useEffect(() => setMounted(true), []);

  // Appearances are unknown until mount; a blank frame beats a wrong label.
  const press = mounted ? nextThemePress({ resolvedTheme, systemTheme }) : null;
  const store = press?.store;

  useEffect(() => {
    if (!store) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (
        event.key.toLowerCase() !== "d" ||
        event.metaKey ||
        event.ctrlKey ||
        event.altKey ||
        isTypingTarget(event.target)
      ) {
        return;
      }
      setTheme(store);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [store, setTheme]);

  // Icon and label name the appearance the next press produces.
  const Icon = press ? ICONS[press.appearance] : null;
  const label = press ? `Switch to ${press.appearance} theme` : "Switch theme";

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          aria-label={label}
          disabled={!store}
          onClick={() => store && setTheme(store)}
          size={iconOnly ? "icon-sm" : "sm"}
          variant="ghost"
        >
          {Icon ? <Icon /> : <span className="size-4" />}
          {iconOnly || !press ? null : (
            <span className="capitalize">{press.appearance}</span>
          )}
        </Button>
      </TooltipTrigger>
      <TooltipContent className="flex items-center gap-1.5" side="right">
        {label}
        <Kbd>D</Kbd>
      </TooltipContent>
    </Tooltip>
  );
};
