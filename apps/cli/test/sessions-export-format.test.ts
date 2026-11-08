/** `pickFormat`: which export format wins. */
import { expect, test } from "bun:test";
import { pickFormat } from "../src/commands/sessions-export";

const none = { format: undefined, json: false, out: undefined } as const;

test.each([
  [{ ...none }, "md"],
  [{ ...none, out: "a.html" }, "html"],
  [{ ...none, out: "a.htm" }, "html"],
  [{ ...none, out: "a.md" }, "md"],
  [{ ...none, out: "a.markdown" }, "md"],
  [{ ...none, out: "a.json" }, "json"],
  [{ ...none, out: "A.HTML" }, "html"],
  [{ ...none, out: "a.txt" }, "md"],
  [{ ...none, out: "noext" }, "md"],
  [{ ...none, json: true }, "json"],
  [{ ...none, json: true, out: "a.html" }, "json"],
  [{ ...none, format: "html", json: true }, "html"],
  [{ ...none, format: "md", out: "a.json" }, "md"],
] as const)("%o -> %s", (input, expected) => {
  expect(pickFormat(input)).toBe(expected);
});
