/** Per-procedure query atoms derived from the single `PeektraceClient`.
 *
 * Each atom is `Atom<Result<Success, Error>>` — read it with `useAtomValue` and
 * branch on the `Result` discriminated union (see `result-view.tsx`). Mutations
 * (`memory.create/update/delete`) are intentionally left to the Memory phase,
 * which will use `PeektraceClient.mutation(tag)`.
 */
import { PeektraceClient } from "./client";
import { sharedHeadersAtom } from "./share-headers";

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

/** `sessionsListAtom` with structural sharing: unchanged headers keep their
 *  identity across reloads, and a reload with equal data notifies nobody. Read
 *  this one in the UI; refresh `sessionsListAtom`. */
export const sessionHeadersAtom = sharedHeadersAtom(sessionsListAtom);

/** Current user settings file (`settings.get`) — used by the Settings UI. */
export const settingsAtom = PeektraceClient.query("settings.get", undefined);
