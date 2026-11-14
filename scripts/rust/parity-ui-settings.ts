/** The `ui` stage's Settings, Capabilities, theme and shortcut checks: the
 * app's rules (`ui_helpers` probe) against the inspector's own functions,
 * and the app's copy against the inspector's rendered markup and sources.
 *
 * The real case reads `~/.peektrace/settings.json` read-only and never
 * prints it: a diff names the field only. */
import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { createElement } from "../../apps/inspector/node_modules/react";
import { renderToStaticMarkup } from "../../apps/inspector/node_modules/react-dom/server";
import {
  GROUP_NOTE,
  GROUPS,
  ShortcutsList,
} from "../../apps/inspector/src/components/shortcuts-dialog";
import type {
  Capability,
  ServerInfo,
} from "../../apps/inspector/src/lib/api-types.gen";
import {
  AGENT_COLUMNS,
  cellNote,
  groupBy,
  LEVEL_GLOSS,
  LEVELS,
} from "../../apps/inspector/src/lib/capability-matrix";
import { tildePath } from "../../apps/inspector/src/lib/paths";
import {
  assemble,
  defaultRoots,
  type RowsByAgent,
  seedRows,
} from "../../apps/inspector/src/lib/settings-form";
import {
  keyLabel,
  keyOf,
  SHORTCUTS,
  shortcutAllowed,
} from "../../apps/inspector/src/lib/shortcuts";
import { nextThemePress } from "../../apps/inspector/src/lib/theme-modes";
import { wireErrorMessage } from "../../apps/inspector/src/lib/wire-error";
import { SettingsEditor } from "../../apps/inspector/src/routes/settings-route";
import { seededCapabilities as CAPABILITIES } from "../../packages/core/src/services/capabilities";
import {
  type PeektraceSettings,
  parseSettings,
} from "../../packages/core/src/services/settings";
import { report } from "./parity-lib";
import { digest } from "./parity-oracle";
import { pipe } from "./parity-tui";
import { compare, tsAnswer } from "./parity-ui-compare";

const INSPECTOR = join(import.meta.dir, "../../apps/inspector/src");

import {
  ALLOWED,
  EDITED,
  INFOS,
  KEY_EVENTS,
  SETTINGS,
  THEME,
  TILDE,
  WIRE,
} from "./parity-ui-settings-cases";

const keysAnswer = () => {
  const labels = (mac: boolean) =>
    Object.fromEntries(
      SHORTCUTS.flatMap((s) => s.keys).map((key) => [
        key,
        keyLabel({ key, mac }),
      ])
    );
  return {
    table: SHORTCUTS,
    labels: { mac: labels(true), other: labels(false) },
    groups: GROUPS,
    notes: GROUP_NOTE,
    keyOf: KEY_EVENTS.map(([key, code, shiftKey]) =>
      keyOf({ key, code, shiftKey })
    ),
    allowed: ALLOWED.map(([metaKey, ctrlKey, altKey, altGraph, key, mac]) =>
      shortcutAllowed(
        {
          altKey,
          code: "",
          ctrlKey,
          defaultPrevented: false,
          getModifierState: (k) => altGraph && k === "AltGraph",
          key,
          metaKey,
          shiftKey: false,
          target: null,
        },
        mac
      )
    ),
  };
};

const capsAnswer = (caps: readonly Capability[]) => ({
  columns: AGENT_COLUMNS.map((c) => [c.id, c.label]),
  levels: LEVELS,
  gloss: LEVEL_GLOSS,
  // The pill text is the level under CSS `capitalize`.
  labels: Object.fromEntries(
    LEVELS.map((l) => [l, l[0]?.toUpperCase() + l.slice(1)])
  ),
  groups: groupBy(caps).map(([g, rows]) => [g, rows.map((c) => c.id)]),
  notes: Object.fromEntries(
    caps.map((c) => [
      c.id,
      Object.fromEntries(
        AGENT_COLUMNS.map((a) => [a.id, cellNote(c.perAgent[a.id])])
      ),
    ])
  ),
});

const settingsCase = (
  settings: unknown,
  rows: RowsByAgent,
  info: ServerInfo
) => {
  const s = settings as PeektraceSettings;
  return {
    seed: seedRows(s),
    assembled: assemble(seedRows(s)),
    assembledEdited: assemble(rows),
    defaults: [...defaultRoots(info).entries()],
  };
};

const ENTITIES: Record<string, string> = {
  "&amp;": "&",
  "&lt;": "<",
  "&gt;": ">",
  "&quot;": '"',
  "&#x27;": "'",
  "&#39;": "'",
};

const decode = (html: string) =>
  html.replace(/&(?:amp|lt|gt|quot|#x27|#39);/g, (m) => ENTITIES[m] ?? m);

const collapse = (s: string) => s.replace(/\s+/g, " ");

/** Everything the inspector shows for these pages: rendered markup (tags
 * kept for attributes, and stripped for text) plus the route sources,
 * whitespace collapsed, for text only a click reveals. */
const copyHaystack = () => {
  const info = INFOS[0] as ServerInfo;
  const editor = (readOnly: boolean) =>
    renderToStaticMarkup(
      createElement(SettingsEditor, {
        info: { ...info, readOnly },
        mtimeMs: 0,
        path: "/home/dev/.peektrace/settings.json",
        settings: { roots: { claude: [{ path: "~/a" }] } },
      })
    );
  const html = [
    editor(false),
    editor(true),
    renderToStaticMarkup(createElement(ShortcutsList, { mac: false })),
  ]
    .map(decode)
    .join("\n");
  const sources = [
    "routes/settings-route.tsx",
    "routes/capabilities-route.tsx",
    "lib/result-view.tsx",
    "components/shortcuts-dialog.tsx",
    "components/read-only.tsx",
    "lib/agent-label.ts",
  ].map((f) => collapse(readFileSync(join(INSPECTOR, f), "utf8")));
  return [html, collapse(html.replace(/<[^>]*>/g, "")), ...sources].join("\n");
};

/** Checks run, for the stage summary. */
export interface SettingsCounts {
  caps: number;
  keys: number;
  settings: number;
}

const ask = (bin: string, reqs: readonly object[]) => {
  const out = pipe(
    bin,
    reqs.map((r) => JSON.stringify(r))
  );
  return out?.map((l) => JSON.parse(l) as unknown) ?? null;
};

const realCase = (bin: string, counts: SettingsCounts) => {
  const path = join(homedir(), ".peektrace", "settings.json");
  if (!existsSync(path)) {
    return;
  }
  const settings = parseSettings(readFileSync(path, "utf8"));
  const empty: RowsByAgent = { claude: [], codex: [], pi: [], opencode: [] };
  const req = {
    fn: "settings",
    cases: [{ settings, rows: empty, info: INFOS[3] }],
  };
  const rust = ask(bin, [req]);
  const ts = tsAnswer(`ui settings real ${digest(req)}`, () => [
    settingsCase(settings, empty, INFOS[3] as ServerInfo),
  ]);
  if (rust && ts !== undefined) {
    compare("ui settings real", { cases: ts }, { cases: rust[0] }, false);
    counts.settings++;
  }
};

export const runSettingsChecks = (
  bin: string,
  real: boolean
): SettingsCounts => {
  const counts: SettingsCounts = { caps: 0, keys: 0, settings: 0 };
  const cases = SETTINGS.map((settings, i) => ({
    settings,
    rows: EDITED[i % EDITED.length],
    info: INFOS[i % INFOS.length],
  }));
  const reqs = [
    { fn: "settings", cases },
    { fn: "tilde", pairs: TILDE },
    { fn: "wire", errors: WIRE },
    { fn: "caps", caps: CAPABILITIES },
    { fn: "caps", caps: [] },
    { fn: "theme", pairs: THEME },
    { fn: "keys", events: KEY_EVENTS, allowed: ALLOWED },
    { fn: "copy" },
  ];
  const rust = ask(bin, reqs);
  if (!rust) {
    return counts;
  }
  const [
    settingsReq,
    tildeReq,
    wireReq,
    capsReq,
    emptyCapsReq,
    themeReq,
    keysReq,
  ] = reqs;
  const answers: [string, object | undefined, () => unknown][] = [
    [
      "settings",
      settingsReq,
      () =>
        cases.map((c) =>
          settingsCase(c.settings, c.rows as RowsByAgent, c.info as ServerInfo)
        ),
    ],
    [
      "tilde",
      tildeReq,
      () => TILDE.map(([path, home]) => tildePath({ path, home })),
    ],
    ["wire", wireReq, () => WIRE.map(wireErrorMessage)],
    [
      "caps",
      capsReq,
      () => capsAnswer(CAPABILITIES as unknown as Capability[]),
    ],
    ["caps empty", emptyCapsReq, () => capsAnswer([])],
    [
      "theme",
      themeReq,
      () =>
        THEME.map(([resolvedTheme, systemTheme]) =>
          nextThemePress({
            resolvedTheme: resolvedTheme ?? undefined,
            systemTheme: systemTheme ?? undefined,
          })
        ),
    ],
    ["keys", keysReq, keysAnswer],
  ];
  answers.forEach(([label, req, answer], i) => {
    const ts = tsAnswer(`ui ${label} ${digest(req)}`, answer);
    if (ts !== undefined) {
      compare(`ui ${label}`, { v: ts }, { v: rust[i] }, true);
    }
  });
  counts.settings += cases.length + TILDE.length + WIRE.length + THEME.length;
  counts.caps += CAPABILITIES.length * AGENT_COLUMNS.length + LEVELS.length;
  counts.keys += SHORTCUTS.length + KEY_EVENTS.length + ALLOWED.length;

  const copy = rust[reqs.length - 1] as
    | { native: string[]; web: string[] }
    | undefined;
  const web = copy?.web ?? [];
  const found = tsAnswer(`ui copy ${digest(web)}`, () => {
    const hay = copyHaystack();
    return web.map((s) => hay.includes(s));
  }) as boolean[] | undefined;
  found?.forEach((ok, i) => {
    if (!ok) {
      report(`ui copy: the inspector has no "${web[i]}"`);
    }
  });
  counts.settings += web.length;
  if (real) {
    realCase(bin, counts);
  }
  return counts;
};
