/** Root app: shell + hash-routed sections. */
import { Toaster } from "@workspace/ui/components/sonner";
import { TooltipProvider } from "@workspace/ui/components/tooltip";
import { AppShell } from "./components/app-shell";
import { useRoute } from "./lib/routes";
import { useWatchRefresh } from "./lib/watch-atoms";
import { CapabilitiesRoute } from "./routes/capabilities-route";
import { MemoryRoute } from "./routes/memory-route";
import { SessionsRoute } from "./routes/sessions-route";

/** Resolve the active section to its screen. */
const Screen = () => {
  const route = useRoute();
  if (route === "capabilities") {
    return <CapabilitiesRoute />;
  }
  if (route === "sessions") {
    return <SessionsRoute />;
  }
  return <MemoryRoute />;
};

/** The inspector application root. */
export const App = () => {
  useWatchRefresh();
  return (
    <TooltipProvider delayDuration={150}>
      <AppShell>
        <Screen />
      </AppShell>
      <Toaster position="bottom-right" richColors />
    </TooltipProvider>
  );
};
