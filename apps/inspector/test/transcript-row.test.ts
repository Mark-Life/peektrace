/** The memo contract behind a flicker-free live transcript.
 *
 * A row that re-renders on every agent write is what made the live view
 * unreadable; a row that *fails* to re-render when its content changed would be
 * worse — a silently stale transcript. These pin both directions.
 */

import { describe, expect, test } from "bun:test";

import type { TimelineEvent } from "@workspace/view-core/api-types.gen";

import {
  CHAT_ROW_FIELDS,
  ROW_FIELDS,
  sameChatEvent,
  sameEvent,
} from "../src/lib/transcript-row";

const event: TimelineEvent = {
  body: '{"command":"ls -la /workspace"}',
  index: 7,
  isSidechain: false,
  kind: "tool-call",
  preview: "ls -la /workspace",
  requestId: "req_1",
  title: "Bash",
  tokensEst: 42,
  toolName: "Bash",
  toolUseId: "toolu_1",
  ts: "2026-08-08T15:00:00.000Z",
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

  test("a context edit is compared by value", () => {
    const edited: TimelineEvent = {
      ...event,
      contextEdit: {
        atIndex: 9,
        original: "old",
        originalTokensEst: 10,
        status: "replaced",
      },
    };

    expect(sameEvent(edited, structuredClone(edited))).toBe(true);
    expect(sameEvent(event, edited)).toBe(false);
    expect(
      sameChatEvent(edited, {
        ...edited,
        contextEdit: { atIndex: 9, status: "removed" },
      })
    ).toBe(false);
  });

  test("an attachment that changes type re-renders its badge", () => {
    const listing: TimelineEvent = {
      ...event,
      attachmentType: "skill_listing",
      kind: "attachment",
      toolName: undefined,
    };

    expect(sameEvent(listing, { ...listing, attachmentType: "todo" })).toBe(
      false
    );
    expect(sameEvent(listing, structuredClone(listing))).toBe(true);
  });

  test("an appended event is never mistaken for an existing one", () => {
    const appended: TimelineEvent = { ...event, index: 8, preview: "next" };
    expect(sameEvent(event, appended)).toBe(false);
  });
});

describe("sameChatEvent", () => {
  test("every field a chat leaf renders forces a re-render when it changes", () => {
    for (const field of CHAT_ROW_FIELDS) {
      const changed = { ...event, [field]: `${event[field]}-changed` };
      expect(sameChatEvent(event, changed)).toBe(false);
    }
  });

  test("a chip label and its icon are chat-only fields", () => {
    const relabelled = { ...event, title: "Other" };
    expect(sameEvent(event, relabelled)).toBe(true);
    expect(sameChatEvent(event, relabelled)).toBe(false);
  });

  test("fields no leaf renders still do not force a re-render", () => {
    expect(
      sameChatEvent(event, { ...event, ts: "2026-01-01T00:00:00.000Z" })
    ).toBe(true);
    expect(sameChatEvent(event, { ...event, requestId: "req_2" })).toBe(true);
  });
});
