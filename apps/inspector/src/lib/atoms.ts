/** Per-procedure query atoms derived from the single `PeektraceClient`.
 *
 * Each atom is `Atom<Result<Success, Error>>` — read it with `useAtomValue` and
 * branch on the `Result` discriminated union (see `result-view.tsx`). Mutations
 * (`memory.create/update/delete`) are intentionally left to the Memory phase,
 * which will use `PeektraceClient.mutation(tag)`.
 */
import { PeektraceClient } from "./client";

/** Live capability matrix (`capabilities.list`). */
export const capabilitiesAtom = PeektraceClient.query(
  "capabilities.list",
  undefined
);

/** Cross-project memory overview (`memory.allVaults`) — used by the Memory UI. */
export const allVaultsAtom = PeektraceClient.query(
  "memory.allVaults",
  undefined
);

/** Lightweight session headers (`sessions.list`) — used by the Sessions UI. */
export const sessionsListAtom = PeektraceClient.query("sessions.list", {});
