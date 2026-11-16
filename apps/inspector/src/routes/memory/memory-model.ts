/** Pure rules behind the memory explorer and its browse table: filters,
 * sort, the size and date columns, and the `[[wikilink]]` body split. */
import type {
  AllVaults,
  MemoryEntry,
  MemoryVault,
} from "@workspace/view-core/api-types.gen";

/** Bytes per kilobyte, for the size column. */
const BYTES_PER_KB = 1024;

/** Splits a body into text + `[[wikilink]]` tokens. */
const WIKILINK_SPLIT = /(?<wikilink>\[\[[^\]]+\]\])/gu;

/** Matches a single `[[wikilink]]` token. */
const WIKILINK_TOKEN = /^\[\[(?<target>[^\]]+)\]\]$/u;

/** Sortable column keys. */
export type SortKey = "title" | "type" | "size" | "mtime" | "inIndex" | "links";

/** Does an entry match the free-text query? */
export const matchesQuery = (entry: MemoryEntry, q: string) => {
  if (q.length === 0) {
    return true;
  }

  const hay =
    `${entry.slug} ${entry.name ?? ""} ${entry.description ?? ""} ${entry.body}`.toLowerCase();

  return hay.includes(q.toLowerCase());
};

/** Apply search + type filter to a vault, returning its matching entries. */
export const filterEntries = ({
  vault,
  query,
  type,
}: {
  readonly vault: MemoryVault;
  readonly query: string;
  readonly type: string;
}) =>
  vault.entries.filter(
    (e) =>
      matchesQuery(e, query) &&
      (type === "all" || (e.type ?? "untyped") === type)
  );

/** The vaults in scope with their matching entries; a query or a type
 * drops vaults with no match. */
export const visibleVaults = ({
  all,
  scope,
  query,
  type,
}: {
  readonly all: AllVaults;
  readonly scope: string;
  readonly query: string;
  readonly type: string;
}) =>
  all.vaults
    .filter((v) => scope === "all" || v.slug === scope)
    .map((vault) => ({
      entries: filterEntries({ query, type, vault }),
      vault,
    }))
    .filter(({ entries }) =>
      query.length === 0 && type === "all" ? true : entries.length > 0
    );

/** Human-readable byte size. */
export const fmtSize = (bytes: number) =>
  bytes < BYTES_PER_KB
    ? `${bytes} B`
    : `${(bytes / BYTES_PER_KB).toFixed(1)} KB`;

/** Short local date-time for the modified column. */
export const fmtDate = (iso: string) => new Date(iso).toLocaleString();

/** A memory's display title: frontmatter name → slug. */
export const titleOf = (e: MemoryEntry) => e.name ?? e.slug;

const COMPARATORS: Record<SortKey, (a: MemoryEntry, b: MemoryEntry) => number> =
  {
    inIndex: (a, b) => Number(a.inIndex) - Number(b.inIndex),
    links: (a, b) => a.links.length - b.links.length,
    mtime: (a, b) => a.mtimeMs - b.mtimeMs,
    size: (a, b) => a.size - b.size,
    title: (a, b) => titleOf(a).localeCompare(titleOf(b)),
    type: (a, b) => (a.type ?? "").localeCompare(b.type ?? ""),
  };

/** Compare two entries by the active sort key. */
export const compareBy = (key: SortKey) => COMPARATORS[key];

/** Sort ascending, then reverse for a descending column. */
export const sortEntries = (
  entries: readonly MemoryEntry[],
  sort: SortKey,
  asc: boolean
) => {
  const arr = entries.toSorted(compareBy(sort));

  return asc ? arr : arr.toReversed();
};

/** One piece of a body: text, or a wikilink and the slug it opens. */
export type BodyPart =
  | { readonly text: string }
  | { readonly link: string; readonly slug: string };

/** Split a body into text and `[[wikilinks]]`, each resolved through the
 * entry's links, else its lower-cased text. */
export const bodyParts = (entry: MemoryEntry): BodyPart[] => {
  const targets = new Map(entry.links.map((l) => [l.rawTarget, l.targetSlug]));

  return entry.body.split(WIKILINK_SPLIT).map((part) => {
    const m = WIKILINK_TOKEN.exec(part);

    const raw = m?.groups?.target;

    if (raw !== undefined && raw !== "") {
      return { link: part, slug: targets.get(raw) ?? raw.toLowerCase() };
    }

    return { text: part };
  });
};
