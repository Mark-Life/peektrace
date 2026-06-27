/** Per-procedure query atoms derived from the single `PeepholeClient`.
 *
 * Each atom is `Atom<Result<Success, Error>>` — read it with `useAtomValue` and
 * branch on the `Result` discriminated union (see `result-view.tsx`). Mutations
 * (`memory.create/update/delete`) are intentionally left to the Memory phase,
 * which will use `PeepholeClient.mutation(tag)`.
 */
import { PeepholeClient } from "./client";

/** Live capability matrix (`capabilities.list`). */
export const capabilitiesAtom = PeepholeClient.query(
  "capabilities.list",
  undefined
);

/** Cross-project memory overview (`memory.allVaults`) — used by the Memory UI. */
export const allVaultsAtom = PeepholeClient.query(
  "memory.allVaults",
  undefined
);

/** Lightweight session headers (`sessions.list`) — used by the Sessions UI. */
export const sessionsListAtom = PeepholeClient.query("sessions.list", {});
