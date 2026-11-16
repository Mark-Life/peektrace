/** Settings write: `PUT /api/settings` with optional mtime compare-and-swap.
 *
 * Call it with `{ payload }` through `useAtomSet(…, { mode: "promiseExit" })`;
 * a stale `expectedMtime` fails with `FileChangedError`.
 */
import { Atom } from "@effect-atom/atom-react";
import type {
  SettingsResult,
  SettingsUpdate,
} from "@workspace/view-core/api-types.gen";

import { apiSend } from "./api";

export const updateSettingsAtom = Atom.fn(
  ({ payload }: { readonly payload: SettingsUpdate }) =>
    apiSend<SettingsResult>("PUT", "/api/settings", payload)
);
