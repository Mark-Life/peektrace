/**
 * The gates a retry pair passes. Kept apart from the pairer so the caveats,
 * the CLI footer and the inspector copy quote the values without loading it.
 */

/** Calls after a failure in which a retry is looked for. */
export const RETRY_WINDOW = 3;

/** Hybrid similarity a retry must reach. */
export const PAIR_THRESHOLD = 0.6;

/** Longest call-to-call gap, in seconds, still read as a retry. */
export const MAX_RETRY_GAP_SEC = 600;
