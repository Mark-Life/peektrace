/** Settings: default scanned folder per agent and the read-only form. */
import { describe, expect, test } from "bun:test";
import { renderToStaticMarkup } from "react-dom/server";
import type { ServerInfo } from "../src/lib/api-types.gen";
import { SettingsEditor } from "../src/routes/settings-route";

const HOME = "/home/dev";

const info = (readOnly: boolean): ServerInfo => ({
  readOnly,
  userHome: HOME,
  agents: [
    {
      agent: "claude",
      roots: [
        {
          isDefault: true,
          home: `${HOME}/.claude`,
          path: `${HOME}/.claude/projects`,
          exists: true,
        },
        {
          isDefault: false,
          home: `${HOME}/work/.claude`,
          path: `${HOME}/work/.claude/projects`,
          exists: true,
        },
      ],
    },
    {
      agent: "codex",
      roots: [
        {
          isDefault: true,
          home: `${HOME}/.codex`,
          path: `${HOME}/.codex/sessions`,
          exists: false,
        },
      ],
    },
  ],
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
  /<fieldset aria-describedby="settings-readonly"[^>]*disabled=""/;

describe("SettingsEditor", () => {
  test("shows each default as a config dir, like the extra rows take", () => {
    const html = render(false);
    expect(html).toContain("~/.peektrace/settings.json");
    expect(html).toContain(
      '<code class="min-w-0 break-all text-muted-foreground">~/.claude</code>'
    );
    expect(html).toContain("scans <code>~/.claude/projects</code>");
    expect(html).toContain("scans <code>~/.codex/sessions</code>");
    expect(html).not.toContain("~/work/.claude/projects");
    expect(html.match(/data-testid="settings-default-root"/g)).toHaveLength(2);
    expect(html.match(/data-testid="root-missing"/g)).toHaveLength(1);
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
