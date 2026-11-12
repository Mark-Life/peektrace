/** Query atoms over the `/api` routes.
 *
 * Each atom is `Atom<Result<Success, ApiFailure>>`: read it with
 * `useAtomValue` and branch on the `Result` union (see `result-view.tsx`).
 */
import { Atom } from "@effect-atom/atom-react";
import type { AllVaults } from "@workspace/core/services/memory/types";
import { Effect } from "effect";
import { apiGet } from "./api";
import type {
  Capability,
  ServerInfo,
  SessionListing,
  SettingsResult,
} from "./api-types.gen";
import { sharedHeadersAtom } from "./share-headers";

/** Live capability matrix. */
export const capabilitiesAtom = Atom.make(
  apiGet<readonly Capability[]>("/api/capabilities")
);

/** Cross-project memory overview, used by the Memory UI. */
export const allVaultsAtom = Atom.make(apiGet<AllVaults>("/api/memory"));

/** Lightweight session headers, used by the Sessions UI. */
export const sessionsListAtom = Atom.make(
  apiGet<SessionListing>("/api/sessions").pipe(Effect.map((l) => l.headers))
);

/** `sessionsListAtom` with structural sharing: unchanged headers keep their
 *  identity across reloads, and a reload with equal data notifies nobody. Read
 *  this one in the UI; refresh `sessionsListAtom`. */
export const sessionHeadersAtom = sharedHeadersAtom(sessionsListAtom);

/** Current user settings file, used by the Settings UI. */
export const settingsAtom = Atom.make(apiGet<SettingsResult>("/api/settings"));

/** Server posture and scanned transcript folders. */
export const serverInfoAtom = Atom.make(apiGet<ServerInfo>("/api/server"));
