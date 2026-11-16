/** Per-session analysis atom.
 *
 * Atoms are built per `{ id, parent, redact }` inside a `useMemo`, so one open
 * session keeps one atom. The atom refetches when the settled sessions version
 * advances, so a live transcript follows the disk.
 */
import { Atom, useAtomRefresh, useAtomValue } from "@effect-atom/atom-react";
import type { AnalyzedSession } from "@workspace/core/services/sessions/schema";
import { useMemo } from "react";
import { apiGet, apiPath } from "./api";
import { settledSessionsVersionAtom, useOnVersionBump } from "./watch-atoms";

export const useAnalyzedSession = ({
  id,
  parent,
  redact,
}: {
  readonly id: string;
  /** Set when `id` is a subagent transcript, resolved through this session. */
  readonly parent: string | null;
  readonly redact: boolean;
}) => {
  const atom = useMemo(
    () =>
      Atom.make(
        apiGet<AnalyzedSession>(apiPath("sessions", id, "analysis"), {
          parent: parent ?? undefined,
          redact: redact ? undefined : "false",
        })
      ),
    [id, parent, redact]
  );
  const refresh = useAtomRefresh(atom);
  useOnVersionBump(useAtomValue(settledSessionsVersionAtom), refresh);
  return atom;
};
