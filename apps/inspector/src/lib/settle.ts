/** Settled watch versions: debounce with a max wait, plus a pause gate.
 *
 * A live agent bumps the sessions version about once a second. Reloading on
 * every bump keeps the main thread busy, and a plain trailing debounce never
 * fires while writes continue. `settledAtom` emits after `wait` ms of quiet, or
 * at the latest `maxWait` ms after the first pending bump. While `paused` is
 * true it holds bumps back; on resume it emits at once if any are pending.
 */
import { Atom } from "@effect-atom/atom-react";

export interface SettleTiming {
  /** Upper bound from the first pending bump to the emit (ms). */
  readonly maxWait: number;
  /** Quiet time after the last bump before emitting (ms). */
  readonly wait: number;
}

export interface PendingBumps {
  readonly firstPendingAt: number;
  readonly lastBumpAt: number;
  readonly now: number;
}

/** Delay until the pending bumps should be emitted (ms, never negative). */
export const settleDelay = (
  { now, firstPendingAt, lastBumpAt }: PendingBumps,
  { wait, maxWait }: SettleTiming
) => Math.max(0, Math.min(lastBumpAt + wait, firstPendingAt + maxWait) - now);

/** Sessions: about one reload per 1.5 s of quiet, and at least every 4 s. */
export const SESSIONS_SETTLE: SettleTiming = { wait: 1500, maxWait: 4000 };

/** Memory edits are rare and small, so they apply on the next tick. */
export const MEMORY_SETTLE: SettleTiming = { wait: 0, maxWait: 0 };

/** `source` delayed by `timing`, held back while `paused` is true. */
export const settledAtom = ({
  source,
  paused,
  timing,
}: {
  readonly source: Atom.Atom<number>;
  readonly paused: Atom.Atom<boolean>;
  readonly timing: SettleTiming;
}) =>
  Atom.readable((get) => {
    let emitted = get.once(source);
    let latest = emitted;
    let isPaused = get.once(paused);
    let firstPendingAt: number | undefined;
    let lastBumpAt = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;

    const cancel = () => {
      if (timer !== undefined) {
        clearTimeout(timer);
        timer = undefined;
      }
    };
    const flush = () => {
      cancel();
      firstPendingAt = undefined;
      emitted = latest;
      get.setSelf(latest);
    };
    const schedule = () => {
      cancel();
      if (firstPendingAt === undefined || isPaused) {
        return;
      }
      const delay = settleDelay(
        { now: Date.now(), firstPendingAt, lastBumpAt },
        timing
      );
      timer = setTimeout(flush, delay);
    };

    get.addFinalizer(cancel);
    get.subscribe(source, (value) => {
      latest = value;
      if (value === emitted) {
        firstPendingAt = undefined;
        cancel();
        return;
      }
      const now = Date.now();
      firstPendingAt ??= now;
      lastBumpAt = now;
      schedule();
    });
    get.subscribe(paused, (next) => {
      // The registry can report the current value again; only a flip counts.
      if (next === isPaused) {
        return;
      }
      isPaused = next;
      if (!isPaused && firstPendingAt !== undefined) {
        flush();
        return;
      }
      schedule();
    });
    return emitted;
  });
