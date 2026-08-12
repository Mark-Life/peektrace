/** Root app: shell + hash-routed sections.
 *
 * Every section but the default one is code-split. `sessions` is what an empty
 * hash resolves to, so it stays in the entry chunk — splitting it would only
 * add a round trip to the first paint. The other three carry the bundle's
 * heaviest leaves (`memory` alone pulls `@tanstack/charts` through the vault
 * donut), and none of them is on the path to a first render.
 */
import { TooltipProvider } from "@workspace/ui/components/tooltip";
import { lazy, Suspense } from "react";
import { AppShell } from "./components/app-shell";
import { useRoute } from "./lib/routes";
import { SessionsRoute } from "./routes/sessions-route";

const CapabilitiesRoute = lazy(() =>
  import("./routes/capabilities-route").then((m) => ({
    default: m.CapabilitiesRoute,
  }))
);
const MemoryRoute = lazy(() =>
  import("./routes/memory-route").then((m) => ({ default: m.MemoryRoute }))
);
const SettingsRoute = lazy(() =>
  import("./routes/settings-route").then((m) => ({ default: m.SettingsRoute }))
);
const StatsRoute = lazy(() =>
  import("./routes/stats-route").then((m) => ({ default: m.StatsRoute }))
);

/** Toasts are a response to a mutation, never part of the first paint, and
 *  `sonner` is ~32 KB — so the host mounts itself once the entry has run. */
const Toaster = lazy(() =>
  import("@workspace/ui/components/sonner").then((m) => ({
    default: m.Toaster,
  }))
);

/** Holds the content region's height while a section chunk arrives. */
const RouteFallback = () => (
  <div aria-busy="true" className="h-64" role="status">
    <span className="sr-only">Loading section…</span>
  </div>
);

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
  if (route === "stats") {
    return <StatsRoute />;
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
      <Suspense fallback={<RouteFallback />}>
        <Screen />
      </Suspense>
    </AppShell>
    <Suspense fallback={null}>
      <Toaster position="bottom-right" richColors />
    </Suspense>
  </TooltipProvider>
);
