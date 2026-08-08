/** Root app: shell + hash-routed sections. */
import { Toaster } from "@workspace/ui/components/sonner";
import { TooltipProvider } from "@workspace/ui/components/tooltip";
import { AppShell } from "./components/app-shell";
import { useRoute } from "./lib/routes";
import { CapabilitiesRoute } from "./routes/capabilities-route";
import { MemoryRoute } from "./routes/memory-route";
import { SessionsRoute } from "./routes/sessions-route";
import { SettingsRoute } from "./routes/settings-route";

/** Resolve the active section to its screen. */
const Screen = () => {
  const route = useRoute();
  if (route === "capabilities") {
    return <CapabilitiesRoute />;
  }
  if (route === "sessions") {
    return <SessionsRoute />;
  }
  if (route === "settings") {
    return <SettingsRoute />;
  }
  return <MemoryRoute />;
};

/** The inspector application root.
 *
 * Deliberately subscribes to nothing: the filesystem watch poll lives in the
 * `WatchStatus` leaf inside the shell, so a disk change never re-renders the
 * whole tree. */
export const App = () => (
  <TooltipProvider delayDuration={150}>
    <AppShell>
      <Screen />
    </AppShell>
    <Toaster position="bottom-right" richColors />
  </TooltipProvider>
);
