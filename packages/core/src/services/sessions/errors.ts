import { Data } from "effect";

/** Raised when a session id (or path) cannot be resolved to a transcript. */
export class SessionNotFoundError extends Data.TaggedError(
  "SessionNotFoundError"
)<{
  readonly id: string;
  readonly searchedRoot: string;
}> {}

/** Raised when a transcript cannot be read or decoded into a session. */
export class TranscriptParseError extends Data.TaggedError(
  "TranscriptParseError"
)<{
  readonly path: string;
  readonly reason: string;
}> {}
