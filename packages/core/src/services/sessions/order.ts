/** Session ordering shared by every surface that lists sessions. */
import type { SessionHeader } from "./schema";

type Timed = Pick<SessionHeader, "id" | "updatedAt">;

/** `updatedAt` as epoch ms, or undefined when absent or unparseable. */
export const timeOf = (h: Pick<SessionHeader, "updatedAt">) => {
  const t = h.updatedAt === undefined ? Number.NaN : Date.parse(h.updatedAt);
  return Number.isNaN(t) ? undefined : t;
};

/** Newest first; rows with no parseable time last; ties broken by id. */
export const byUpdatedDesc = (a: Timed, b: Timed) => {
  const ta = timeOf(a);
  const tb = timeOf(b);
  if (ta === tb) {
    return a.id.localeCompare(b.id);
  }
  if (ta === undefined) {
    return 1;
  }
  return tb === undefined ? -1 : tb - ta;
};
