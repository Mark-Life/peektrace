/** Dark-first theme provider + toggle, built on `next-themes` (shipped in ui). */
import { Button } from "@workspace/ui/components/button";
import { MoonIcon, SunIcon } from "lucide-react";
import { ThemeProvider as NextThemeProvider, useTheme } from "next-themes";
import type { ReactNode } from "react";

/**
 * Wrap the app in `next-themes`. Forensic tool → default dark, but honor an
 * explicit user choice and persist it.
 */
export const ThemeProvider = ({
  children,
}: {
  readonly children: ReactNode;
}) => (
  <NextThemeProvider
    attribute="class"
    defaultTheme="dark"
    disableTransitionOnChange
    enableSystem
  >
    {children}
  </NextThemeProvider>
);

/** Light/dark toggle button for the sidebar footer. */
export const ThemeToggle = () => {
  const { resolvedTheme, setTheme } = useTheme();
  const isDark = resolvedTheme === "dark";
  return (
    <Button
      aria-label="Toggle theme"
      onClick={() => setTheme(isDark ? "light" : "dark")}
      size="sm"
      variant="ghost"
    >
      {isDark ? <SunIcon /> : <MoonIcon />}
      <span>{isDark ? "Light" : "Dark"}</span>
    </Button>
  );
};
