/** Filesystem-driven freshness on the client (poll mechanism).
 *
 * The server's `WatchService` advances a monotonic per-scope version whenever an
 * agent writes outside the app. `watchVersionsAtom` reads those versions via the
 * `watch.poll` RPC; `useWatchRefresh` polls it on a short interval and, when a
 * scope's version increases, refreshes only the atoms that scope feeds —
 * `allVaultsAtom` for memory, `sessionsListAtom` for sessions. No full-page
 * reload, no flashes: the affected list/gauge just re-validates from disk.
 *
 * The memory explorer and the session list both read from these two root atoms
 * (the open vault is a slice of `allVaultsAtom`), so refreshing them keeps the
 * visible surface — including a currently open vault — fresh.
 */
import { Result, useAtomRefresh, useAtomValue } from "@effect-atom/atom-react";
import type { WatchVersions } from "@workspace/rpc/contract";
import { useEffect, useRef } from "react";
import { allVaultsAtom, sessionsListAtom } from "./atoms";
import { PeektraceClient } from "./client";

/** Poll cadence for the watch token (ms). */
const POLL_INTERVAL = 1000;

/** Live per-scope watch versions (`watch.poll`). */
export const watchVersionsAtom = PeektraceClient.query("watch.poll", undefined);

/**
 * Mount once at the app root. Polls `watch.poll` every `POLL_INTERVAL`ms and
 * refreshes the affected root atoms when a scope's version advances.
 */
export const useWatchRefresh = () => {
  const versions = useAtomValue(watchVersionsAtom);
  const pollAgain = useAtomRefresh(watchVersionsAtom);
  const refreshVaults = useAtomRefresh(allVaultsAtom);
  const refreshSessions = useAtomRefresh(sessionsListAtom);
  const last = useRef<WatchVersions | null>(null);

  useEffect(() => {
    const id = setInterval(pollAgain, POLL_INTERVAL);
    return () => clearInterval(id);
  }, [pollAgain]);

  useEffect(() => {
    if (!Result.isSuccess(versions)) {
      return;
    }
    const next = versions.value;
    const prev = last.current;
    last.current = next;
    if (prev === null) {
      return;
    }
    if (next.memory > prev.memory) {
      refreshVaults();
    }
    if (next.sessions > prev.sessions) {
      refreshSessions();
    }
  }, [versions, refreshVaults, refreshSessions]);
};
