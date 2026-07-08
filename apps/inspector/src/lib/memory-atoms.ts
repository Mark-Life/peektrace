/** Memory CRUD mutation atoms + the live capability gate.
 *
 * Queries live in `atoms.ts`; this adds the three write paths via
 * `PeektraceClient.mutation(tag)` (an `AtomResultFn` — call it with a payload and
 * read back a `Result`). After any successful write the explorer refreshes
 * `allVaultsAtom` so the gauge / diff / graph re-validate from disk. Drive the
 * writes with `useAtomSet(atom, { mode: "promiseExit" })` to inspect the typed
 * `Exit` (so a `FileChangedError` can surface the CAS-conflict choice).
 */
import type { AgentId } from "@workspace/core/services/agent-id";
import type { Capability } from "@workspace/rpc/contract";
import { PeektraceClient } from "./client";

/** Create a memory (`memory.create`) → new `MemoryEntry`. */
export const createMemoryAtom = PeektraceClient.mutation("memory.create");

/** Update a memory body/frontmatter (`memory.update`), CAS on mtime. */
export const updateMemoryAtom = PeektraceClient.mutation("memory.update");

/** Delete a memory (`memory.delete`) → dangling-reference report. */
export const deleteMemoryAtom = PeektraceClient.mutation("memory.delete");

/** The capability id that gates all memory writes. */
export const MEMORY_CRUD = "memory.crud";

/**
 * Decide whether the given agent may write memories, given the live matrix.
 * Returns the per-agent verdict so the UI can show the note when blocked.
 */
export const memoryCrudVerdict = ({
  caps,
  agent,
}: {
  readonly caps: readonly Capability[];
  readonly agent: AgentId;
}) => {
  const cap = caps.find((c) => c.id === MEMORY_CRUD);
  const support = cap?.perAgent[agent];
  return {
    level: support?.level ?? "unsupported",
    note: support?.note,
    canWrite: support?.level === "supported",
    capability: cap,
  } as const;
};
