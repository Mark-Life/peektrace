/**
 * Row-by-row `ls --json` comparison for a store OpenCode wrote to while both
 * CLIs ran. Sessions that changed in that window are left out; every other
 * session must be listed by both, in the same order, with the same row.
 */
import { firstLineDiff } from "./parity-lib";

export interface LsRow {
  readonly id: string;
  readonly path: string;
}

export const lsRowDiffs = <R extends LsRow>(
  ts: readonly R[],
  rust: readonly R[],
  changed: ReadonlySet<string>
) => {
  const byPath = (rows: readonly R[]) =>
    new Map(
      rows.filter((r) => !changed.has(r.path)).map((r) => [r.path, r] as const)
    );
  const t = byPath(ts);
  const r = byPath(rust);
  const onlyIn = (side: string, a: typeof t, b: typeof t) =>
    [...a.keys()].filter((p) => !b.has(p)).map((p) => `only in ${side}: ${p}`);
  const rows = [...t].flatMap(([path, row]) => {
    const other = r.get(path);
    const a = JSON.stringify(row);
    const b = JSON.stringify(other);
    return other && a !== b ? [`${path} ${firstLineDiff(a, b)}`] : [];
  });
  const both = (m: typeof t, o: typeof t) =>
    [...m.keys()].filter((p) => o.has(p));
  const tsOrder = both(t, r);
  const rustOrder = both(r, t);
  const at = tsOrder.findIndex((p, i) => p !== rustOrder[i]);
  const order =
    at < 0
      ? []
      : [`order differs at ${at}: ts=${tsOrder[at]} rust=${rustOrder[at]}`];
  return [...onlyIn("ts", t, r), ...onlyIn("rust", r, t), ...rows, ...order];
};

/** Paths of rows whose session stamp differs from the one taken before. */
export const changedPaths = (
  rows: readonly LsRow[],
  before: ReadonlyMap<string, string>,
  stamp: (id: string) => string
) =>
  new Set(
    rows.filter((r) => before.get(r.id) !== stamp(r.id)).map((r) => r.path)
  );
