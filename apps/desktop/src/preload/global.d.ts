import type { PeepholeBridge } from "./index";

declare global {
  interface Window {
    /** contextBridge API exposed by the preload (see src/preload/index.ts). */
    readonly peephole: PeepholeBridge;
  }
}
