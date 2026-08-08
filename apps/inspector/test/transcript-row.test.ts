/** The memo contract behind a flicker-free live transcript.
 *
 * A row that re-renders on every agent write is what made the live view
 * unreadable; a row that *fails* to re-render when its content changed would be
 * worse — a silently stale transcript. These pin both directions.
 */

import { describe, expect, test } from "bun:test";
import type { TimelineEvent } from "@workspace/core/services/sessions/schema";
import { ROW_FIELDS, sameEvent } from "../src/lib/transcript-row";

const event: TimelineEvent = {
  index: 7,
  kind: "tool-call",
  ts: "2026-08-08T15:00:00.000Z",
  requestId: "req_1",
  isSidechain: false,
  title: "Bash",
  preview: "ls -la /workspace",
  body: '{"command":"ls -la /workspace"}',
  tokensEst: 42,
  toolName: "Bash",
  toolUseId: "toolu_1",
};

describe("sameEvent", () => {
  test("a re-analyzed session's fresh object is still the same row", () => {
    expect(sameEvent(event, structuredClone(event))).toBe(true);
    expect(event === structuredClone(event)).toBe(false);
  });

  test("every field the row renders forces a re-render when it changes", () => {
    for (const field of ROW_FIELDS) {
      const changed = { ...event, [field]: `${event[field]}-changed` };
      expect(sameEvent(event, changed)).toBe(false);
    }
  });

  test("fields the row never renders do not force a re-render", () => {
    // `ts`/`requestId`/`title` feed grouping and filtering, not the row itself.
    expect(sameEvent(event, { ...event, ts: "2026-01-01T00:00:00.000Z" })).toBe(
      true
    );
    expect(sameEvent(event, { ...event, requestId: "req_2" })).toBe(true);
    expect(sameEvent(event, { ...event, title: "Other" })).toBe(true);
  });

  test("an appended event is never mistaken for an existing one", () => {
    const appended: TimelineEvent = { ...event, index: 8, preview: "next" };
    expect(sameEvent(event, appended)).toBe(false);
  });
});
