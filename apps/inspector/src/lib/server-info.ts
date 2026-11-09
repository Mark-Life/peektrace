/** `server.info` for views that show scanned folders. Roots are fixed at boot,
 * but each folder's `exists` flag is read per request, so these views refetch
 * on mount instead of trusting the first answer. */
import { Result, useAtomRefresh, useAtomValue } from "@effect-atom/atom-react";
import { Option } from "effect";
import { useEffect, useRef } from "react";
import { serverInfoAtom } from "./atoms";
import { settledSessionsVersionAtom, useOnVersionBump } from "./watch-atoms";

/** `server.info`, refetched once on mount. */
export const useFreshServerInfo = () => {
  const info = useAtomValue(serverInfoAtom);
  const refresh = useAtomRefresh(serverInfoAtom);
  // A first load is already in flight; only an older answer needs a refetch.
  const stale = useRef(!Result.isInitial(info));
  useEffect(() => {
    if (stale.current) {
      stale.current = false;
      refresh();
    }
  }, [refresh]);
  return { info, refresh };
};

/** Like `useFreshServerInfo`, and also refetched on every settled sessions bump,
 * for views waiting for a scanned folder to appear. */
export const useLiveServerInfo = () => {
  const fresh = useFreshServerInfo();
  useOnVersionBump(useAtomValue(settledSessionsVersionAtom), fresh.refresh);
  return fresh;
};

/** A failed refetch over an earlier answer keeps that answer, so views built on
 * it (and their local state) stay mounted. */
export const withLastSuccess = <A, E>(
  result: Result.Result<A, E>
): Result.Result<A, E> =>
  Result.isFailure(result) && Option.isSome(result.previousSuccess)
    ? result.previousSuccess.value
    : result;
