/** Line diff for the before/after strings an edit tool records.
 *
 * Pure and dependency-free, next to the shared highlighter so both the web
 * inspector and the CLI can render an `Edit` the way a review tool does: one
 * block, `-` and `+` gutters, unchanged lines for context.
 *
 * Line numbers count from 1 *within the snippet*. A tool call records the text
 * it replaced, never where in the file it sat, so numbering them any other way
 * would invent an offset the transcript does not have.
 */

/** What a row does to the file: unchanged, removed, added, or lines skipped. */
export type DiffRowKind = "add" | "ctx" | "del" | "gap";

export interface DiffRow {
  /** Unchanged lines a gap row stands in for. */
  readonly count?: number;
  readonly kind: DiffRowKind;
  /** 1-based line in the after text, for `ctx` and `add`. */
  readonly newNo?: number;
  /** 1-based line in the before text, for `ctx` and `del`. */
  readonly oldNo?: number;
  readonly text: string;
}

export interface LineDiff {
  readonly added: number;
  readonly removed: number;
  readonly rows: readonly DiffRow[];
}

/** Unchanged lines kept either side of a change. */
const CONTEXT = 3;

/**
 * Ceiling on the LCS table. Past it the diff falls back to a whole-block
 * replace: the table is quadratic, and a 2 MB `old_string` would otherwise
 * freeze the tab it renders in.
 */
const MAX_CELLS = 1_000_000;

const splitLines = (s: string) => (s === "" ? [] : s.split("\n"));

/** Every removed line, then every added one — the diff of last resort. */
const replaceAll = (before: string[], after: string[]): LineDiff => ({
  added: after.length,
  removed: before.length,
  rows: [
    ...before.map((text, i) => ({ kind: "del" as const, oldNo: i + 1, text })),
    ...after.map((text, i) => ({ kind: "add" as const, newNo: i + 1, text })),
  ],
});

/** Longest-common-subsequence lengths, filled from the end backwards. */
const lcsTable = (before: string[], after: string[]) => {
  const width = after.length + 1;
  const table = new Int32Array((before.length + 1) * width);
  for (let i = before.length - 1; i >= 0; i--) {
    for (let j = after.length - 1; j >= 0; j--) {
      const here = i * width + j;
      table[here] =
        before[i] === after[j]
          ? (table[here + width + 1] ?? 0) + 1
          : Math.max(table[here + width] ?? 0, table[here + 1] ?? 0);
    }
  }
  return { table, width };
};

/** Walk the table forwards, emitting one row per line of either side. */
const alignRows = (before: string[], after: string[]): DiffRow[] => {
  const { table, width } = lcsTable(before, after);
  const rows: DiffRow[] = [];
  let i = 0;
  let j = 0;
  while (i < before.length && j < after.length) {
    const a = before[i] ?? "";
    const b = after[j] ?? "";
    if (a === b) {
      rows.push({ kind: "ctx", oldNo: i + 1, newNo: j + 1, text: a });
      i++;
      j++;
    } else if (
      (table[(i + 1) * width + j] ?? 0) >= (table[i * width + j + 1] ?? 0)
    ) {
      rows.push({ kind: "del", oldNo: i + 1, text: a });
      i++;
    } else {
      rows.push({ kind: "add", newNo: j + 1, text: b });
      j++;
    }
  }
  for (; i < before.length; i++) {
    rows.push({ kind: "del", oldNo: i + 1, text: before[i] ?? "" });
  }
  for (; j < after.length; j++) {
    rows.push({ kind: "add", newNo: j + 1, text: after[j] ?? "" });
  }
  return rows;
};

/** Indexes of rows worth showing: every change, plus `CONTEXT` either side. */
const nearAChange = (rows: readonly DiffRow[]) => {
  const keep = new Set<number>();
  rows.forEach((row, i) => {
    if (row.kind === "ctx") {
      return;
    }
    for (let k = i - CONTEXT; k <= i + CONTEXT; k++) {
      keep.add(k);
    }
  });
  return keep;
};

/** Collapse long unchanged stretches into one countable gap row. */
const withGaps = (rows: readonly DiffRow[]): DiffRow[] => {
  const keep = nearAChange(rows);
  const out: DiffRow[] = [];
  let skipped = 0;
  rows.forEach((row, i) => {
    if (keep.has(i)) {
      if (skipped > 0) {
        out.push({ count: skipped, kind: "gap", text: "" });
        skipped = 0;
      }
      out.push(row);
      return;
    }
    skipped++;
  });
  if (skipped > 0) {
    out.push({ count: skipped, kind: "gap", text: "" });
  }
  return out;
};

/** Rows for a before/after pair, with unchanged stretches collapsed. */
export const diffLines = (before: string, after: string): LineDiff => {
  const a = splitLines(before);
  const b = splitLines(after);
  const rows =
    (a.length + 1) * (b.length + 1) > MAX_CELLS
      ? replaceAll(a, b).rows
      : alignRows(a, b);
  return {
    added: rows.filter((r) => r.kind === "add").length,
    removed: rows.filter((r) => r.kind === "del").length,
    rows: withGaps(rows),
  };
};
