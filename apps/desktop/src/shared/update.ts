// Discriminated union describing the desktop auto-update lifecycle. The main
// process drives a native dialog from this ("Restart to update"), and the shape
// is load-bearing for the pure decision helpers in ../main/updater-state.ts and
// their tests. The MVP has no renderer bundle (inline `data:` screens only), so
// nothing is broadcast — but the union is kept so a future renderer card can
// wire in without reshaping the state.

export type DesktopUpdateStatus =
  | { readonly state: "idle" }
  | { readonly state: "available"; readonly version: string }
  | {
      readonly state: "downloading";
      readonly version: string;
      readonly percent: number;
    }
  | { readonly state: "downloaded"; readonly version: string }
  | {
      readonly state: "error";
      readonly version: string;
      readonly message: string;
    }
  | { readonly state: "installing"; readonly version: string };

// IPC channels for a future renderer card are intentionally omitted in the MVP
// (nothing to push to). When a renderer is added, prefix them "peephole:":
//   export const UPDATE_STATUS_CHANNEL = "peephole:updates:status" as const;
//   export const UPDATE_STATUS_GET_CHANNEL = "peephole:updates:status:get" as const;
//   export const UPDATE_INSTALL_CHANNEL = "peephole:updates:quit-and-install" as const;
