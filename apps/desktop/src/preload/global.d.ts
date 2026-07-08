import type { PeektraceBridge } from "./index";

declare global {
  interface Window {
    /** contextBridge API exposed by the preload (see src/preload/index.ts). */
    readonly peektrace: PeektraceBridge;
  }
}
