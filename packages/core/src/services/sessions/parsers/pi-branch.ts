/**
 * Pi sessions are trees linked by `parentId`. Following Pi's session manager,
 * the leaf is the last entry and the active branch is its walk to the root.
 */
type Raw = Record<string, unknown>;

/**
 * Line indexes on the active branch, plus the `session` header lines, or null
 * to keep every line: an entry without an `id` or `parentId` key, a duplicate
 * id, a missing parent or a cycle means the tree is not trusted.
 */
export const activeBranch = (
  lines: readonly Raw[]
): ReadonlySet<number> | null => {
  const byId = new Map<string, number>();
  const keep = new Set<number>();
  for (const [index, line] of lines.entries()) {
    if (line.type === "session") {
      keep.add(index);
      continue;
    }
    const id = line.id;
    if (typeof id !== "string" || !("parentId" in line) || byId.has(id)) {
      return null;
    }
    byId.set(id, index);
  }
  let at = [...byId.values()].at(-1);
  while (at !== undefined) {
    if (keep.has(at)) {
      return null;
    }
    keep.add(at);
    const parent = lines[at]?.parentId;
    if (parent === null) {
      return keep;
    }
    if (typeof parent !== "string") {
      return null;
    }
    at = byId.get(parent);
  }
  return null;
};
