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
import { useEffect, useMemo, useRef } from "react";
import { sessionHeaderAtom } from "./atoms";
import { PeektraceClient } from "./client";
import { isHeaderUpdate } from "./share-headers";
import { settledSessionsVersionAtom, useOnVersionBump } from "./watch-atoms";

const noop = () => undefined;

/**
 * Build a memoized `sessions.analyze` atom for one session id + redact flag, and
 * re-validate it when that session changes on disk, so a live transcript grows
 * by appending rows rather than going stale.
 *
 * A listed top-level session re-analyses only when its own list header changes,
 * so writes to other sessions cost the open transcript nothing. A subagent, or a
 * session the list does not carry, falls back to the settled sessions version.
 */
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
      PeektraceClient.query("sessions.analyze", {
        id,
        ...(parent === null ? {} : { parent }),
        ...(redact ? {} : { redact: false }),
      }),
    [id, parent, redact]
  );
  const refresh = useAtomRefresh(atom);
  const header = useAtomValue(sessionHeaderAtom(id));
  const tracked = parent === null && header !== undefined;

  const seenHeader = useRef(header);
  useEffect(() => {
    const prev = seenHeader.current;
    seenHeader.current = header;
    if (parent === null && isHeaderUpdate(prev, header)) {
      refresh();
    }
  }, [header, parent, refresh]);

  useOnVersionBump(
    useAtomValue(settledSessionsVersionAtom),
    tracked ? noop : refresh
  );
  return atom;
};
