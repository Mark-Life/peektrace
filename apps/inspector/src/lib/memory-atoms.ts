/** Memory writes over `/api/memory`. Call each with `{ payload }` through
 * `useAtomSet(…, { mode: "promiseExit" })`.
 */
import { Atom } from "@effect-atom/atom-react";
import type {
  DeleteResult,
  MemoryEntry,
} from "@workspace/core/services/memory/types";
import { apiPath, apiSend } from "./api";

interface MemoryRef {
  readonly name: string;
  readonly project: string;
}

interface MemoryCreate extends MemoryRef {
  readonly body: string;
  readonly description: string;
  readonly type: string;
}

interface MemoryUpdate extends MemoryRef {
  readonly body?: string;
  readonly expectedMtime?: number;
  readonly frontmatter?: {
    readonly description?: string;
    readonly name?: string;
    readonly type?: string;
  };
}

const entries = (project: string) =>
  apiPath("memory", "projects", project, "entries");

export const createMemoryAtom = Atom.fn(
  ({ payload }: { readonly payload: MemoryCreate }) =>
    apiSend<MemoryEntry>("POST", entries(payload.project), payload)
);

export const updateMemoryAtom = Atom.fn(
  ({ payload }: { readonly payload: MemoryUpdate }) =>
    apiSend<MemoryEntry>(
      "PATCH",
      `${entries(payload.project)}/${encodeURIComponent(payload.name)}`,
      payload
    )
);

export const deleteMemoryAtom = Atom.fn(
  ({ payload }: { readonly payload: MemoryRef }) =>
    apiSend<DeleteResult>(
      "DELETE",
      `${entries(payload.project)}/${encodeURIComponent(payload.name)}`
    )
);
