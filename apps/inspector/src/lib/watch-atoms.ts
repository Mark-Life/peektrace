/** Filesystem-driven freshness on the client.
 *
 * The server's watcher advances a monotonic per-scope version whenever an
 * agent writes outside the app, and pushes the versions over `/api/events`
 * (`live.ts`). When a scope's settled version increases, only the atoms that
 * scope feeds refresh: `allVaultsAtom` for memory, `sessionsListAtom` for
 * sessions, plus the open session's analysis (see `useAnalyzedSession`).
 *
 * Refreshes key off the settled versions (see `settle.ts`): sessions reload at
 * most every few seconds under continuous writes, and nothing reloads while the
 * user has paused live updates. Callers read the derived `…VersionAtom`s, so an
 * unchanged version notifies nobody, and only a leaf (`WatchStatus`) mounts
 * `useWatchRefresh`.
 */
import {
  Atom,
  useAtomRefresh,
  useAtomSet,
  useAtomValue,
} from "@effect-atom/atom-react";
import { useEffect, useRef } from "react";
import type { WatchVersions } from "./api-types.gen";
import { allVaultsAtom, sessionsListAtom } from "./atoms";
import { startLive } from "./live";
import { MEMORY_SETTLE, SESSIONS_SETTLE, settledAtom } from "./settle";

/** Live per-scope watch versions, set by the event stream. */
export const watchVersionsAtom = Atom.make<WatchVersions>({
  memory: 0,
  sessions: 0,
}).pipe(Atom.keepAlive);

/** One scope's version as a plain number. Equal values never reach
 *  subscribers. */
const versionAtom = (scope: keyof WatchVersions) =>
  Atom.map(watchVersionsAtom, (v) => v[scope]);

/** Memory-scope version: bumps when a memory `.md` changes on disk. */
const memoryVersionAtom = versionAtom("memory");

/** Sessions-scope version: bumps when a session `.jsonl` changes on disk. */
const sessionsVersionAtom = versionAtom("sessions");

/** "Pause live": holds every refresh until resumed. In memory on purpose — a
 *  forgotten, persisted pause would look like a broken app. */
export const livePausedAtom = Atom.make(false).pipe(Atom.keepAlive);

/** Memory version as applied: gated by pause. */
const settledMemoryVersionAtom = settledAtom({
  paused: livePausedAtom,
  source: memoryVersionAtom,
  timing: MEMORY_SETTLE,
}).pipe(Atom.keepAlive);

/** Sessions version as applied: debounced with a max wait, gated by pause. */
export const settledSessionsVersionAtom = settledAtom({
  paused: livePausedAtom,
  source: sessionsVersionAtom,
  timing: SESSIONS_SETTLE,
}).pipe(Atom.keepAlive);

/** True while paused and the disk has moved past what is on screen. */
export const liveWaitingAtom = Atom.readable(
  (get) =>
    get(livePausedAtom) &&
    (get(memoryVersionAtom) > get(settledMemoryVersionAtom) ||
      get(sessionsVersionAtom) > get(settledSessionsVersionAtom))
);

/**
 * Run `onBump` whenever `version` advances past the value seen at mount, so the
 * first snapshot (which only reports where the watcher already was) refreshes
 * nothing. Safe against an unstable `onBump`: the guard has already moved on.
 */
export const useOnVersionBump = (version: number, onBump: () => void) => {
  const seen = useRef(version);
  useEffect(() => {
    if (version > seen.current) {
      seen.current = version;
      onBump();
    } else if (version < seen.current) {
      // A restarted server counts from 0 again.
      seen.current = version;
    }
  }, [version, onBump]);
};

/**
 * Follow the server's versions and fan the settled bumps out to the root
 * atoms. Returns the total applied bump count across scopes, for a quiet
 * "syncing" hint.
 *
 * Mount this in a LEAF component only — it subscribes to both settled version
 * atoms, so its caller re-renders on every settled bump. `WatchStatus` is that
 * leaf.
 */
export const useWatchRefresh = () => {
  const setVersions = useAtomSet(watchVersionsAtom);
  const memory = useAtomValue(settledMemoryVersionAtom);
  const sessions = useAtomValue(settledSessionsVersionAtom);
  const refreshVaults = useAtomRefresh(allVaultsAtom);
  const refreshSessions = useAtomRefresh(sessionsListAtom);

  useEffect(
    () =>
      startLive({
        onVersions: setVersions,
        onReconnect: () => {
          refreshSessions();
          refreshVaults();
        },
      }),
    [setVersions, refreshSessions, refreshVaults]
  );

  useOnVersionBump(memory, refreshVaults);
  useOnVersionBump(sessions, refreshSessions);

  return memory + sessions;
};
