/** Project picker options: Claude-only, deduped, server labels, recency order. */
import { describe, expect, test } from "bun:test";
import { memoryProjectOptions } from "../src/routes/memory/project-options";

const PREFIX = "-home-dev-code-";

const label = (name: string) => ({ slug: `${PREFIX}${name}`, project: name });

const header = (
  project: string,
  updatedAt?: string,
  agent: "claude" | "codex" | "pi" | "opencode" = "claude"
) => ({ agent, project, ...(updatedAt ? { updatedAt } : {}) });

describe("memoryProjectOptions", () => {
  test("empty input gives no options", () => {
    expect(memoryProjectOptions({ headers: [], labels: [] })).toEqual([]);
  });

  test("dedupes a slug across sessions and keeps the newest date", () => {
    const options = memoryProjectOptions({
      headers: [
        header(`${PREFIX}alpha`, "2026-01-01T00:00:00Z"),
        header(`${PREFIX}alpha`, "2026-03-01T00:00:00Z"),
        header(`${PREFIX}alpha`),
        header(`${PREFIX}beta`, "2026-02-01T00:00:00Z"),
      ],
      labels: [label("alpha"), label("beta")],
    });
    expect(options.map((o) => o.slug)).toEqual([
      `${PREFIX}alpha`,
      `${PREFIX}beta`,
    ]);
    expect(options[0]?.lastActive).toBe("2026-03-01T00:00:00Z");
    expect(options.map((o) => o.label)).toEqual(["alpha", "beta"]);
  });

  test("ignores non-Claude sessions", () => {
    const options = memoryProjectOptions({
      headers: [
        header("codex-project", "2026-05-01T00:00:00Z", "codex"),
        header("pi-project", "2026-05-01T00:00:00Z", "pi"),
        header("oc-project", "2026-05-01T00:00:00Z", "opencode"),
        header(`${PREFIX}alpha`, "2026-01-01T00:00:00Z"),
      ],
      labels: [],
    });
    expect(options.map((o) => o.slug)).toEqual([`${PREFIX}alpha`]);
  });

  test("includes projects with no session and uses the server label", () => {
    const options = memoryProjectOptions({
      headers: [header(`${PREFIX}alpha`, "2026-01-01T00:00:00Z")],
      labels: [
        { slug: `${PREFIX}alpha`, project: "Alpha App" },
        { slug: `${PREFIX}gamma`, project: "gamma" },
      ],
    });
    expect(options).toEqual([
      {
        slug: `${PREFIX}alpha`,
        label: "Alpha App",
        lastActive: "2026-01-01T00:00:00Z",
      },
      { slug: `${PREFIX}gamma`, label: "gamma" },
    ]);
  });

  test("puts undated projects last, sorted by label", () => {
    const options = memoryProjectOptions({
      headers: [header(`${PREFIX}zeta`), header(`${PREFIX}eta`)],
      labels: [label("zeta"), label("eta"), label("theta")],
    });
    expect(options.map((o) => o.label)).toEqual(["eta", "theta", "zeta"]);
  });

  test("falls back to the slug when the server has no label yet", () => {
    const options = memoryProjectOptions({
      headers: [header(`${PREFIX}new-app`, "2026-01-01T00:00:00Z")],
      labels: [],
    });
    expect(options.map((o) => o.label)).toEqual([`${PREFIX}new-app`]);
  });

  test("labels session projects with the server label", () => {
    const options = memoryProjectOptions({
      headers: [header(`${PREFIX}web`, "2026-02-01T00:00:00Z")],
      labels: [
        { slug: `${PREFIX}web`, project: "code-web" },
        { slug: `${PREFIX}api`, project: "code-api" },
      ],
    });
    expect(options.map((o) => o.label)).toEqual(["code-web", "code-api"]);
  });
});
