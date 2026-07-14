/**
 * Peektrace desktop shell — Electrobun main (Bun) process.
 *
 * Boots a native-WebView window, spawns the `peektrace` CLI as a loopback sidecar,
 * and points the window at the inspector the sidecar serves. Owns the app menu,
 * auto-update entry point, window-state persistence, crash recovery, and clean
 * sidecar teardown on quit.
 */
import type { ChildProcess } from "node:child_process";
import Electrobun, { BrowserView, BrowserWindow, Utils } from "electrobun/bun";
import type { DesktopRPC } from "../shared/rpc";
import { logFilePath, scoped } from "./log";
import { installApplicationMenu } from "./menu";
import { startupHtml } from "./screens";
import {
  killSidecarSync,
  onUnexpectedSidecarExit,
  type SidecarConnection,
  SidecarPortInUseError,
  startSidecar,
  stopSidecar,
} from "./sidecar";
import { checkForUpdates } from "./updater";
import {
  loadWindowState,
  saveWindowState,
  type WindowFrame,
} from "./window-state";

const log = scoped("main");

const DOCS_URL = "https://github.com/Mark-Life/peektrace";
const CRASH_VIEW_URL = "views://crash/index.html";
const FATAL_DETAIL_MAX = 1800;
const isMac = process.platform === "darwin";
// Marks the inspector URL when the native titlebar overlays the web content.
const INSET_TITLEBAR = "inset";

let mainWindow: BrowserWindow | null = null;
let connection: SidecarConnection | null = null;
let quitting = false;

// Bun-side RPC handlers for the crash view. The request timeout that matters
// lives on the crash view (the side that sends restartServer); this side only
// handles incoming requests, so no maxRequestTime is needed here.
const rpc = BrowserView.defineRPC<DesktopRPC>({
  handlers: {
    requests: {
      restartServer: async () => {
        await restartSidecarAndReload();
        return { ok: true };
      },
    },
    messages: {},
  },
});

/** Create the main window, restoring its last frame, showing the startup screen. */
const createMainWindow = (): BrowserWindow => {
  const frame = loadWindowState();
  const window = new BrowserWindow({
    title: "Peektrace",
    frame,
    html: startupHtml(),
    rpc,
    ...(isMac
      ? {
          titleBarStyle: "hiddenInset" as const,
          // Placed in the titlebar row the web UI keeps clear (see INSET_TITLEBAR):
          // y centers the 12px controls in that 32px row.
          trafficLightOffset: { x: 6, y: 10 },
        }
      : { titleBarStyle: "default" as const }),
  });
  mainWindow = window;

  // getFrame() is unusable in the close handler: electrobun removes the native
  // window before that handler runs, so it returns zeros. Track the frame live
  // on move/resize instead and persist the last good value on close.
  let lastFrame: WindowFrame = frame;
  const captureFrame = () => {
    try {
      lastFrame = window.getFrame();
    } catch {
      // Window not queryable (e.g. mid-teardown); keep the last good frame.
    }
  };
  window.on("move", captureFrame);
  window.on("resize", captureFrame);
  window.on("close", () => {
    saveWindowState(lastFrame);
    if (mainWindow === window) {
      mainWindow = null;
    }
  });

  return window;
};

/**
 * Point the window at the inspector the sidecar serves.
 *
 * On macOS the window controls float over the web content (`hiddenInset`), so
 * the inspector is told to keep a titlebar row clear of its own chrome — it
 * serves the same bundle to browsers, which need no such row.
 */
const loadInspector = (conn: SidecarConnection): void => {
  const window = mainWindow ?? createMainWindow();
  const query = isMac ? `?titlebar=${INSET_TITLEBAR}` : "";
  window.webview.loadURL(`http://${conn.hostname}:${conn.port}/${query}`);
};

/** Swap the dead web UI for the crash screen (its button drives restartServer). */
const showCrashScreen = (): void => {
  mainWindow?.webview.loadURL(CRASH_VIEW_URL);
};

let restartInFlight: Promise<void> | null = null;

const doRestart = async (): Promise<void> => {
  if (connection) {
    await stopSidecar(connection.child);
    connection = null;
  }
  const next = await startWithReporting();
  if (!next) {
    throw new Error("Sidecar failed to restart.");
  }
  // A quit can race the spawn (connection is null across the await, so the
  // before-quit reap sees nothing). Reap the just-spawned child rather than
  // adopting it.
  if (quitting) {
    await stopSidecar(next.child);
    return;
  }
  connection = next;
  loadInspector(next);
};

/**
 * Stop the current sidecar, start a fresh one, and reload the window.
 * Serialized: concurrent callers (e.g. a double-clicked crash-screen button)
 * share the one in-flight restart instead of spawning competing sidecars.
 */
const restartSidecarAndReload = (): Promise<void> => {
  if (!restartInFlight) {
    restartInFlight = doRestart().finally(() => {
      restartInFlight = null;
    });
  }
  return restartInFlight;
};

// Last non-port-conflict startup failure, surfaced by boot() in a dialog.
let lastSidecarStartError: unknown = null;

/** Start the sidecar, mapping a port conflict to a dialog and returning null. */
const startWithReporting = async (): Promise<SidecarConnection | null> => {
  try {
    return await startSidecar();
  } catch (error) {
    if (error instanceof SidecarPortInUseError) {
      await Utils.showMessageBox({
        type: "error",
        title: "Peektrace port in use",
        message: `Port ${error.port} is already taken.`,
        detail:
          "Another process is listening on that port. Quit it and relaunch Peektrace.",
      });
      return null;
    }
    lastSidecarStartError = error;
    log.error("Failed to start Peektrace sidecar", error);
    return null;
  }
};

/** Surface a fatal startup failure in a dialog before quitting. */
const showFatalDialog = async (error: unknown): Promise<void> => {
  showCrashScreen();
  const detail =
    error instanceof Error ? (error.stack ?? error.message) : String(error);
  await Utils.showMessageBox({
    type: "error",
    title: "Peektrace failed to start",
    message: "The local Peektrace server crashed during startup.",
    detail: `${detail.slice(0, FATAL_DETAIL_MAX)}\n\nFull log: ${logFilePath() ?? "unavailable"}`,
  });
};

/** Synchronously reap the sidecar. Called from quit paths where async is unsafe. */
const reapSidecar = (): void => {
  const child: ChildProcess | undefined = connection?.child;
  connection = null;
  if (child) {
    killSidecarSync(child);
  }
};

const installMenu = (): void => {
  installApplicationMenu({
    onAbout: () => {
      Utils.showMessageBox({
        type: "info",
        title: "Peektrace",
        message: "Peektrace",
        detail: "Local trace inspector for AI coding sessions.",
      }).catch(() => undefined);
    },
    onDocs: () => {
      Utils.openExternal(DOCS_URL);
    },
    onCheckUpdates: () => {
      checkForUpdates({
        interactive: true,
        beforeApply: async () => {
          if (connection) {
            await stopSidecar(connection.child);
            connection = null;
          }
        },
      }).catch((error) => log.error("Update check failed", error));
    },
  });
};

const boot = async (): Promise<void> => {
  installMenu();
  createMainWindow();

  // A sidecar dying under a live window leaves every request failing with no
  // explanation — swap in the crash screen whose Restart button drives the RPC.
  onUnexpectedSidecarExit(() => {
    if (!quitting) {
      showCrashScreen();
    }
  });

  connection = await startWithReporting();
  if (!connection) {
    if (lastSidecarStartError != null) {
      await showFatalDialog(lastSidecarStartError);
    }
    Utils.quit();
    return;
  }
  loadInspector(connection);

  // A background boot check kicks off the updater immediately; inert until a
  // release baseUrl is configured.
  checkForUpdates({ interactive: false }).catch((error) =>
    log.error("Update check failed", error)
  );
};

// Reap the sidecar before the app exits. before-quit fires for menu quit, Cmd+Q,
// and the exitOnLastWindowClosed path, and reaps on the graceful path. The
// process 'exit' hook only covers plain JS-driven exits (not native force-exit
// or SIGKILL); the CLI's own SIGTERM handler is the real backstop there.
Electrobun.events.on("before-quit", () => {
  quitting = true;
  reapSidecar();
});
process.on("exit", reapSidecar);

boot().catch((error) => {
  log.error("Fatal boot error", error);
  showFatalDialog(error).finally(() => Utils.quit());
});
