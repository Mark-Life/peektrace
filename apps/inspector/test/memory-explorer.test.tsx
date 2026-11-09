/** Memory page: empty state with a create action, no agent selector. */
import { describe, expect, test } from "bun:test";
import type { Capability } from "@workspace/rpc/contract";
import { renderToStaticMarkup } from "react-dom/server";
import { Explorer } from "../src/routes/memory/memory-explorer";

const caps = (claude: Capability["perAgent"]["claude"]["level"]) => [
  {
    id: "memory.crud",
    group: "memory",
    title: "Memory CRUD",
    description: "",
    perAgent: {
      claude: { level: claude },
      codex: { level: "planned" },
      pi: { level: "planned" },
      opencode: { level: "planned" },
    },
  } satisfies Capability,
];

const EMPTY = { projects: [], vaults: [], labels: [] };

describe("Explorer with no memory folders", () => {
  test("offers New memory in… and agent-neutral copy", () => {
    const html = renderToStaticMarkup(
      <Explorer all={EMPTY} caps={caps("supported")} readOnly={false} />
    );
    expect(html).toContain('data-testid="memory-empty"');
    expect(html).toContain('data-testid="create-anywhere"');
    expect(html).toContain("No memories yet");
    expect(html).toContain("Create the first memory");
    expect(html).not.toContain("Claude projects root");
    expect(html).not.toContain('data-testid="memory-search"');
    expect(html).not.toContain('data-testid="agent-select"');
    expect(html).toContain(
      "Memory editing: Claude. Planned: Codex, Pi, OpenCode."
    );
    expect(html).not.toContain('data-testid="capability-banner"');
  });

  test("hides the create action and shows the banner when editing is blocked", () => {
    const html = renderToStaticMarkup(
      <Explorer all={EMPTY} caps={caps("planned")} readOnly={false} />
    );
    expect(html).toContain('data-testid="memory-empty"');
    expect(html).not.toContain('data-testid="create-anywhere"');
    expect(html).not.toContain("Create the first memory");
    expect(html).toContain("Memories appear here once Claude saves one.");
    expect(html).toContain('data-testid="capability-banner"');
    expect(html).toContain("Memory editing is planned");
    expect(html).not.toContain('data-testid="memory-support"');
    expect(html).toContain("Memory editing: no agent yet.");
  });

  test("a read-only server hides create and explains why", () => {
    const html = renderToStaticMarkup(
      <Explorer all={EMPTY} caps={caps("supported")} readOnly />
    );
    expect(html).not.toContain('data-testid="create-anywhere"');
    expect(html).toContain('data-testid="capability-banner"');
    expect(html).toContain("Read-only server");
    expect(html).toContain("<code>--read-only</code>");
    expect(html).not.toContain("Memory editing is");
  });
});
