/**
 * A `scanning N/M transcripts…` line on stderr while a stats pass runs. Shown
 * only on a TTY, so pipes and agents reading stderr get nothing.
 */
import {
  StatsProgress,
  type StatsProgressShape,
} from "@workspace/core/services/stats/progress";
import { Effect } from "effect";

/** Quiet for this long, so a warm run prints nothing. */
const DELAY_MS = 250;
/** Gap between redraws. */
const EVERY_MS = 100;
const CLEAR = "\r\x1b[2K";

interface ProgressSink extends StatsProgressShape {
  /** Erase the line if one was drawn. */
  readonly clear: () => void;
}

/** Throttled sink over a writer and a clock, both injectable for tests. */
export const makeProgressSink = (args: {
  readonly now: () => number;
  readonly write: (chunk: string) => void;
}) => {
  const { now, write } = args;
  const start = now();
  let last = Number.NEGATIVE_INFINITY;
  let drawn = false;
  // A broken stderr must never fail the scan.
  const safeWrite = (chunk: string) => {
    try {
      write(chunk);
    } catch {
      // ignore
    }
  };
  return {
    onFile: (done, total) => {
      const t = now();
      if (t - start < DELAY_MS) {
        return;
      }
      if (t - last < EVERY_MS && done !== total) {
        return;
      }
      last = t;
      drawn = true;
      const fmt = (n: number) => n.toLocaleString("en-US");
      safeWrite(`\rscanning ${fmt(done)}/${fmt(total)} transcripts…`);
    },
    clear: () => {
      if (drawn) {
        drawn = false;
        safeWrite(CLEAR);
      }
    },
  } satisfies ProgressSink;
};

/**
 * Provide `StatsProgress` around an in-process stats call when stderr is a
 * TTY. The line is cleared on success, failure and interrupt.
 */
export const withScanProgress = <A, E, R>(effect: Effect.Effect<A, E, R>) => {
  if (!process.stderr.isTTY) {
    return effect;
  }
  return Effect.suspend(() => {
    const sink = makeProgressSink({
      now: Date.now,
      write: (chunk) => process.stderr.write(chunk),
    });
    return effect.pipe(
      Effect.provideService(StatsProgress, sink),
      Effect.ensuring(Effect.sync(sink.clear))
    );
  });
};
