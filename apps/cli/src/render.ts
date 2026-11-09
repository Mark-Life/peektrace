/** Pure stdout rendering helpers: tidy fixed-width tables + small formatters.
 *
 * Kept side-effect-free (return strings) so commands stay declarative and the
 * formatters are unit-testable. No color codes — plain text for pipe-friendliness.
 */
import { fmtUsd } from "@workspace/core/services/sessions/cost";

const GAP = 2;
/** Narrowest the last column shrinks to when fitting a terminal. */
const MIN_LAST_COLUMN = 20;

/** Terminal columns `value` takes (wide characters count twice). */
const displayWidth = (value: string): number => Bun.stringWidth(value);

/** Right-pad `value` to `width` columns with spaces. */
const pad = (value: string, width: number): string => {
  const have = displayWidth(value);
  return have >= width ? value : value + " ".repeat(width - have);
};

/** Cut `value` to at most `max` columns, ending in an ellipsis when clipped. */
export const clipToWidth = (value: string, max: number): string => {
  if (displayWidth(value) <= max) {
    return value;
  }
  let out = "";
  let used = 0;
  for (const char of value) {
    const w = displayWidth(char);
    if (used + w > max - 1) {
      break;
    }
    out += char;
    used += w;
  }
  return `${out}…`;
};

/**
 * Tab-separated twin of {@link table}: one lowercased header line, then raw
 * tab-joined rows — no padding, no rule. Emitted when stdout is piped/captured
 * (an LLM tool call), stripping the alignment whitespace a human never sees.
 */
const compactTable = (
  headers: readonly string[],
  rows: readonly (readonly string[])[]
): string => {
  const head = headers.map((h) => h.toLowerCase()).join("\t");
  if (rows.length === 0) {
    return `${head}\n(none)`;
  }
  return [head, ...rows.map((row) => row.join("\t"))].join("\n");
};

/**
 * Render a text table. Columns size to their widest cell; an empty `rows`
 * yields a single "(none)" line under the header. Pass `compact` (set when
 * stdout is not a TTY) to emit the token-lean tab-separated form instead.
 */
export const table = (
  headers: readonly string[],
  rows: readonly (readonly string[])[],
  opts?: { readonly compact?: boolean; readonly maxWidth?: number }
): string => {
  if (opts?.compact) {
    return compactTable(headers, rows);
  }
  const widths = headers.map((header, col) =>
    rows.reduce(
      (max, row) => Math.max(max, displayWidth(row[col] ?? "")),
      displayWidth(header)
    )
  );
  const last = widths.length - 1;
  const room =
    opts?.maxWidth === undefined
      ? undefined
      : Math.max(
          MIN_LAST_COLUMN,
          opts.maxWidth -
            widths.slice(0, last).reduce((sum, w) => sum + w + GAP, 0)
        );
  if (room !== undefined && (widths[last] ?? 0) > room) {
    widths[last] = room;
  }
  const fit = (cell: string, col: number) =>
    col === last && room !== undefined ? clipToWidth(cell, room) : cell;
  const line = (cells: readonly string[]): string =>
    cells
      .map((cell, col) => pad(fit(cell, col), widths[col] ?? 0))
      .join("  ")
      .trimEnd();
  const head = line(headers);
  const rule = widths.map((w) => "-".repeat(w)).join("  ");
  if (rows.length === 0) {
    return `${head}\n${rule}\n(none)`;
  }
  return [head, rule, ...rows.map(line)].join("\n");
};

const KB = 1024;
const MB = KB * KB;
const THOUSAND = 1000;
const MILLION = 1_000_000;
const PERCENT = 100;
const DECIMALS = 1;

/** Human-readable byte size (1 KB = 1024 B). */
export const bytes = (n: number): string => {
  if (n < KB) {
    return `${n} B`;
  }
  if (n < MB) {
    return `${(n / KB).toFixed(DECIMALS)} KB`;
  }
  return `${(n / MB).toFixed(DECIMALS)} MB`;
};

/** Compact token count: `950`, `12.3k`, `32.4M`. */
export const tokens = (value: number): string => {
  const n = Math.round(value);
  if (n < THOUSAND) {
    return String(n);
  }
  return n < MILLION
    ? `${(n / THOUSAND).toFixed(DECIMALS)}k`
    : `${(n / MILLION).toFixed(DECIMALS)}M`;
};

/** An estimated USD figure: `~$1.23`, `<$0.01` below a cent, `-` when absent. */
export const usdCell = (n: number | undefined): string =>
  n === undefined ? "-" : fmtUsd(n);

/** Dumb-zone cross turn for display: 1-based like the inspector, `never` for -1. */
export const crossTurnCell = (n: number): string =>
  n < 0 ? "never" : String(n + 1);

const SEC_PER_MIN = 60;
const SEC_PER_HOUR = 3600;
const PAD = 2;

/** Duration as `1h04m`, `4m05s` or `2.3s`. Approximate by construction. */
export const hms = (seconds: number): string => {
  const total = Math.max(0, Math.round(seconds));
  if (total < SEC_PER_MIN) {
    return `${seconds < SEC_PER_MIN ? seconds.toFixed(DECIMALS) : String(total)}s`;
  }
  const pad = (n: number) => String(n).padStart(PAD, "0");
  if (total < SEC_PER_HOUR) {
    return `${Math.floor(total / SEC_PER_MIN)}m${pad(total % SEC_PER_MIN)}s`;
  }
  return `${Math.floor(total / SEC_PER_HOUR)}h${pad(
    Math.floor((total % SEC_PER_HOUR) / SEC_PER_MIN)
  )}m`;
};

/** Percentage string from a 0..1 fraction. */
export const percent = (fraction: number): string =>
  `${(fraction * PERCENT).toFixed(DECIMALS)}%`;

/** First segment of a UUID-ish id, for narrow table columns. */
export const shortId = (id: string): string => id.split("-")[0] ?? id;

/** Pretty-printed JSON for `--json` output. */
export const json = (value: unknown): string => JSON.stringify(value, null, 2);

/** Notice for listed transcripts that failed to read. */
export const unreadableNotice = (n: number) =>
  `${n} ${n === 1 ? "session" : "sessions"} could not be read`;
