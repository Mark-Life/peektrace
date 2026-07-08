import { contextBridge, ipcRenderer } from "electron";
import type { DesktopServerSettings } from "../shared/server-settings";

// Channel strings must byte-match the ipcMain.handle keys in src/main/index.ts,
// or the invoke silently rejects.
const api = {
  /** Open an http(s) URL in the user's default browser (scheme-checked in main). */
  openExternal(url: string): Promise<void> {
    return ipcRenderer.invoke("peephole:shell:open-external", url);
  },
  /** Read the persisted server settings (currently just the port). */
  getSettings(): Promise<DesktopServerSettings> {
    return ipcRenderer.invoke("peephole:settings:get");
  },
  /** Patch one or more server settings. Returns the new full settings. */
  updateSettings(
    patch: Partial<DesktopServerSettings>
  ): Promise<DesktopServerSettings> {
    return ipcRenderer.invoke("peephole:settings:update", patch);
  },
  /** Restart the sidecar; main reloads the window on success. */
  restartServer(): Promise<void> {
    return ipcRenderer.invoke("peephole:server:restart");
  },
} as const;

contextBridge.exposeInMainWorld("peephole", api);

export type PeepholeBridge = typeof api;
