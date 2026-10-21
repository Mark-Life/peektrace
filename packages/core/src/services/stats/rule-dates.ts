/**
 * When each memory line appeared, so a rule is dated by its own line and not
 * by its file's mtime: editing one line of `AGENTS.md` must not reset the
 * still-firing measurement of every rule the file already held.
 *
 * The ledger keeps, per file (by real path), a hash of each line it held at the
 * last report and the time that line was first seen. A report dates each line:
 *
 * - a line the ledger holds keeps its recorded date;
 * - a new line of a file the ledger knows dates from the file's mtime, the
 *   edit that added it (now, when the file has no mtime);
 * - every line of a file the ledger has never seen dates from its mtime, the
 *   best guess without history.
 *
 * Lines are stored as hashes of their whitespace-collapsed text, never as text,
 * so the ledger holds nothing a memory file's secret could be read back from.
 * A file's entry keeps only the lines it holds now; a file this report did not
 * read keeps its entry, for when its project shows up again.
 */
import { createHash } from "node:crypto";
import type { LineDater, MemoryDoc } from "./detectors/rules";

/** Line hash → first-seen epoch ms (null: undated), per real path. */
export interface LineLedger {
  readonly files: Readonly<
    Record<string, Readonly<Record<string, number | null>>>
  >;
}

export const EMPTY_LEDGER: LineLedger = { files: {} };

const LINE_HASH_LEN = 16;
const SPACES = /\s+/g;

/** The ledger key of one line, or null for a blank line. */
const lineKey = (line: string): string | null => {
  const text = line.replace(SPACES, " ").trim();
  return text === ""
    ? null
    : createHash("sha1").update(text).digest("hex").slice(0, LINE_HASH_LEN);
};

/**
 * Date every line of `docs` against `ledger`. Returns the dater the rule check
 * reads and the ledger to write back. Pure: `now` is the caller's clock.
 */
export const dateLines = (args: {
  readonly docs: readonly MemoryDoc[];
  readonly ledger: LineLedger;
  readonly now: number;
}): { readonly dateOf: LineDater; readonly next: LineLedger } => {
  const { docs, ledger, now } = args;
  const files: Record<string, Readonly<Record<string, number | null>>> = {
    ...ledger.files,
  };
  /** Per real path, each line index's date. */
  const dated = new Map<string, readonly (number | null)[]>();
  for (const doc of docs) {
    if (dated.has(doc.realPath)) {
      continue;
    }
    const known = ledger.files[doc.realPath];
    const fresh = known === undefined ? doc.mtimeMs : (doc.mtimeMs ?? now);
    const kept: Record<string, number | null> = {};
    const days = doc.lines.map((line) => {
      const key = lineKey(line);
      if (key === null) {
        return null;
      }
      if (!Object.hasOwn(kept, key)) {
        kept[key] =
          known !== undefined && Object.hasOwn(known, key)
            ? (known[key] ?? null)
            : fresh;
      }
      return kept[key] ?? null;
    });
    dated.set(doc.realPath, days);
    files[doc.realPath] = kept;
  }
  return {
    dateOf: (doc, line) => {
      const days = dated.get(doc.realPath);
      return days === undefined ? doc.mtimeMs : (days[line] ?? null);
    },
    next: { files },
  };
};
