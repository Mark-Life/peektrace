/** The header's live toggle states: pressed when paused, and a label that says
 *  when changes are held back. */
import { expect, test } from "bun:test";
import { TooltipProvider } from "@workspace/ui/components/tooltip";
import { renderToStaticMarkup } from "react-dom/server";
import type { LiveMode } from "../src/components/watch-status";
import { WatchStatusView } from "../src/components/watch-status";

const render = (mode: LiveMode, syncing = false) =>
  renderToStaticMarkup(
    <TooltipProvider>
      <WatchStatusView
        mode={mode}
        onToggle={() => undefined}
        syncing={syncing}
      />
    </TooltipProvider>
  );

test("live: the toggle offers to pause", () => {
  const html = render("live", true);
  expect(html).toContain('aria-pressed="false"');
  expect(html).toContain('aria-label="Pause live updates"');
  expect(html).toContain("syncing");
});

test("paused: the toggle is pressed and offers to resume", () => {
  const html = render("paused");
  expect(html).toContain('aria-pressed="true"');
  expect(html).toContain('aria-label="Resume live updates"');
  expect(html).toContain(">paused<");
});

test("paused with changes on disk says so", () => {
  expect(render("waiting")).toContain("paused · updates waiting");
});
