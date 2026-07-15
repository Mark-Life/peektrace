/** Reactive `localStorage` value with JSON (de)serialization.
 *
 * All storage access is guarded so the UI still works (just unpersisted) when
 * `window`/`localStorage` is unavailable or throws (SSR, private mode, quota).
 */
import { useCallback, useState } from "react";

/** Read + JSON-parse a stored value, falling back on any failure. `normalize`
 *  coerces a parsed-but-wrong-shape value (stale schema, hand-edited storage)
 *  back to a safe `T` so consumers never see an unexpected shape. */
const readStored = <T>(
  key: string,
  fallback: T,
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
const writeStored = <T>(key: string, value: T) => {
  if (typeof window === "undefined") {
    return;
  }
  try {
    window.localStorage.setItem(key, JSON.stringify(value));
  } catch {
    // Private mode / quota — degrade to in-memory (unpersisted) state.
  }
};

/**
 * Reactive `localStorage` value. Reads once lazily on mount (from the given
 * `key`) and writes through on every set. The returned setter accepts either a
 * next value or an updater `(prev) => next`, mirroring `useState`. Pass
 * `normalize` to coerce a parsed-but-wrong-shape stored value back to a safe
 * `T`; it defaults to trusting the parsed value.
 */
export const useLocalStorage = <T>(
  key: string,
  fallback: T,
  normalize: (parsed: unknown) => T = (parsed) => parsed as T
) => {
  const [value, setValue] = useState<T>(() =>
    readStored(key, fallback, normalize)
  );

  const set = useCallback(
    (next: T | ((prev: T) => T)) => {
      setValue((prev) => {
        const resolved =
          typeof next === "function" ? (next as (prev: T) => T)(prev) : next;
        writeStored(key, resolved);
        return resolved;
      });
    },
    [key]
  );

  return [value, set] as const;
};
