/** Parameterized session query atoms.
 *
 * `sessions.list` is static (`sessionsListAtom` in `atoms.ts`), but the debug
 * view selects a session at runtime, so `sessions.analyze` / `sessions.get`
 * atoms are built per `{ id, redact }`. `PeepholeClient.query(tag, payload)`
 * returns a fresh `Atom<Result<…>>` each call, so callers must memoize the atom
 * by its inputs (see `useAnalyzedSession`) to keep it stable across renders.
 */
import { useMemo } from "react";
import { PeepholeClient } from "./client";

/** Build a memoized `sessions.analyze` atom for one session id + redact flag. */
export const useAnalyzedSession = ({
  id,
  redact,
}: {
  readonly id: string;
  readonly redact: boolean;
}) =>
  useMemo(
    () =>
      PeepholeClient.query("sessions.analyze", {
        id,
        ...(redact ? {} : { redact: false }),
      }),
    [id, redact]
  );
