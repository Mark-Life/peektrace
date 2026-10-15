/** A hash `view` overrides the stored layout; anything unknown is ignored. */
import { expect, test } from "bun:test";
import {
  resolveTranscriptMode,
  transcriptModeOf,
} from "../src/lib/transcript-mode";

test("only table and chat parse as modes", () => {
  expect(transcriptModeOf("table")).toBe("table");
  expect(transcriptModeOf("chat")).toBe("chat");
  expect(transcriptModeOf("grid")).toBeNull();
  expect(transcriptModeOf(null)).toBeNull();
});

test("no hash view falls back to the stored layout", () => {
  expect(resolveTranscriptMode({ hash: null, stored: "table" })).toBe("table");
  expect(resolveTranscriptMode({ hash: null, stored: "chat" })).toBe("chat");
});

test("an explicit hash view wins over the stored layout", () => {
  expect(resolveTranscriptMode({ hash: "chat", stored: "table" })).toBe("chat");
  expect(resolveTranscriptMode({ hash: "table", stored: "chat" })).toBe(
    "table"
  );
});

test("an unknown hash view keeps the stored layout", () => {
  expect(resolveTranscriptMode({ hash: "grid", stored: "table" })).toBe(
    "table"
  );
});
