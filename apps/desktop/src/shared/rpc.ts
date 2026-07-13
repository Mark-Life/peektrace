import type { RPCSchema } from "electrobun/bun";

/**
 * RPC contract between the Bun main process and the bundled crash view.
 *
 * The inspector UI (served over http by the sidecar) does not use RPC — the only
 * interactive local surface is the crash screen, whose "Restart server" button
 * calls the `restartServer` request that runs in Bun.
 */
export interface DesktopRPC {
  bun: RPCSchema<{
    requests: {
      restartServer: {
        params: Record<string, never>;
        response: { ok: boolean };
      };
    };
    messages: Record<string, never>;
  }>;
  webview: RPCSchema<{
    requests: Record<string, never>;
    messages: Record<string, never>;
  }>;
}
