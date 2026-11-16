/** Reactive `localStorage` value with JSON (de)serialization.
 *
 * All storage access is guarded so the UI still works (just unpersisted) when
 * `window`/`localStorage` is unavailable or throws (SSR, private mode, quota).
 */
import { Predicate } from "effect";
import { useCallback, useState } from "react";

/** Read + JSON-parse a stored value, falling back on any failure. `normalize`
 *  coerces a parsed-but-wrong-shape value (stale schema, hand-edited storage)
 *  back to a safe `T` so consumers never see an unexpected shape. */
const readStored = <T>(
  key: string,
  fallback: T,
  // oxlint-disable-next-line anti-slop/no-unknown-parameters -- JSON.parse output is the untrusted boundary `normalize` exists to validate
  normalize: (parsed: unknown) => T
): T => {
  if (typeof window === "undefined") {
    return fallback;
  }

  try {
    const raw = window.localStorage.getItem(key);

    return raw === null ? fallback : normalize(JSON.parse(raw));
  } catch {
    return fallback;
  }
};

/** JSON-serialize + write a value, swallowing any failure. */
const writeStored = (key: string, serialize: () => string) => {
  if (typeof window === "undefined") {
    return;
  }

  try {
    window.localStorage.setItem(key, serialize());
  } catch {
    // Private mode / quota — degrade to in-memory (unpersisted) state.
  }
};

/** True when `update` is an updater function rather than a plain value. */
const isUpdater = <T>(update: T | ((prev: T) => T)): update is (prev: T) => T =>
  Predicate.isFunction(update);

/**
 * Reactive `localStorage` value. Reads once lazily on mount (from the given
 * `key`) and writes through on every set. The returned setter accepts either a
 * next value or an updater `(prev) => next`, mirroring `useState`. Pass
 * `normalize` to coerce a parsed-but-wrong-shape stored value back to a safe
 * `T`.
 */
export const useLocalStorage = <T>(
  key: string,
  fallback: T,
  // oxlint-disable-next-line anti-slop/no-unknown-parameters -- JSON.parse output is the untrusted boundary `normalize` exists to validate
  normalize: (parsed: unknown) => T
) => {
  const [value, setValue] = useState<T>(() =>
    readStored(key, fallback, normalize)
  );

  const set = useCallback(
    (update: T | ((prev: T) => T)) => {
      setValue((prev) => {
        const resolved = isUpdater(update) ? update(prev) : update;

        writeStored(key, () => JSON.stringify(resolved));

        return resolved;
      });
    },
    [key]
  );

  return [value, set] as const;
};
