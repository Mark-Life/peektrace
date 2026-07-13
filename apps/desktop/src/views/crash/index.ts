/// <reference lib="dom" />
/**
 * Crash view (browser context). Shown when the sidecar dies under a live window.
 * The "Restart server" button calls the Bun-side `restartServer` RPC; on success
 * Bun reloads this window to the inspector, so there is nothing more to do here.
 */
import Electrobun, { Electroview } from "electrobun/view";
import type { DesktopRPC } from "../../shared/rpc";

// maxRequestTime arms the timeout on the side that SENDS the request — here, the
// crash view calling restartServer. The default is 1s, but a restart (SIGTERM +
// up to 5s grace, then a fresh spawn awaiting the ready handshake) routinely
// takes longer, so a too-low timeout would reject a restart that is in fact
// succeeding. Size it well above the restart's worst case.
const rpc = Electroview.defineRPC<DesktopRPC>({
  maxRequestTime: 30_000,
  handlers: { requests: {}, messages: {} },
});
const electrobun = new Electrobun.Electroview({ rpc });

const button = document.getElementById("restart") as HTMLButtonElement | null;
const status = document.getElementById("status");

button?.addEventListener("click", async () => {
  if (!(button && status)) {
    return;
  }
  const client = electrobun.rpc;
  if (!client) {
    return;
  }
  button.disabled = true;
  status.textContent = "Restarting…";
  try {
    await client.request.restartServer({});
  } catch {
    button.disabled = false;
    status.textContent =
      "Restart failed — try quitting and reopening Peektrace.";
  }
});
