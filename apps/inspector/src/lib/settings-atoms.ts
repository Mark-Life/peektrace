/** Settings mutation atom.
 *
 * The `settings.get` query lives in `atoms.ts`; this adds the write path via
 * `PeektraceClient.mutation("settings.update")` (an `AtomResultFn` — call it with
 * a payload and read back a `Result`). Drive it with
 * `useAtomSet(atom, { mode: "promiseExit" })` so a `FileChangedError` CAS
 * conflict can be handled; refresh `settingsAtom` after a successful write.
 */
import { PeektraceClient } from "./client";

/** Write the settings file (`settings.update`), CAS on mtime. */
export const updateSettingsAtom = PeektraceClient.mutation("settings.update");
