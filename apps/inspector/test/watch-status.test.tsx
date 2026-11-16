/** The header's live toggle states: one fixed name, pressed when paused, and a
 *  live-region description that says when changes are held back. */
import { expect, test } from "bun:test";

import { TooltipProvider } from "@workspace/ui/components/tooltip";
import { renderToStaticMarkup } from "react-dom/server";

import type { LiveMode } from "../src/components/watch-status";
import { WatchStatusView } from "../src/components/watch-status";

const render = (mode: LiveMode, syncing = false) =>
  renderToStaticMarkup(
    <TooltipProvider>
      <WatchStatusView mode={mode} onToggle={() => {}} syncing={syncing} />
    </TooltipProvider>
  );

const HIDDEN_SYNCING_LABEL =
  /aria-hidden="true"[^>]*><span[^>]*>syncing<\/span>/u;

const WAITING_LIVE_REGION =
  /aria-live="polite"[^>]*role="status"[^>]*>paused · updates waiting</u;

const DESCRIBED_BY = /aria-describedby="(?<id>[^"]+)"/u;

/** Text of the element the toggle's `aria-describedby` points at. */
const describedBy = (html: string) => {
  const id = DESCRIBED_BY.exec(html)?.groups?.id;

  return id === undefined
    ? undefined
    : new RegExp(`id="${id}"[^>]*>(?<text>[^<]*)<`, "u").exec(html)?.groups
        ?.text;
};

test("live: the toggle is not pressed and the label is hidden", () => {
  const html = render("live", true);
  expect(html).toContain('aria-pressed="false"');
  expect(html).toContain('aria-label="Pause live updates"');
  expect(html).toMatch(HIDDEN_SYNCING_LABEL);
  expect(html).not.toContain('role="status"');
  expect(html).not.toContain("aria-live");
  expect(describedBy(html)).toBeUndefined();
});

test("paused: the same toggle is pressed and described as paused", () => {
  const html = render("paused");
  expect(html).toContain('aria-pressed="true"');
  expect(html).toContain('aria-label="Pause live updates"');
  expect(html).not.toContain("Resume live updates");
  expect(describedBy(html)).toBe("paused");
  expect(html).toContain('aria-live="polite"');
  expect(html).toContain('role="status"');
});

test("paused with changes on disk is announced through the live region", () => {
  const html = render("waiting");
  expect(describedBy(html)).toBe("paused · updates waiting");
  expect(html).toMatch(WAITING_LIVE_REGION);
});
