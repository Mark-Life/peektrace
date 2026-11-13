/** Empty session list: scanned folders per agent and a Settings link. */
import { describe, expect, test } from "bun:test";
import { Result } from "@effect-atom/atom-react";
import { Cause } from "effect";
import { renderToStaticMarkup } from "react-dom/server";
import type { ServerInfo } from "../src/lib/api-types.gen";
import { NoSessions } from "../src/routes/sessions/no-sessions";

const HOME = "/home/dev";

const root = (path: string, exists = true) => ({
  isDefault: true,
  home: path,
  path,
  exists,
});

const INFO: ServerInfo = {
  readOnly: false,
  userHome: HOME,
  agents: [
    { agent: "claude", roots: [root(`${HOME}/.claude/projects`)] },
    { agent: "codex", roots: [root(`${HOME}/.codex/sessions`, false)] },
    { agent: "pi", roots: [root(`${HOME}/.pi/agent/sessions`)] },
    { agent: "opencode", roots: [root("/srv/opencode")] },
  ],
};

const render = (info: Result.Result<ServerInfo, unknown>) =>
  renderToStaticMarkup(<NoSessions info={info} onRetry={() => undefined} />);

describe("NoSessions", () => {
  test("lists every agent's folder with ~ paths and a Settings link", () => {
    const html = render(Result.success(INFO));
    expect(html).toContain("No sessions found");
    for (const label of ["Claude", "Codex", "Pi", "OpenCode"]) {
      expect(html).toContain(`>${label}</span>`);
    }
    expect(html).toContain("~/.claude/projects");
    expect(html).toContain("~/.codex/sessions");
    expect(html).toContain("/srv/opencode");
    expect(html).not.toContain(`${HOME}/`);
    expect(html.match(/data-testid="root-missing"/g)).toHaveLength(1);
    expect(html).toContain('href="#/settings"');
    expect(html).toContain("looked for transcripts in these folders:");
  });

  test("shows skeleton lines while the folders load", () => {
    const html = render(Result.initial());
    expect(html).toContain('data-testid="scanned-pending"');
    expect(html).not.toContain('data-testid="scanned"');
    expect(html).toContain('href="#/settings"');
  });

  test("a failed folder list says so, offers a retry and keeps the link", () => {
    const html = render(Result.failure(Cause.fail("down")));
    expect(html).not.toContain('data-testid="scanned"');
    expect(html).not.toContain('data-testid="scanned-pending"');
    expect(html).not.toContain("these folders:");
    expect(html).toContain("could not list the folders it scans");
    expect(html).toContain('data-testid="scanned-retry"');
    expect(html).toContain('href="#/settings"');
  });
});
