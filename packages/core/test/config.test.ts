import { describe, expect, test } from "bun:test";
import { parseConfig } from "../src/services/config";

describe("parseConfig", () => {
  test("empty / blank text yields the empty config", () => {
    expect(parseConfig("")).toEqual({});
    expect(parseConfig("   \n ")).toEqual({});
  });

  test("malformed JSON falls back to the empty config", () => {
    expect(parseConfig("{ not json")).toEqual({});
  });

  test("parses per-agent extra roots with optional labels", () => {
    const cfg = parseConfig(
      JSON.stringify({
        roots: {
          claude: [
            { path: "~/work/.claude", label: "work" },
            { path: "/abs/.claude" },
          ],
        },
      })
    );
    expect(cfg.roots?.claude).toEqual([
      { path: "~/work/.claude", label: "work" },
      { path: "/abs/.claude" },
    ]);
  });

  test("ignores an unknown agent key", () => {
    const cfg = parseConfig(JSON.stringify({ roots: { bogus: [] } }));
    expect(cfg.roots?.claude).toBeUndefined();
    expect(
      (cfg.roots as Record<string, unknown> | undefined)?.bogus
    ).toBeUndefined();
  });

  test("rejects a root entry missing `path` → empty", () => {
    expect(
      parseConfig(JSON.stringify({ roots: { claude: [{ label: "x" }] } }))
    ).toEqual({});
  });

  test("salvages a valid agent when another agent's value is malformed", () => {
    const cfg = parseConfig(
      JSON.stringify({
        roots: { claude: [{ path: "/a" }], codex: "not-an-array" },
      })
    );
    expect(cfg.roots?.claude).toEqual([{ path: "/a" }]);
    expect(cfg.roots?.codex).toBeUndefined();
  });

  test("drops a single bad entry but keeps valid ones in the same agent", () => {
    const cfg = parseConfig(
      JSON.stringify({
        roots: { claude: [{ path: "/good" }, { bad: true }, { path: "/ok" }] },
      })
    );
    expect(cfg.roots?.claude).toEqual([{ path: "/good" }, { path: "/ok" }]);
  });
});
