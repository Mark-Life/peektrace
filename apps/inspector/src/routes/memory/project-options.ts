/** Projects a new memory can go into, built from session headers plus the
 * server's Claude project labels.
 *
 * Only Claude has a per-project memory folder, so only Claude sessions count.
 * Every labelled project dir is included too, so the list does not wait on
 * sessions and a vault that outlived its sessions still shows. Labels come
 * from the server, so a project keeps its name after its first memory.
 */

import type { ProjectLabel } from "@workspace/core/services/memory/types";
import type { SessionHeader } from "@workspace/core/services/sessions/schema";

/** One pickable project. */
export interface ProjectOption {
  readonly label: string;
  /** Newest session `updatedAt` (ISO), absent for vault-only projects. */
  readonly lastActive?: string;
  readonly slug: string;
}

/** Newest `updatedAt` per Claude project slug. */
const lastActiveBySlug = (
  headers: readonly Pick<SessionHeader, "agent" | "project" | "updatedAt">[]
) => {
  const out = new Map<string, string | undefined>();
  for (const h of headers) {
    if (h.agent !== "claude") {
      continue;
    }
    const prev = out.get(h.project);
    const next =
      prev === undefined || (h.updatedAt !== undefined && h.updatedAt > prev)
        ? h.updatedAt
        : prev;
    out.set(h.project, next);
  }
  return out;
};

/** Most recent first; projects with no session date last; then by label. */
const byRecency = (a: ProjectOption, b: ProjectOption) => {
  if (a.lastActive !== b.lastActive) {
    if (a.lastActive === undefined) {
      return 1;
    }
    if (b.lastActive === undefined) {
      return -1;
    }
    return a.lastActive > b.lastActive ? -1 : 1;
  }
  return a.label.localeCompare(b.label);
};

/** Dedupe, label and sort the projects a memory can be created in. */
export const memoryProjectOptions = ({
  headers,
  labels,
}: {
  readonly headers: readonly Pick<
    SessionHeader,
    "agent" | "project" | "updatedAt"
  >[];
  readonly labels: readonly ProjectLabel[];
}) => {
  const active = lastActiveBySlug(headers);
  const label = new Map(labels.map((l) => [l.slug, l.project]));
  const slugs = [...new Set([...active.keys(), ...label.keys()])];
  return slugs
    .map((slug) => {
      const lastActive = active.get(slug);
      return {
        slug,
        label: label.get(slug) ?? slug,
        ...(lastActive === undefined ? {} : { lastActive }),
      };
    })
    .sort(byRecency);
};
