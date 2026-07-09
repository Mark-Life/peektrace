/** CLI error surface: a typed user error plus a clean one-line renderer.
 *
 * The root command's error boundary (`index.ts`) catches these — and the RPC
 * domain errors that flow up through `withClient` — and prints only the rendered
 * message to stderr, never a raw Effect FiberFailure / bunfs stack dump.
 */
import { Data } from "effect";

/** A user-facing CLI error: rendered as a clean one-liner, never a stack dump. */
export class CliUserError extends Data.TaggedError("CliUserError")<{
  readonly message: string;
}> {}

interface Tagged {
  readonly _tag: string;
}

/** Narrow an unknown value to a tagged (`_tag`) object. */
const isTagged = (u: unknown): u is Tagged =>
  typeof u === "object" && u !== null && "_tag" in u;

/**
 * Human message for a known domain/user error; a generic message fallback for
 * anything else. Used by the CLI error boundary to render expected failures
 * cleanly. Structural (`_tag`-keyed) so it works for both in-process instances
 * and schema-decoded remote errors without importing every error class.
 */
export const formatCliError = (error: unknown): string => {
  if (!isTagged(error)) {
    return error instanceof Error ? error.message : String(error);
  }
  const e = error as Tagged & Record<string, unknown>;
  switch (e._tag) {
    case "CliUserError":
      return String(e.message);
    case "SessionNotFoundError":
      return `Session "${String(e.id)}" not found (searched ${String(e.searchedRoot)}).`;
    case "MemoryNotFoundError":
      return `Memory "${String(e.name)}" not found in project "${String(e.project)}".`;
    case "MemoryValidationError": {
      // `.name` is also the base Error's name (the tag); only treat it as the
      // schema's memory-name field when it differs from the tag.
      const named =
        typeof e.name === "string" && e.name !== e._tag ? ` "${e.name}"` : "";
      return `Invalid memory${named}: ${String(e.reason)}`;
    }
    case "FileChangedError":
      return `File changed on disk (${String(e.reason)}): ${String(e.path)}. Reload and retry.`;
    case "PathOutsideRootError":
      return `Refused: path escapes the allowed roots: ${String(e.path)}`;
    case "CapabilityUnsupportedError":
      return `Unsupported for agent "${String(e.agentId)}": ${String(e.capabilityId)}`;
    case "TranscriptParseError":
      return `Failed to parse transcript ${String(e.path)}: ${String(e.reason)}`;
    default:
      return typeof e.message === "string" ? e.message : `Error: ${e._tag}`;
  }
};
