/** Optional scan progress hook, read once when `StatsServiceLive` is built. */
import { Context } from "effect";

/** Called after each transcript of a pass. Must not throw. */
export interface StatsProgressShape {
  readonly onFile: (done: number, total: number) => void;
}

/**
 * Provide around the layer build to watch a pass advance. Absent by default,
 * so remote callers and the web server never see it.
 */
export class StatsProgress extends Context.Tag("@peektrace/StatsProgress")<
  StatsProgress,
  StatsProgressShape
>() {}
