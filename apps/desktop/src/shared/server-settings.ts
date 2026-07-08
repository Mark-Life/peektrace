/**
 * Persistent desktop sidecar settings, consumed by the main process and exposed
 * to the renderer over IPC. The shape lives in `src/shared/` so both sides agree
 * on it. Auth is not a setting: the MVP sidecar binds loopback-only with no
 * token, so there is nothing to persist here beyond the port.
 */

export interface DesktopServerSettings {
  /** TCP port the sidecar prefers to bind. */
  readonly port: number;
}

export const DEFAULT_SERVER_SETTINGS: DesktopServerSettings = {
  port: 4321,
};
