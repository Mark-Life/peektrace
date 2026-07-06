/** Theme provider + toggle, built on `next-themes` (shipped in ui). */
import { Button } from "@workspace/ui/components/button";
import { MonitorIcon, MoonIcon, SunIcon } from "lucide-react";
import { ThemeProvider as NextThemeProvider, useTheme } from "next-themes";
import { type ReactNode, useEffect, useState } from "react";

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

const ORDER = ["light", "dark", "system"] as const;
type ThemeMode = (typeof ORDER)[number];

const ICONS: Record<ThemeMode, typeof SunIcon> = {
  light: SunIcon,
  dark: MoonIcon,
  system: MonitorIcon,
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

/** Cycles light → dark → system, also bound to the `D` shortcut. */
export const ThemeToggle = () => {
  const { theme, setTheme } = useTheme();
  const [mounted, setMounted] = useState(false);

  useEffect(() => setMounted(true), []);

  const current = (mounted ? (theme as ThemeMode) : "system") ?? "system";
  const next: ThemeMode =
    ORDER[(ORDER.indexOf(current) + 1) % ORDER.length] ?? "system";

  useEffect(() => {
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
      setTheme(next);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [next, setTheme]);

  const Icon = ICONS[current];
  const label = `Switch to ${next} theme`;

  return (
    <Button
      aria-label={label}
      onClick={() => setTheme(next)}
      size="sm"
      variant="ghost"
    >
      <Icon />
      <span className="capitalize">{current}</span>
    </Button>
  );
};
