/** Boot the OpenTUI renderer and mount the React tree.
 *
 * `runTui` owns the native renderer lifecycle: it resolves only when the user
 * quits (`q` / `Ctrl+C`, routed through `App`'s `onQuit`), at which point the
 * renderer is destroyed (terminal restored, native resources freed) and the
 * caller's Effect scope tears the sibling HTTP server down. `exitOnCtrlC` is off
 * so teardown always runs through the same path.
 */
import { createCliRenderer } from "@opentui/core";
import { createRoot } from "@opentui/react";
import { App } from "./app";
import type { TuiBridge } from "./runtime";

/** Render the TUI; resolves when the user quits. */
export const runTui = async (bridge: TuiBridge): Promise<void> => {
  const renderer = await createCliRenderer({ exitOnCtrlC: false });
  await new Promise<void>((resolve) => {
    let quit = false;
    const onQuit = () => {
      if (quit) {
        return;
      }
      quit = true;
      renderer.destroy();
      resolve();
    };
    createRoot(renderer).render(<App bridge={bridge} onQuit={onQuit} />);
  });
};
