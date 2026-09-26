/** Pi `context_edit` entries: edited targets show the new content, dropped
 * targets are marked, and budget slices switch to the edited size only after
 * the edit's line.
 */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { analyze } from "../../src/services/sessions/analyze";
import {
  contextEditNote,
  contextEditTag,
} from "../../src/services/sessions/labels";
import { parsePiSession } from "../../src/services/sessions/parsers/pi";
import { redactParsed } from "../../src/services/sessions/redact";
import { estTokens } from "../../src/services/sessions/tokens";

const path = join(
  import.meta.dir,
  "../fixtures/sessions-pi/pi-context-edit.jsonl"
);
const fixture = (name: string) =>
  join(import.meta.dir, `../fixtures/sessions-pi/${name}`);
const parseText = (text: string) =>
  parsePiSession({ path, text, sessionId: "fallback", slug: "" });
const parse = () => parseText(readFileSync(path, "utf8"));

/** Events built from the transcript line at `index`. */
const at = (index: number) => parse().events.filter((e) => e.index === index);

describe("pi context edits in history", () => {
  test("an omitted assistant attempt keeps its text and is marked removed", () => {
    const [ev] = at(8);
    expect(ev?.body).toBe("A".repeat(400));
    expect(ev?.contextEdit).toEqual({ status: "removed", atIndex: 9 });
  });

  test("a replaced tool result shows the edit and keeps the original", () => {
    const [ev] = at(4);
    expect(ev?.body).toBe("[big.txt elided]");
    expect(ev?.tokensEst).toBe(estTokens("[big.txt elided]"));
    expect(ev?.toolUseId).toBe("t1");
    expect(ev?.contextEdit).toEqual({
      status: "replaced",
      atIndex: 10,
      originalTokensEst: 1000,
      original: "X".repeat(4000),
    });
  });

  test("the latest edit of a target wins", () => {
    const [ev] = at(5);
    expect(ev?.body).toStartWith("reminder ");
    expect(ev?.contextEdit).toEqual({
      status: "removed",
      atIndex: 12,
      steps: [
        { atIndex: 11, tokensEst: estTokens("first edit"), thinkingEst: 0 },
      ],
    });
  });

  test("a string replacement of assistant content becomes one text block", () => {
    const events = at(6);
    expect(events.map((e) => e.kind)).toEqual(["assistant-text"]);
    expect(events[0]?.body).toBe("Short summary.");
    expect(events[0]?.contextEdit).toMatchObject({
      status: "replaced",
      originalTokensEst: estTokens("Summarizing the big file in detail."),
    });
  });

  test("edits of entries pi cannot edit are ignored", () => {
    expect(parse().events.filter((e) => e.contextEdit)).toHaveLength(4);
  });

  test("turns still link to the events of their own call", () => {
    const p = parse();
    const texts = p.turns.map((t) =>
      t.eventIndexes.map((i) => p.events[i]?.body)
    );
    expect(texts).toEqual([
      ["", JSON.stringify({ path: "big.txt" }, null, 2)],
      ["Short summary."],
      ["A".repeat(400)],
      ["Done."],
    ]);
  });
});

describe("pi context edits in the budget", () => {
  const snaps = () => analyze(parse()).snapshots;

  test("turns before the edits count the original content", () => {
    const s = snaps()[2]?.slices;
    expect(s?.tool_results).toBe(1000);
    expect(s?.other).toBe(estTokens(`reminder ${"R".repeat(391)}`));
  });

  test("turns after the edits count the edited content", () => {
    const s = snaps()[3]?.slices;
    const call = estTokens(JSON.stringify({ path: "big.txt" }, null, 2));
    expect(s?.tool_results).toBe(estTokens("[big.txt elided]"));
    expect(s?.other).toBe(0);
    expect(s?.assistant_text).toBe(call + estTokens("Short summary."));
    // Only the first call's thinking survives: the second was replaced
    // wholesale and the third dropped.
    expect(s?.thinking).toBe(50 - call);
  });
});

describe("pi context edit labels", () => {
  test("tags and notes name the edit line and keep the original", () => {
    const [removed] = at(8);
    const [replaced] = at(4);
    expect(removed && contextEditTag(removed)).toBe("removed");
    expect(replaced && contextEditTag(replaced)).toBe("edited");
    expect(removed && contextEditNote(removed)).toContain("line 10");
    expect(replaced && contextEditNote(replaced)).toContain("~1000 tok");
    expect(replaced && contextEditNote(replaced)).toEndWith("X".repeat(4000));
  });
});

describe("pi context edits over several steps", () => {
  const p = () =>
    parsePiSession({
      path: fixture("pi-context-edit-steps.jsonl"),
      text: readFileSync(fixture("pi-context-edit-steps.jsonl"), "utf8"),
      sessionId: "fallback",
      slug: "",
    });
  const snaps = () => analyze(p()).snapshots;
  const call = estTokens(JSON.stringify({ path: "big.txt" }, null, 2));

  test("turns between two edits count the first replacement", () => {
    const s = snaps()[2]?.slices;
    expect(s?.tool_results).toBe(estTokens("Y".repeat(800)));
    expect(s?.assistant_text).toBe(call + estTokens("short"));
  });

  test("turns after the last edit count the final state", () => {
    const s = snaps()[3]?.slices;
    expect(s?.tool_results).toBe(0);
    expect(s?.assistant_text).toBe(call + estTokens("ok"));
  });

  test("turns before the first edit count the original, as measured", () => {
    const s = snaps()[1]?.slices;
    // ctx 2200 - ctx 1000 - output 20: the tool result's real size.
    expect(p().events.find((e) => e.index === 4)?.tokensMeasured).toBe(1180);
    expect(s?.tool_results).toBe(1180);
  });

  test("the note names every edit line", () => {
    const ev = p().events.find((e) => e.index === 4);
    expect(ev && contextEditNote(ev)).toBe(
      "Dropped from model context by the edit at line 10. Earlier edit at line 7. Later turns do not see it."
    );
  });
});

describe("pi context edits under redaction", () => {
  test("the replaced original is redacted", () => {
    const secret = `sk-ant-api03-${"a1B2c3D4e5F6g7H8".repeat(6)}`;
    const text = readFileSync(path, "utf8").replace(
      "X".repeat(40),
      `api_key=${secret} `
    );
    const ev = redactParsed(parseText(text)).events.find((e) => e.index === 4);
    const edit = ev?.contextEdit;
    expect(edit?.status).toBe("replaced");
    expect(edit?.status === "replaced" && edit.original).not.toContain(secret);
  });
});
