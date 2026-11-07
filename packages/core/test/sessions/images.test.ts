/** Images are located, not copied: events carry a ref, bytes come on demand. */
import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { imageAt } from "../../src/services/sessions/images";
import {
  jsonlLineAt,
  parseClaudeSession,
  parseJsonl,
} from "../../src/services/sessions/parse";

const path = join(
  import.meta.dir,
  "../fixtures/sessions-claude/measured-image.jsonl"
);
const text = readFileSync(path, "utf8");
const lines = parseJsonl(text);

describe("jsonlLineAt", () => {
  test("numbers lines the way parseJsonl does, parsing only the one asked for", () => {
    for (const [n, line] of lines.entries()) {
      expect(jsonlLineAt(text, n)).toEqual(line);
    }
    const gappy = '{"a":1}\n\n  \n{"b":2}\n{"c":';
    expect(jsonlLineAt(gappy, 1)).toEqual({ b: 2 });
    expect(jsonlLineAt(gappy, 2)).toBeUndefined();
  });
});

describe("image refs", () => {
  test("a pasted image and a tool-result screenshot each carry a ref", () => {
    const events = parseClaudeSession({ text, path, sessionId: "s" }).events;
    const withImages = events.filter((e) => e.images);
    expect(withImages.map((e) => [e.kind, e.index, e.images])).toEqual([
      ["tool-result", 2, [{ block: 0, item: 0, mediaType: "image/png" }]],
      ["attachment", 4, [{ block: 1, mediaType: "image/png" }]],
    ]);
    expect(withImages.every((e) => !e.body.includes("iVBORw0KGgo"))).toBe(true);
  });

  test("a ref resolves to the base64 on its line", () => {
    expect(imageAt({ line: lines[4], block: 1 })?.mediaType).toBe("image/png");
    expect(imageAt({ line: lines[2], block: 0, item: 0 })?.data).toStartWith(
      "iVBORw0KGgo"
    );
  });

  test("a ref to anything but a showable base64 image resolves to nothing", () => {
    expect(imageAt({ line: lines[4], block: 0 })).toBeUndefined();
    expect(imageAt({ line: lines[2], block: 0 })).toBeUndefined();
    const svg = {
      message: {
        content: [
          {
            type: "image",
            source: {
              type: "base64",
              media_type: "image/svg+xml",
              data: "PHN2Zz4=",
            },
          },
        ],
      },
    };
    expect(imageAt({ line: svg, block: 0 })).toBeUndefined();
    const url = {
      message: {
        content: [
          { type: "image", source: { type: "url", url: "https://x/y.png" } },
        ],
      },
    };
    expect(imageAt({ line: url, block: 0 })).toBeUndefined();
  });
});
