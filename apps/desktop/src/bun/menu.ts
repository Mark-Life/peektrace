/**
 * Native application menu (macOS/Windows; Linux has no app menu in Electrobun).
 *
 * Beyond the custom Peektrace actions, the Edit menu roles matter: without them a
 * WebView-hosted app gets no Cmd/Ctrl+C/V/Z, breaking text fields in the
 * inspector. Roles bind the standard OS shortcuts automatically.
 */
import Electrobun, { ApplicationMenu } from "electrobun/bun";

type MenuTemplate = Parameters<typeof ApplicationMenu.setApplicationMenu>[0];

export interface MenuActionHandlers {
  readonly onAbout: () => void;
  readonly onCheckUpdates: () => void;
  readonly onDocs: () => void;
}

const buildTemplate = (): MenuTemplate => [
  {
    label: "Peektrace",
    submenu: [
      { label: "About Peektrace", action: "about" },
      { type: "separator" },
      { label: "Check for Updates…", action: "check-updates" },
      { label: "Documentation", action: "docs" },
      { type: "separator" },
      { role: "hide" },
      { role: "hideOthers" },
      { role: "showAll" },
      { type: "separator" },
      { role: "quit" },
    ],
  },
  {
    label: "Edit",
    submenu: [
      { role: "undo" },
      { role: "redo" },
      { type: "separator" },
      { role: "cut" },
      { role: "copy" },
      { role: "paste" },
      { role: "pasteAndMatchStyle" },
      { role: "delete" },
      { role: "selectAll" },
    ],
  },
  {
    label: "View",
    submenu: [{ role: "toggleFullScreen" }],
  },
  {
    label: "Window",
    submenu: [{ role: "minimize" }, { role: "zoom" }, { role: "close" }],
  },
];

/**
 * Install the application menu and route its custom actions to the handlers.
 * Safe to call once at startup; the click listener is registered globally.
 */
export const installApplicationMenu = (handlers: MenuActionHandlers): void => {
  ApplicationMenu.setApplicationMenu(buildTemplate());
  Electrobun.events.on("application-menu-clicked", (event: unknown) => {
    const action = (event as { data?: { action?: string } }).data?.action;
    switch (action) {
      case "about":
        handlers.onAbout();
        break;
      case "docs":
        handlers.onDocs();
        break;
      case "check-updates":
        handlers.onCheckUpdates();
        break;
      default:
        break;
    }
  });
};
