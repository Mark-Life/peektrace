/** Narrow a failed mutation `Exit` to its typed Peektrace API error.
 *
 * API mutations resolve to an `Exit<Success, WireError>`; on failure the typed
 * domain error (`FileChangedError`, `CapabilityUnsupportedError`, …) lives in the
 * `Cause`. This pulls the first expected failure out so callers can branch on
 * `_tag` (e.g. show the CAS-conflict choice) instead of stringly-matching.
 */
import type { ApiError } from "@workspace/view-core/api-types.gen";
import { Cause, Option, Predicate } from "effect";
import type { Exit } from "effect";

/** Every typed error an `/api` route answers with. */
export type WireError = ApiError;

/** The set of `_tag`s carried by the typed API errors. */
const WIRE_TAGS = new Set<string>([
  "FileChangedError",
  "MemoryValidationError",
  "MemoryNotFoundError",
  "CapabilityUnsupportedError",
  "PathOutsideRootError",
  "SessionNotFoundError",
  "TranscriptParseError",
  "SettingsWriteError",
  "StatsScanError",
  "StatsRowNotFoundError",
  "BadRequestError",
  "ImageNotFoundError",
  "NotFoundError",
  "MethodNotAllowedError",
]);

/** True when `value` is an object tagged as one of the typed API errors. */
// oxlint-disable-next-line anti-slop/no-unknown-parameters -- type guard over an untrusted failure value
const isWireError = (value: unknown): value is WireError =>
  Predicate.hasProperty(value, "_tag") &&
  Predicate.isString(value._tag) &&
  WIRE_TAGS.has(value._tag);

/** Narrow an unknown failure value to a typed `WireError`, if it is one. */
// oxlint-disable-next-line anti-slop/no-unknown-parameters -- a `Cause` failure channel is `unknown`
export const asWireError = (value: unknown): WireError | undefined =>
  isWireError(value) ? value : undefined;

/** Extract the typed `WireError` from a failed mutation exit, if present. */
export const wireErrorOf = (
  exit: Exit.Exit<unknown, unknown>
): WireError | undefined =>
  exit._tag === "Failure"
    ? asWireError(Option.getOrUndefined(Cause.failureOption(exit.cause)))
    : undefined;

/** Pull a typed `WireError` out of a query failure `Cause`, if present. */
export const wireErrorOfCause = (
  cause: Cause.Cause<unknown>
): WireError | undefined =>
  asWireError(Option.getOrUndefined(Cause.failureOption(cause)));

const FALLBACK_MESSAGE = "The request failed.";

/** A human message for any wire error, with a CAS-conflict callout. */
export const wireErrorMessage = (error: WireError): string => {
  switch (error._tag) {
    case "FileChangedError": {
      return `The file changed on disk (${error.reason}) since you loaded it.`;
    }

    case "MemoryValidationError": {
      return error.reason;
    }

    case "MemoryNotFoundError": {
      return `Memory "${error.name}" not found in ${error.project}.`;
    }

    case "CapabilityUnsupportedError": {
      return `${error.agentId} does not support ${error.capabilityId}.`;
    }

    case "PathOutsideRootError": {
      return `Refused: ${error.path} is outside the agent roots.`;
    }

    case "SessionNotFoundError": {
      return `Session "${error.id}" was not found.`;
    }

    case "TranscriptParseError": {
      return `That transcript could not be read (${error.reason}). The file may be locked or malformed.`;
    }

    case "SettingsWriteError": {
      return `Settings could not be saved (${error.reason}).`;
    }

    case "BadRequestError": {
      return `The server refused the request (${error.reason}).`;
    }

    case "ImageNotFoundError": {
      return "That image is not in the transcript.";
    }

    case "NotFoundError": {
      return `No such API route: ${error.path}.`;
    }

    case "MethodNotAllowedError": {
      return `The server does not accept ${error.method} here.`;
    }

    case "StatsScanError":
    case "StatsRowNotFoundError": {
      return FALLBACK_MESSAGE;
    }

    default: {
      return FALLBACK_MESSAGE;
    }
  }
};
