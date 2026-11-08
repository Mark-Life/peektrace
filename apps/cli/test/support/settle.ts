/** Deadline-based frame polling for TUI tests driven by async bridge calls. */
import type { testRender } from "@opentui/react/test-utils";

type Setup = Awaited<ReturnType<typeof testRender>>;

const TICK_MS = 10;

/**
 * Test timeout for files that use `settleFor`. The `settleFor` waits in one
 * test must add up to less than this, so a missed needle throws with its frame
 * instead of ending in a bare bun timeout. Apply with `setDefaultTimeout`.
 */
export const SETTLE_TEST_TIMEOUT_MS = 15_000;

/**
 * Poll for `needle` while advancing the timer queue: async bridge chains need
 * real macrotasks that `waitForFrame` skips. Throws with the last frame when
 * `needle` has not shown up by the deadline.
 */
export const settleFor = async (
  setup: Setup,
  needle: string,
  timeoutMs = 4000
) => {
  const deadline = Date.now() + timeoutMs;
  let frame = "";
  while (Date.now() < deadline) {
    await setup.renderOnce();
    frame = setup.captureCharFrame();
    if (frame.includes(needle)) {
      return frame;
    }
    await new Promise((r) => setTimeout(r, TICK_MS));
  }
  throw new Error(
    `settleFor: "${needle}" not rendered within ${timeoutMs}ms. Last frame:\n${frame}`
  );
};
