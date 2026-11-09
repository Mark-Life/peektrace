/** Memory CRUD mutation atoms.
 *
 * Queries live in `atoms.ts`; this adds the three write paths via
 * `PeektraceClient.mutation(tag)` (an `AtomResultFn` — call it with a payload and
 * read back a `Result`). After any successful write the explorer refreshes
 * `allVaultsAtom` so the gauge / diff / graph re-validate from disk. Drive the
 * writes with `useAtomSet(atom, { mode: "promiseExit" })` to inspect the typed
 * `Exit` (so a `FileChangedError` can surface the CAS-conflict choice).
 */
import { PeektraceClient } from "./client";

/** Create a memory (`memory.create`) → new `MemoryEntry`. */
export const createMemoryAtom = PeektraceClient.mutation("memory.create");

/** Update a memory body/frontmatter (`memory.update`), CAS on mtime. */
export const updateMemoryAtom = PeektraceClient.mutation("memory.update");

/** Delete a memory (`memory.delete`) → dangling-reference report. */
export const deleteMemoryAtom = PeektraceClient.mutation("memory.delete");
