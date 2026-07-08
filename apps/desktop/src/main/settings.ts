import Store from "electron-store";
import {
  DEFAULT_SERVER_SETTINGS,
  type DesktopServerSettings,
} from "../shared/server-settings";

interface PersistedShape {
  readonly server: DesktopServerSettings;
}

// PEEPHOLE_DESKTOP_SETTINGS_DIR overrides the store location — an e2e seam so a
// test run gets an isolated settings.json instead of the shared userData dir.
const store = new Store<PersistedShape>({
  name: "settings",
  ...(process.env.PEEPHOLE_DESKTOP_SETTINGS_DIR
    ? { cwd: process.env.PEEPHOLE_DESKTOP_SETTINGS_DIR }
    : {}),
  defaults: { server: DEFAULT_SERVER_SETTINGS },
});

// Backfill if an older settings.json predates the server section.
if (!store.has("server")) {
  store.set("server", DEFAULT_SERVER_SETTINGS);
}

/** Read the persisted server settings, defensively defaulting a missing port. */
export const getServerSettings = (): DesktopServerSettings => ({
  port: store.get("server")?.port ?? DEFAULT_SERVER_SETTINGS.port,
});

/** Patch the server settings and return the merged, persisted result. */
export const updateServerSettings = (
  patch: Partial<DesktopServerSettings>
): DesktopServerSettings => {
  const next: DesktopServerSettings = {
    port: patch.port ?? getServerSettings().port,
  };
  store.set("server", next);
  return next;
};
