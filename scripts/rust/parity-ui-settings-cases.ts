/** Inputs for the `ui` stage's Settings, theme and shortcut checks. */
import type {
  ApiError,
  ServerInfo,
} from "../../apps/inspector/src/lib/api-types.gen";
import type { RowsByAgent } from "../../apps/inspector/src/lib/settings-form";

const AGENTS = ["claude", "codex", "pi", "opencode"] as const;

const root = (
  isDefault: boolean,
  home: string,
  path: string,
  exists = true
) => ({
  isDefault,
  home,
  path,
  exists,
});

export const INFOS: ServerInfo[] = [
  {
    readOnly: false,
    userHome: "/home/dev",
    agents: AGENTS.map((agent, i) => ({
      agent,
      roots: [
        root(
          true,
          `/home/dev/.${agent}`,
          `/home/dev/.${agent}/projects`,
          i % 2 === 0
        ),
        root(
          false,
          `/home/dev/w/.${agent}`,
          `/home/dev/w/.${agent}/projects`,
          false
        ),
      ],
    })),
  },
  {
    readOnly: true,
    userHome: "/home/dev",
    agents: [
      {
        agent: "claude",
        roots: [root(true, "/home/dev/.claude", "/home/dev/.claude/projects")],
      },
      { agent: "codex", roots: [root(false, "/x", "/x/sessions")] },
      { agent: "opencode", roots: [root(true, "/o", "/o")] },
    ],
  },
  {
    readOnly: false,
    userHome: "C:\\Users\\dev\\",
    agents: [
      {
        agent: "pi",
        roots: [
          root(
            true,
            "C:\\Users\\dev\\.pi",
            "C:\\Users\\dev\\.pi\\agent\\sessions",
            false
          ),
        ],
      },
      { agent: "claude", roots: [] },
    ],
  },
  { readOnly: false, userHome: "", agents: [] },
];

export const SETTINGS: unknown[] = [
  {},
  { roots: {} },
  {
    roots: {
      codex: [{ path: "~/c", label: "work" }],
      claude: [{ path: "~/a" }],
    },
  },
  { roots: { bogus: [{ path: "~/x" }], pi: [{ path: "~/p", label: "" }] } },
  {
    roots: {
      claude: [
        { path: "  ~/a  " },
        { path: "\u00a0~/b\ufeff", label: " l " },
        { path: "\u0085~/c" },
        { path: "" },
        { path: "   ", label: "only label" },
      ],
    },
  },
  {
    roots: {
      claude: [{ path: "~/1" }],
      codex: [{ path: "~/2", label: "two" }],
      pi: [{ path: "~/3" }],
      opencode: [{ path: "~/4", label: "four words here" }],
    },
  },
];

const row = (path: string, label = "") => ({ path, label });

export const EDITED: RowsByAgent[] = [
  { claude: [], codex: [], pi: [], opencode: [] },
  {
    claude: [row(""), row("  ", "x")],
    codex: [row("\t~/c\n", " a b ")],
    pi: [],
    opencode: [],
  },
  {
    claude: [row("~/w/.claude", "work")],
    codex: [],
    pi: [row("\u2028~/p\u3000")],
    opencode: [row(" ", " ")],
  },
];

export const TILDE: [string, string][] = [
  ["/home/dev", "/home/dev"],
  ["/home/dev/.claude/projects", "/home/dev"],
  ["/home/devx/.claude", "/home/dev"],
  ["/var/data/sessions", "/home/dev"],
  ["/home/dev/.pi", "/home/dev/"],
  ["/a/b", ""],
  ["C:\\Users\\dev\\.claude\\projects", "C:\\Users\\dev"],
  ["C:\\Users\\dev\\.pi", "C:\\Users\\dev\\"],
  ["C:\\Users\\devx", "C:\\Users\\dev"],
  ["/x/y", "/"],
  ["/", "/"],
  ["", "/home/dev"],
  ["/home/dev//x", "/home/dev//"],
];

export const WIRE: ApiError[] = [
  { _tag: "FileChangedError", path: "p", reason: "mtime" },
  { _tag: "FileChangedError", path: "p", reason: "hash" },
  { _tag: "FileChangedError", path: "p", reason: "missing" },
  { _tag: "MemoryValidationError", reason: "bad name", name: "x" },
  { _tag: "MemoryNotFoundError", project: "proj", name: "note" },
  {
    _tag: "CapabilityUnsupportedError",
    capabilityId: "memory.crud",
    agentId: "codex",
  },
  { _tag: "PathOutsideRootError", path: "/etc", roots: ["/home"] },
  { _tag: "SessionNotFoundError", id: "abc", searchedRoot: "/r" },
  { _tag: "TranscriptParseError", path: "/t", reason: "EACCES" },
  {
    _tag: "SettingsWriteError",
    path: "settings.json",
    reason: "read-only mode is enabled; refusing to write",
  },
  { _tag: "BadRequestError", reason: "body needs settings" },
  { _tag: "ImageNotFoundError", id: "i", line: 1, block: 2 },
  { _tag: "NotFoundError", path: "/api/x" },
  { _tag: "MethodNotAllowedError", method: "DELETE" },
  { _tag: "StatsScanError", reason: "r" },
  { _tag: "StatsRowNotFoundError", key: "k", table: "t" },
];

const LOOKS = ["light", "dark", "system", "", "blue", null] as const;
export const THEME = LOOKS.flatMap((r) => LOOKS.map((s) => [r, s] as const));

export const KEY_EVENTS: [string, string, boolean][] = [
  ["j", "KeyJ", false],
  ["D", "KeyD", true],
  ["ArrowDown", "ArrowDown", false],
  ["о", "KeyJ", false],
  ["в", "KeyD", false],
  ["х", "BracketLeft", false],
  ["ъ", "BracketRight", false],
  ["?", "Slash", true],
  ["?", "Slash", false],
  ["/", "Slash", false],
  [".", "Slash", false],
  [",", "Slash", true],
  ["+", "BracketRight", false],
  ["]", "Digit6", false],
  ["/", "Digit7", true],
  ["z", "Slash", false],
  ["Z", "Slash", true],
  [".", "Period", false],
  ["F1", "F1", false],
  ["Escape", "Escape", false],
  ["😀", "KeyA", false],
  ["é", "Quote", false],
  [" ", "Space", false],
  ["ß", "Minus", false],
  ...["j", "k", "/", "[", "]", "d", "?"].flatMap((k) =>
    [false, true].map((shift) => [k, "", shift] as [string, string, boolean])
  ),
];

/** `[meta, ctrl, alt, altGraph, key, mac]`. */
export const ALLOWED: [boolean, boolean, boolean, boolean, string, boolean][] =
  [
    [false, false, false, false, "j", false],
    [true, false, false, false, "j", false],
    [false, true, false, false, "j", false],
    [false, false, true, false, "j", false],
    [false, true, true, true, "[", false],
    [false, false, true, false, "]", true],
    [false, false, true, false, "[", true],
    [false, false, true, false, "∂", true],
    [false, false, true, false, "]", false],
    [false, true, true, false, "]", true],
    [false, false, true, false, "j", true],
  ];
