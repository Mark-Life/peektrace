/** The history opens as a chat; only an explicit `view=table` picks the table. */
import { expect, test } from "bun:test";
import { transcriptModeOf } from "../src/lib/transcript-mode";

test("no view param reads as chat", () => {
  expect(transcriptModeOf(null)).toBe("chat");
});

test("view=table selects the table", () => {
  expect(transcriptModeOf("table")).toBe("table");
});

test("legacy view=chat and unknown values fall back to chat", () => {
  expect(transcriptModeOf("chat")).toBe("chat");
  expect(transcriptModeOf("grid")).toBe("chat");
});
