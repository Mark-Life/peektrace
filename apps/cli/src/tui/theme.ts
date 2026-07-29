/** Terminal palette + pure formatters for the TUI.
 *
 * Dark-first, dense, forensic — the same posture as the web inspector, retuned
 * for a terminal. Colors are hex strings OpenTUI accepts directly on `fg`/`bg`/
 * `borderColor`. Formatters mirror the inspector's `session-format` helpers so
 * the two surfaces render identical numbers.
 */
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";

/** Core palette. */
export const C = {
  bg: "#0b0e14",
  panel: "#11151c",
  panelSel: "#182030",
  border: "#232a36",
  borderActive: "#3d6fd0",
  text: "#c9d1d9",
  textDim: "#8b949e",
  textFaint: "#6e7681",
  primary: "#6aa0ff",
  accent: "#39d0d8",
  good: "#3fb950",
  warn: "#d29922",
  bad: "#f85149",
  info: "#58a6ff",
  onPrimary: "#0b0e14",
} as const;

/** Support-level → color, matching the capabilities matrix legend. */
export const LEVEL_COLOR = {
  supported: C.good,
  partial: C.warn,
  planned: C.info,
  unsupported: C.textFaint,
} as const;

/** Plain-language gloss per support level (legend + cell notes). */
export const LEVEL_GLOSS = {
  supported: "Built and working today.",
  partial: "Partially built — some paths work.",
  planned: "On the roadmap, not yet built.",
  unsupported: "Not applicable / no plan.",
} as const;

/** Display label per agent id. */
export const AGENT_LABEL: Record<string, string> = {
  "claude-code": "Claude",
  claude: "Claude",
  codex: "Codex",
  pi: "Pi",
  opencode: "OpenCode",
};

/** Accent color per agent, for badges. */
export const AGENT_COLOR: Record<string, string> = {
  "claude-code": "#cc785c",
  claude: "#cc785c",
  codex: "#10a37f",
  pi: "#b08cff",
  opencode: "#f2b705",
};

/** Session provider → display label (verdict header). */
export const PROVIDER_LABEL: Record<string, string> = {
  "claude-code": "Claude Code",
  codex: "Codex",
  pi: "Pi",
  opencode: "OpenCode",
};

const THOUSAND = 1000;
const MILLION = THOUSAND * THOUSAND;
const KB = 1024;
const MB = KB * KB;
const PERCENT = 100;

/** Thousands-separated rounded integer (e.g. 12_345 -> "12,345"). */
export const fmt = (n: number): string => Math.round(n).toLocaleString("en-US");

/** Compact magnitude (e.g. 12_345 -> "12.3K", 1_200_000 -> "1.2M"). */
export const fmtK = (n: number): string => {
  if (Math.abs(n) >= MILLION) {
    return `${(n / MILLION).toFixed(1)}M`;
  }
  if (Math.abs(n) >= THOUSAND) {
    return `${(n / THOUSAND).toFixed(1)}K`;
  }
  return String(Math.round(n));
};

/** Percentage from a 0..1 fraction: 1 decimal under 10%, integer above. */
export const fmtPct = (fraction: number): string => {
  const pct = fraction * PERCENT;
  return pct < 10 ? `${pct.toFixed(1)}%` : `${Math.round(pct)}%`;
};

/** Human-readable byte size (1 KB = 1024 B). */
export const fmtBytes = (n: number): string => {
  if (n < KB) {
    return `${n} B`;
  }
  if (n < MB) {
    return `${(n / KB).toFixed(1)} KB`;
  }
  return `${(n / MB).toFixed(1)} MB`;
};

/** `YYYY-MM-DDTHH:MM` length — the slice shown when the ISO regex misses. */
const ISO_MINUTE_LEN = 16;
/** Dumb-zone floor below which the warn threshold is pinned at 75%. */
const WARN_FLOOR = 0.75;

const STARTED_RE = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}:\d{2})/;
const MONTHS = [
  "Jan",
  "Feb",
  "Mar",
  "Apr",
  "May",
  "Jun",
  "Jul",
  "Aug",
  "Sep",
  "Oct",
  "Nov",
  "Dec",
];

/**
 * Format a session start time read literally off the ISO string (never
 * Date-parsed), matching the inspector: `2026-07-09T08:18…` -> `Jul 9, 08:18`.
 */
export const fmtStarted = (iso: string | undefined): string => {
  if (iso === undefined) {
    return "—";
  }
  const m = STARTED_RE.exec(iso);
  if (m === null) {
    return iso.slice(0, ISO_MINUTE_LEN);
  }
  const month = MONTHS[Number(m[2]) - 1] ?? m[2];
  return `${month} ${Number(m[3])}, ${m[4]}`;
};

// Terminal-corrupting byte strippers. Tool output routinely carries raw ANSI
// color codes and other C0 control bytes; OpenTUI's text buffer renders those as
// "ghost" glyphs, so every server-authored string is run through `sanitize`
// before display. Built via `RegExp` from unicode escapes so no literal control
// character appears in source.
// Patterns are built through this indirection (not literals) so the control
// bytes stay as `\u` escapes — avoiding both a literal-control-char regex and
// the "prefer a regex literal" lint, which would otherwise conflict here.
const re = (pattern: string) => new RegExp(pattern, "g");
const ANSI_CSI = re("\\u001b\\[[0-9;?]*[ -/]*[@-~]");
const ANSI_OSC = re("\\u001b\\][^\\u0007\\u001b]*(?:\\u0007|\\u001b\\\\)");
const ANSI_ESC = re("\\u001b[@-Z\\\\-_]");
const C0_CONTROL = re("[\\u0000-\\u0008\\u000b\\u000c\\u000e-\\u001f\\u007f]");
const CR = re("\\r\\n?");
const TAB = re("\\t");

/**
 * Strip terminal escape sequences and control bytes from server text: ANSI
 * CSI/OSC/other escapes, C0 controls (keeping newlines), CR→LF, tabs→spaces.
 * Everything shown in the terminal passes through here first.
 */
export const sanitize = (text: string): string =>
  text
    .replace(ANSI_OSC, "")
    .replace(ANSI_CSI, "")
    .replace(ANSI_ESC, "")
    .replace(CR, "\n")
    .replace(TAB, "  ")
    .replace(C0_CONTROL, "");

/** First line of a string, sanitized, trimmed to `max` chars with an ellipsis. */
export const firstLine = (text: string, max = 120): string => {
  const line = (sanitize(text).split("\n")[0] ?? "").trim();
  return line.length > max ? `${line.slice(0, max - 1)}…` : line;
};

/** Truncate to `max` chars with a trailing ellipsis when clipped. */
export const clip = (text: string, max: number): string =>
  text.length > max ? `${text.slice(0, Math.max(0, max - 1))}…` : text;

/** Peak-context health zone, mirroring the inspector's `zoneOf`. */
export type HealthZone = "ok" | "warn" | "bad";

/** Classify a session's health from peak fraction vs the dumb-zone floor. */
export const zoneOf = (session: AnalyzedSession): HealthZone => {
  const frac =
    session.contextWindow > 0
      ? session.peakContextTokens / session.contextWindow
      : 0;
  const dumbZone = session.dumbZoneFraction;
  const warn = dumbZone < WARN_FLOOR ? WARN_FLOOR : (1 + dumbZone) / 2;
  if (frac < dumbZone) {
    return "ok";
  }
  return frac < warn ? "warn" : "bad";
};

/** Verdict word + color per health zone. */
export const ZONE_VERDICT: Record<
  HealthZone,
  { readonly label: string; readonly color: string }
> = {
  ok: { label: "Healthy", color: C.good },
  warn: { label: "Degrading", color: C.warn },
  bad: { label: "Rotting", color: C.bad },
};

/**
 * A horizontal bar built from block glyphs, filled to `fraction` (0..1) of
 * `width` cells. Used for gauges where a nested flex box would be overkill.
 */
export const barGlyphs = (fraction: number, width: number): string => {
  const filled = Math.max(0, Math.min(width, Math.round(fraction * width)));
  return "█".repeat(filled) + "░".repeat(Math.max(0, width - filled));
};
