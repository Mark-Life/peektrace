/** Parameterized session query atoms.
 *
 * `sessions.list` is static (`sessionsListAtom` in `atoms.ts`), but the debug
 * view selects a session at runtime, so `sessions.analyze` / `sessions.get`
 * atoms are built per `{ id, redact }`. `PeektraceClient.query(tag, payload)`
 * returns a fresh `Atom<Result<…>>` each call, so callers must memoize the atom
 * by its inputs (see `useAnalyzedSession`) to keep it stable across renders.
 *
 * The memoization is load-bearing twice over: a new atom is also a new registry
 * node with no previous value, so it resolves to `Initial` and the view falls
 * back to the pending skeleton — the open transcript blanks and remounts. Keeping
 * one node means a refresh resolves to `waiting(previous)` instead: the rendered
 * transcript stays mounted while the newer analysis is in flight.
 */
import { useAtomRefresh, useAtomValue } from "@effect-atom/atom-react";
import { useMemo } from "react";
import { PeektraceClient } from "./client";
import { sessionsVersionAtom, useOnVersionBump } from "./watch-atoms";

/**
 * Build a memoized `sessions.analyze` atom for one session id + redact flag, and
 * re-validate it whenever an agent appends to a session on disk, so a live
 * transcript grows by appending rows rather than going stale.
 *
 * The version is read as a number, not as the poll's `Result`, so this only
 * re-renders when something actually changed (see `watch-atoms`).
 */
export const useAnalyzedSession = ({
  id,
  redact,
}: {
  readonly id: string;
  readonly redact: boolean;
}) => {
  const atom = useMemo(
    () =>
      PeektraceClient.query("sessions.analyze", {
        id,
        ...(redact ? {} : { redact: false }),
      }),
    [id, redact]
  );
  const refresh = useAtomRefresh(atom);
  useOnVersionBump(useAtomValue(sessionsVersionAtom), refresh);
  return atom;
};
