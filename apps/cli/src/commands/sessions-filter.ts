/** Pure client-side filtering for `sessions ls`: `--since`, `--cwd`, sort, `--limit`. */
import { resolve } from "node:path";
import type { SessionHeader } from "@workspace/core/services/sessions/schema";

const RELATIVE = /^(\d+)([hdw])$/;
const ISO_DATE = /^\d{4}-\d{2}-\d{2}(?:[T ][\d:.]+(?:Z|[+-]\d{2}:?\d{2})?)?$/;
const UNIT_MS = { h: 3_600_000, d: 86_400_000, w: 604_800_000 } as const;
const MAX_DATE_MS = 8.64e15;

/** A relative (`7d`, `12h`, `2w`) or ISO date to epoch ms; `undefined` if invalid. */
export const parseSince = (text: string, now: number) => {
  const trimmed = text.trim();
  const match = RELATIVE.exec(trimmed);
  const parsed = match
    ? now - Number(match[1]) * UNIT_MS[match[2] as keyof typeof UNIT_MS]
    : Date.parse(ISO_DATE.test(trimmed) ? trimmed : "");
  return Number.isFinite(parsed) && Math.abs(parsed) <= MAX_DATE_MS
    ? parsed
    : undefined;
};

const TRAILING_SEPARATORS = /[\\/]+$/;
const WINDOWS_PATH = /^(?:[a-z]:|\\\\)/i;

const stripSeparators = (path: string) =>
  path.replace(TRAILING_SEPARATORS, "") || "/";

/** Resolve a `--cwd` argument against this machine's working directory. */
export const resolveCwd = (text: string) => stripSeparators(resolve(text));

/** True when the header's cwd is `cwd` or inside it. Rows without a cwd never match.
 * Both `/` and `\` separate segments; Windows paths compare case-insensitively. */
export const matchesCwd = (header: Pick<SessionHeader, "cwd">, cwd: string) => {
  if (header.cwd === undefined) {
    return false;
  }
  const fold = WINDOWS_PATH.test(cwd)
    ? (p: string) => p.toLowerCase()
    : (p: string) => p;
  const have = fold(stripSeparators(header.cwd));
  const want = fold(stripSeparators(cwd));
  if (want === "/" || have === want) {
    return true;
  }
  const next = have[want.length];
  return have.startsWith(want) && (next === "/" || next === "\\");
};

const timeOf = (header: Pick<SessionHeader, "updatedAt">) => {
  const t =
    header.updatedAt === undefined ? Number.NaN : Date.parse(header.updatedAt);
  return Number.isNaN(t) ? undefined : t;
};

/** Newest first; rows with no parseable time last; ties broken by id. */
export const byUpdatedDesc = (
  a: Pick<SessionHeader, "id" | "updatedAt">,
  b: Pick<SessionHeader, "id" | "updatedAt">
) => {
  const ta = timeOf(a);
  const tb = timeOf(b);
  if (ta !== tb) {
    if (ta === undefined) {
      return 1;
    }
    if (tb === undefined) {
      return -1;
    }
    return tb - ta;
  }
  return a.id.localeCompare(b.id);
};

export interface SelectOptions {
  readonly cwd?: string | undefined;
  /** Max rows; `0` means no limit. */
  readonly limit: number;
  /** Epoch ms lower bound on `updatedAt`. */
  readonly since?: number | undefined;
}

/** Apply `--since`, `--cwd`, sort, then `--limit`. `hidden` counts rows cut by the limit. */
export const selectSessions = <
  H extends Pick<SessionHeader, "cwd" | "id" | "updatedAt">,
>(
  headers: readonly H[],
  { since, cwd, limit }: SelectOptions
) => {
  const kept = headers
    .filter((h) => {
      if (since === undefined) {
        return true;
      }
      const t = timeOf(h);
      return t !== undefined && t >= since;
    })
    .filter((h) => cwd === undefined || matchesCwd(h, cwd))
    .sort(byUpdatedDesc);
  const rows = limit === 0 ? kept : kept.slice(0, limit);
  return { rows, hidden: kept.length - rows.length };
};
