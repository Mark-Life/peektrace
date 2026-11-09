/** Structural sharing for the session list.
 *
 * Every `sessions.list` reload decodes fresh header objects, so without sharing
 * a single growing session re-renders every row in the rail. `shareHeaders`
 * keeps the previous object for each header whose fields did not change, and
 * the previous array when nothing changed at all, so memoized rows and
 * `useMemo` dependencies see stable identities.
 */
import { Atom, Result } from "@effect-atom/atom-react";
import type { SessionHeader } from "@workspace/core/services/sessions/schema";
import { Option } from "effect";

/** Field-by-field equality. Every `SessionHeader` field is a primitive. */
const sameHeader = (a: SessionHeader, b: SessionHeader) => {
  const keys = Object.keys(a) as (keyof SessionHeader)[];
  return (
    keys.length === Object.keys(b).length && keys.every((k) => a[k] === b[k])
  );
};

/** Reuse `prev` objects (and `prev` itself) wherever `next` has equal data. */
export const shareHeaders = (
  prev: readonly SessionHeader[],
  next: readonly SessionHeader[]
) => {
  if (prev.length === 0) {
    return next;
  }
  const byId = new Map(prev.map((h) => [h.id, h]));
  const shared = next.map((h) => {
    const old = byId.get(h.id);
    return old !== undefined && sameHeader(old, h) ? old : h;
  });
  const unchanged =
    shared.length === prev.length && shared.every((h, i) => h === prev[i]);
  return unchanged ? prev : shared;
};

/**
 * Derive a shared view of a `sessions.list` result. A refresh in flight is not
 * surfaced (`waiting` is dropped), and a reload with equal data yields an equal
 * `Result`, so subscribers are not notified at all.
 */
export const sharedHeadersAtom = <E>(
  source: Atom.Atom<Result.Result<readonly SessionHeader[], E>>
) =>
  Atom.readable((get) => {
    const result = get(source);
    if (!Result.isSuccess(result)) {
      return result;
    }
    const prev = get
      .self<Result.Result<readonly SessionHeader[], E>>()
      .pipe(
        Option.filter(Result.isSuccess),
        Option.match({ onNone: () => [], onSome: (r) => r.value })
      );
    const headers = shareHeaders(prev, result.value);
    return Result.success<readonly SessionHeader[], E>(headers, {
      timestamp: result.timestamp,
    });
  });
