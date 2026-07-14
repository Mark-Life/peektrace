/**
 * Auto-update flow over Electrobun's built-in differential updater.
 *
 * Electrobun updates are hash-based and served from a static `release.baseUrl`
 * (S3/R2/GitHub Releases) — a different hosting model than the previous
 * electron-updater + GitHub `latest*.yml` pipeline. Until that host is wired up
 * (baseUrl unset), this is a friendly no-op: a manual check just reports that
 * updates aren't configured. The check → download → prompt → apply flow below is
 * ready for when a release pipeline publishes an `artifacts/` folder.
 */
import { Updater, Utils } from "electrobun/bun";
import { scoped } from "./log";

const updaterLog = scoped("updater");

/** Resolve the configured release base URL, or "" when updates are disabled. */
const releaseBaseUrl = async (): Promise<string> => {
  try {
    const local = await Updater.getLocalInfo();
    return local.baseUrl ?? "";
  } catch {
    return "";
  }
};

export interface UpdateCheckOptions {
  /** Run before the app is replaced (e.g. stop the sidecar). */
  readonly beforeApply?: () => Promise<void>;
  /** Surface "no update" / "not configured" outcomes in a dialog. */
  readonly interactive: boolean;
}

/**
 * Check for an update and, when one is ready, prompt the user to restart. Errors
 * are logged; they only reach a dialog when `interactive` (a manual menu check).
 */
export const checkForUpdates = async ({
  interactive,
  beforeApply,
}: UpdateCheckOptions): Promise<void> => {
  const baseUrl = await releaseBaseUrl();
  if (!baseUrl) {
    if (interactive) {
      await Utils.showMessageBox({
        type: "info",
        title: "Updates unavailable",
        message: "Auto-update isn't configured for this build.",
      });
    }
    return;
  }

  try {
    const info = await Updater.checkForUpdate();
    if (info.error) {
      throw new Error(info.error);
    }
    if (!info.updateAvailable) {
      if (interactive) {
        await Utils.showMessageBox({
          type: "info",
          title: "No updates",
          message: "You're on the latest version.",
        });
      }
      return;
    }

    await Updater.downloadUpdate();
    if (!Updater.updateInfo()?.updateReady) {
      throw new Error("Update downloaded but is not ready to apply.");
    }

    const { response } = await Utils.showMessageBox({
      type: "info",
      title: "Update ready",
      message: `Peektrace ${info.version} is ready to install.`,
      detail: "Restart now to apply the update, or keep working.",
      buttons: ["Restart now", "Later"],
      defaultId: 0,
      cancelId: 1,
    });
    if (response === 0) {
      await beforeApply?.();
      await Updater.applyUpdate();
    }
  } catch (error) {
    updaterLog.warn("check failed", error);
    if (interactive) {
      await Utils.showMessageBox({
        type: "error",
        title: "Update check failed",
        message: "Couldn't check for updates.",
        detail: "Check your network and try again from the menu.",
      });
    }
  }
};
