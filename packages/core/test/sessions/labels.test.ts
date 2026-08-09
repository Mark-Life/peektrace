/** What a transcript row calls an event.
 *
 * Every attachment used to badge as `attachment`, so a skill listing and a file
 * opened in the IDE looked identical until expanded. These pin the humanised
 * type — including for a type nobody has written down here.
 */

import { describe, expect, test } from "bun:test";
import { eventBadgeLabel } from "../../src/services/sessions/labels";
import type { TimelineEvent } from "../../src/services/sessions/schema";

const attachment = (attachmentType?: string): TimelineEvent => ({
  index: 3,
  kind: "attachment",
  title: "attachment",
  preview: "…",
  body: "…",
  tokensEst: 12,
  ...(attachmentType === undefined ? {} : { attachmentType }),
});

describe("eventBadgeLabel", () => {
  test("an attachment badges as its type, underscores as spaces", () => {
    expect(eventBadgeLabel(attachment("skill_listing"))).toBe("skill listing");
    expect(eventBadgeLabel(attachment("opened_file_in_ide"))).toBe(
      "opened file in ide"
    );
  });

  test("a type nobody has seen yet still reads as words", () => {
    expect(eventBadgeLabel(attachment("some_future_thing"))).toBe(
      "some future thing"
    );
  });

  test("a missing or empty type falls back to the kind", () => {
    expect(eventBadgeLabel(attachment())).toBe("attachment");
    expect(eventBadgeLabel(attachment(""))).toBe("attachment");
    expect(eventBadgeLabel(attachment("   "))).toBe("attachment");
  });

  test("a tool call still badges as its tool", () => {
    const call: TimelineEvent = {
      index: 0,
      kind: "tool-call",
      title: "Bash",
      preview: "ls",
      body: "{}",
      tokensEst: 4,
      toolName: "Bash",
    };
    expect(eventBadgeLabel(call)).toBe("Bash");
  });

  test("an event carrying neither badges as its kind", () => {
    expect(
      eventBadgeLabel({
        index: 1,
        kind: "assistant-text",
        title: "Assistant",
        preview: "hi",
        body: "hi",
        tokensEst: 1,
      })
    ).toBe("assistant-text");
  });
});
