/** Export download helper: file name, MIME type and redaction pass-through. */
import { expect, test } from "bun:test";
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import { exportFile } from "../src/lib/export-file";

const session = {
  provider: "claude",
  sessionId: "0123456789-abcdef",
  path: "/x/t.jsonl",
  cwd: "/home/demo/work",
  models: [],
  events: [],
  turns: [],
  compactionIndexes: [],
  subagents: [],
} as unknown as AnalyzedSession;

const at = "2026-02-02T00:00:00.000Z";

test("names the file with the short id and format", () => {
  expect(exportFile(session, "md", true, at).name).toBe("session-01234567.md");
  expect(exportFile(session, "json", true, at).name).toBe(
    "session-01234567.json"
  );
});

test("sets the MIME type per format", () => {
  expect(exportFile(session, "html", true, at).type).toBe(
    "text/html;charset=utf-8"
  );
  expect(exportFile(session, "md", true, at).type).toBe(
    "text/markdown;charset=utf-8"
  );
  expect(exportFile(session, "json", true, at).type).toBe(
    "application/json;charset=utf-8"
  );
});

test("passes the redaction flag to the renderer", () => {
  expect(exportFile(session, "md", true, at).text).toContain("~/work");
  expect(exportFile(session, "md", false, at).text).toContain(
    "/home/demo/work"
  );
});
