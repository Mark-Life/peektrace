/** Export download helper: file name, and the route with the screen's redaction. */
import { expect, test } from "bun:test";

import { exportName, exportRequest } from "../src/lib/export-file";

test("names the file with the short id and format", () => {
  expect(exportName("0123456789-abcdef", "md")).toBe("session-01234567.md");
  expect(exportName("0123456789-abcdef", "json")).toBe("session-01234567.json");
});

test("asks the export route with the screen's redaction and parent", () => {
  expect(
    exportRequest({ id: "s1", parent: null, redact: true }, "html")
  ).toEqual({
    path: "/api/sessions/s1/export",
    query: { format: "html", parent: undefined, redact: undefined },
  });
  expect(
    exportRequest({ id: "agent-1", parent: "s1", redact: false }, "md")
  ).toEqual({
    path: "/api/sessions/agent-1/export",
    query: { format: "md", parent: "s1", redact: "false" },
  });
});
