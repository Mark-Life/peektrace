/** Pure client-side filtering for `sessions ls`: `--since`, `--cwd`, sort, `--limit`. */
import { resolve } from "node:path";
import type { SessionHeader } from "@workspace/core/services/sessions/schema";

const RELATIVE = /^(\d+)([hdw])$/;
const UNIT_MS = { h: 3_600_000, d: 86_400_000, w: 604_800_000 } as const;

/** A relative (`7d`, `12h`, `2w`) or ISO date to epoch ms; `undefined` if invalid. */
export const parseSince = (text: string, now: number) => {
  const match = RELATIVE.exec(text.trim());
  if (match) {
    const unit = UNIT_MS[match[2] as keyof typeof UNIT_MS];
    return now - Number(match[1]) * unit;
  }
  const parsed = Date.parse(text);
  return Number.isNaN(parsed) ? undefined : parsed;
};

const TRAILING_SLASHES = /\/+$/;

const stripSlash = (path: string) =>
  path.length > 1 ? path.replace(TRAILING_SLASHES, "") || "/" : path;

/** Resolve a `--cwd` argument against this machine's working directory. */
export const resolveCwd = (text: string) => stripSlash(resolve(text));

/** True when the header's cwd is `cwd` or inside it. Rows without a cwd never match. */
export const matchesCwd = (header: Pick<SessionHeader, "cwd">, cwd: string) => {
  if (header.cwd === undefined) {
    return false;
  }
  const have = stripSlash(header.cwd);
  const want = stripSlash(cwd);
  return (
    want === "/" ||
    have === want ||
    have.startsWith(want.endsWith("/") ? want : `${want}/`)
  );
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
