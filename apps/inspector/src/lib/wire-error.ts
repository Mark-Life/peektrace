/** Narrow a failed mutation `Exit` to its typed Peephole wire error.
 *
 * RPC mutations resolve to an `Exit<Success, WireError>`; on failure the typed
 * domain error (`FileChangedError`, `CapabilityUnsupportedError`, …) lives in the
 * `Cause`. This pulls the first expected failure out so callers can branch on
 * `_tag` (e.g. show the CAS-conflict choice) instead of stringly-matching.
 */
import type { WireError } from "@workspace/rpc/contract";
import { Cause, type Exit, Option } from "effect";

/** The set of `_tag`s carried by the typed RPC wire errors. */
const WIRE_TAGS = new Set<WireError["_tag"]>([
  "FileChangedError",
  "MemoryValidationError",
  "MemoryNotFoundError",
  "CapabilityUnsupportedError",
  "PathOutsideRootError",
  "SessionNotFoundError",
  "TranscriptParseError",
]);

/** Narrow an unknown failure value to a typed `WireError`, if it is one. */
export const asWireError = (value: unknown): WireError | undefined => {
  if (
    typeof value === "object" &&
    value !== null &&
    "_tag" in value &&
    WIRE_TAGS.has((value as { _tag: WireError["_tag"] })._tag)
  ) {
    return value as WireError;
  }
  return;
};

/** Extract the typed `WireError` from a failed mutation exit, if present. */
export const wireErrorOf = (
  exit: Exit.Exit<unknown, WireError>
): WireError | undefined =>
  exit._tag === "Failure"
    ? Option.getOrUndefined(Cause.failureOption(exit.cause))
    : undefined;

/** Pull a typed `WireError` out of a query failure `Cause`, if present. */
export const wireErrorOfCause = (
  cause: Cause.Cause<unknown>
): WireError | undefined =>
  asWireError(Option.getOrUndefined(Cause.failureOption(cause)));

/** A human message for any wire error, with a CAS-conflict callout. */
export const wireErrorMessage = (error: WireError): string => {
  switch (error._tag) {
    case "FileChangedError":
      return `The file changed on disk (${error.reason}) since you loaded it.`;
    case "MemoryValidationError":
      return error.reason;
    case "MemoryNotFoundError":
      return `Memory "${error.name}" not found in ${error.project}.`;
    case "CapabilityUnsupportedError":
      return `${error.agentId} does not support ${error.capabilityId}.`;
    case "PathOutsideRootError":
      return `Refused: ${error.path} is outside the agent roots.`;
    case "SessionNotFoundError":
      return `Session "${error.id}" was not found.`;
    case "TranscriptParseError":
      return `That transcript could not be read (${error.reason}). The file may be locked or malformed.`;
    default:
      return "The request failed.";
  }
};
