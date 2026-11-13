/** Settings write: `PUT /api/settings` with optional mtime compare-and-swap.
 *
 * Call it with `{ payload }` through `useAtomSet(…, { mode: "promiseExit" })`;
 * a stale `expectedMtime` fails with `FileChangedError`.
 */
import { Atom } from "@effect-atom/atom-react";
import { apiSend } from "./api";
import type { SettingsResult, SettingsUpdate } from "./api-types.gen";

export const updateSettingsAtom = Atom.fn(
  ({ payload }: { readonly payload: SettingsUpdate }) =>
    apiSend<SettingsResult>("PUT", "/api/settings", payload)
);
