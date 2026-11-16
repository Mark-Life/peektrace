/** Memory editing summary line, derived from the capability matrix. */
import { describe, expect, test } from "bun:test";
import type { Capability } from "../src/lib/api-types.gen";
import {
  memoryCrudVerdict,
  memorySupportSummary,
} from "../src/lib/memory-support";

const crud = (perAgent: Capability["perAgent"]): Capability => ({
  id: "memory.crud",
  group: "memory",
  title: "Memory CRUD",
  description: "",
  perAgent,
});

describe("memorySupportSummary", () => {
  test("names the supported agent and groups the rest by level", () => {
    expect(
      memorySupportSummary([
        crud({
          claude: { level: "supported" },
          codex: { level: "planned" },
          pi: { level: "planned" },
          opencode: { level: "unsupported" },
        }),
      ])
    ).toBe(
      "Memory editing: Claude. Planned: Codex, Pi. Not supported: OpenCode."
    );
  });

  test("updates when another agent gains support", () => {
    expect(
      memorySupportSummary([
        crud({
          claude: { level: "supported" },
          codex: { level: "supported" },
          pi: { level: "partial" },
          opencode: { level: "planned" },
        }),
      ])
    ).toBe("Memory editing: Claude, Codex. Partial: Pi. Planned: OpenCode.");
  });

  test("a missing capability row means no agent can edit", () => {
    expect(memorySupportSummary([])).toBe(
      "Memory editing: no agent yet. Not supported: Claude, Codex, Pi, OpenCode."
    );
  });
});

describe("memoryCrudVerdict", () => {
  test("follows the Claude cell", () => {
    const caps = [
      crud({
        claude: { level: "supported" },
        codex: { level: "planned" },
        pi: { level: "planned" },
        opencode: { level: "planned" },
      }),
    ];
    expect(memoryCrudVerdict({ caps, readOnly: false })).toMatchObject({
      blockedBy: null,
    });
  });

  test("blocks with the note when Claude is not supported", () => {
    const caps = [
      crud({
        claude: { level: "planned", note: "soon" },
        codex: { level: "supported" },
        pi: { level: "planned" },
        opencode: { level: "planned" },
      }),
    ];
    expect(memoryCrudVerdict({ caps, readOnly: false })).toEqual({
      level: "planned",
      note: "soon",
      blockedBy: "capability",
    });
  });

  test("a missing cell counts as unsupported", () => {
    expect(memoryCrudVerdict({ caps: [], readOnly: false }).level).toBe(
      "unsupported"
    );
  });

  test("a read-only server blocks even a supported cell", () => {
    const caps = [
      crud({
        claude: { level: "supported" },
        codex: { level: "planned" },
        pi: { level: "planned" },
        opencode: { level: "planned" },
      }),
    ];
    expect(memoryCrudVerdict({ caps, readOnly: true })).toMatchObject({
      blockedBy: "read-only",
    });
  });
});
