/** What a transcript row calls an event.
 *
 * Every attachment used to badge as `attachment`, so a skill listing and a file
 * opened in the IDE looked identical until expanded. These pin the humanised
 * type — including for a type nobody has written down here.
 */

import { describe, expect, test } from "bun:test";

import type { TimelineEvent } from "@workspace/view-core/api-types.gen";

import { eventBadgeLabel, isUserAuthored } from "../src/lib/event-labels";

const attachment = (attachmentType?: string): TimelineEvent => {
  const base: TimelineEvent = {
    body: "…",
    index: 3,
    kind: "attachment",
    preview: "…",
    title: "attachment",
    tokensEst: 12,
  };

  return attachmentType === undefined ? base : { ...base, attachmentType };
};

const prompt = (promptOrigin?: TimelineEvent["promptOrigin"]) =>
  isUserAuthored(
    promptOrigin === undefined
      ? { kind: "user-prompt" }
      : { kind: "user-prompt", promptOrigin }
  );

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
      body: "{}",
      index: 0,
      kind: "tool-call",
      preview: "ls",
      title: "Bash",
      tokensEst: 4,
      toolName: "Bash",
    };

    expect(eventBadgeLabel(call)).toBe("Bash");
  });

  test("an event carrying neither badges as its kind", () => {
    expect(eventBadgeLabel({ kind: "assistant-text" })).toBe("assistant-text");
  });
});

describe("isUserAuthored", () => {
  test("only prompts a person sent are the user's", () => {
    expect((["human", "sdk", "command"] as const).every((o) => prompt(o))).toBe(
      true
    );
    expect(
      (["task-notification", "scheduled", "harness"] as const).some((o) =>
        prompt(o)
      )
    ).toBe(false);
  });

  test("an event without an origin still counts as the user's", () => {
    expect(isUserAuthored({ kind: "user-prompt" })).toBe(true);
    expect(isUserAuthored({ kind: "assistant-text" })).toBe(false);
  });
});
