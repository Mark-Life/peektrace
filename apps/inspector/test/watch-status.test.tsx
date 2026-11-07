/** The header's live toggle states: one fixed name, pressed when paused, and a
 *  description that says when changes are held back. */
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

const DESCRIBED_BY = /aria-describedby="([^"]+)"/;

/** Text of the element the toggle's `aria-describedby` points at. */
const describedBy = (html: string) => {
  const id = html.match(DESCRIBED_BY)?.[1];
  return id === undefined
    ? undefined
    : html.match(new RegExp(`id="${id}"[^>]*>([^<]*)<`))?.[1];
};

test("live: the toggle is not pressed and the label is hidden", () => {
  const html = render("live", true);
  expect(html).toContain('aria-pressed="false"');
  expect(html).toContain('aria-label="Pause live updates"');
  expect(html).toContain("syncing");
  expect(describedBy(html)).toBeUndefined();
});

test("paused: the same toggle is pressed and described as paused", () => {
  const html = render("paused");
  expect(html).toContain('aria-pressed="true"');
  expect(html).toContain('aria-label="Pause live updates"');
  expect(html).not.toContain("Resume live updates");
  expect(describedBy(html)).toBe("paused");
});

test("paused with changes on disk tells screen readers too", () => {
  expect(describedBy(render("waiting"))).toBe("paused · updates waiting");
});
