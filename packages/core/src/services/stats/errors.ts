import { Data } from "effect";

/** Raised when the corpus walk cannot complete (root unreadable, cache dir denied). */
export class StatsScanError extends Data.TaggedError("StatsScanError")<{
  readonly reason: string;
  readonly path?: string;
}> {}

/** Raised when a drill key names no row in the requested table. */
export class StatsRowNotFoundError extends Data.TaggedError(
  "StatsRowNotFoundError"
)<{
  readonly key: string;
  readonly table: string;
}> {}

/** Raised when the fact cache cannot be read or written. */
export class StatsCacheError extends Data.TaggedError("StatsCacheError")<{
  readonly path: string;
  readonly reason: string;
}> {}
