/** Settings: default scanned folder per agent and the read-only form. */
import { describe, expect, test } from "bun:test";

import type { ServerInfo } from "@workspace/view-core/api-types.gen";
import { renderToStaticMarkup } from "react-dom/server";

import { SettingsEditor } from "../src/routes/settings-route";

const HOME = "/home/dev";

const info = (readOnly: boolean): ServerInfo => ({
  agents: [
    {
      agent: "claude",
      roots: [
        {
          exists: true,
          home: `${HOME}/.claude`,
          isDefault: true,
          path: `${HOME}/.claude/projects`,
        },
        {
          exists: true,
          home: `${HOME}/work/.claude`,
          isDefault: false,
          path: `${HOME}/work/.claude/projects`,
        },
      ],
    },
    {
      agent: "codex",
      roots: [
        {
          exists: false,
          home: `${HOME}/.codex`,
          isDefault: true,
          path: `${HOME}/.codex/sessions`,
        },
      ],
    },
  ],
  readOnly,
  userHome: HOME,
});

const render = (readOnly: boolean) =>
  renderToStaticMarkup(
    <SettingsEditor
      info={info(readOnly)}
      mtimeMs={0}
      path={`${HOME}/.peektrace/settings.json`}
      settings={{}}
    />
  );

const FIELDSET_DISABLED =
  /<fieldset aria-describedby="settings-readonly"[^>]*disabled=""/u;

describe("SettingsEditor", () => {
  test("shows each default as a config dir, like the extra rows take", () => {
    const html = render(false);
    expect(html).toContain("~/.peektrace/settings.json");
    expect(html).toContain(
      '<code class="text-muted-foreground min-w-0 break-all">~/.claude</code>'
    );
    expect(html).toContain("scans <code>~/.claude/projects</code>");
    expect(html).toContain("scans <code>~/.codex/sessions</code>");
    expect(html).not.toContain("~/work/.claude/projects");
    expect(html.match(/data-testid="settings-default-root"/gu)).toHaveLength(2);
    expect(html.match(/data-testid="root-missing"/gu)).toHaveLength(1);
    expect(html).not.toContain(`${HOME}/`);
  });

  test("an editable server leaves the form enabled", () => {
    const html = render(false);
    expect(html).not.toContain('data-testid="settings-readonly"');
    expect(html).not.toMatch(FIELDSET_DISABLED);
  });

  test("a read-only server disables the form and says why", () => {
    const html = render(true);
    expect(html).toContain('id="settings-readonly"');
    expect(html).toContain("Settings can&#x27;t be saved from here.");
    expect(html).toMatch(FIELDSET_DISABLED);
  });
});
