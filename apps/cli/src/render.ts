/** Pure stdout rendering helpers: tidy fixed-width tables + small formatters.
 *
 * Kept side-effect-free (return strings) so commands stay declarative and the
 * formatters are unit-testable. No color codes — plain text for pipe-friendliness.
 */

/** Right-pad `value` to `width` with spaces. */
const pad = (value: string, width: number): string =>
  value.length >= width ? value : value + " ".repeat(width - value.length);

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
  opts?: { readonly compact?: boolean }
): string => {
  if (opts?.compact) {
    return compactTable(headers, rows);
  }
  const widths = headers.map((header, col) =>
    rows.reduce(
      (max, row) => Math.max(max, (row[col] ?? "").length),
      header.length
    )
  );
  const line = (cells: readonly string[]): string =>
    cells
      .map((cell, col) => pad(cell, widths[col] ?? 0))
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

/** Compact token count (e.g. 12_345 -> "12.3k"). */
export const tokens = (n: number): string =>
  n < THOUSAND ? String(n) : `${(n / THOUSAND).toFixed(DECIMALS)}k`;

/** Percentage string from a 0..1 fraction. */
export const percent = (fraction: number): string =>
  `${(fraction * PERCENT).toFixed(DECIMALS)}%`;

/** First segment of a UUID-ish id, for narrow table columns. */
export const shortId = (id: string): string => id.split("-")[0] ?? id;

/** Pretty-printed JSON for `--json` output. */
export const json = (value: unknown): string => JSON.stringify(value, null, 2);
