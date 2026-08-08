/** Filesystem-driven freshness on the client (poll mechanism).
 *
 * The server's `WatchService` advances a monotonic per-scope version whenever an
 * agent writes outside the app. `watchVersionsAtom` reads those versions via the
 * `watch.poll` RPC; a leaf component (`WatchStatus`) polls it on a short interval
 * and, when a scope's version increases, refreshes only the atoms that scope
 * feeds — `allVaultsAtom` for memory, `sessionsListAtom` for sessions, plus the
 * open session's analysis (see `useAnalyzedSession`). No full-page reload.
 *
 * Two rules keep a refresh from disturbing the reader, both learned from the
 * transcript flicker:
 *
 * 1. Nothing subscribes to the raw `Result` of `watch.poll`. Every poll produces
 *    a new `Result` (waiting, then success), so a subscriber re-renders twice a
 *    second whether or not anything changed — and at the app root that means the
 *    whole tree, transcript included. Callers read the derived `…VersionAtom`s
 *    instead: the registry drops a value equal to the previous one before it
 *    reaches subscribers, so an unchanged version notifies nobody.
 * 2. Whatever does subscribe must be a leaf. `WatchStatus` renders one dot and
 *    no children, so a real version bump re-renders that dot alone.
 */
import {
  Atom,
  Result,
  useAtomRefresh,
  useAtomValue,
} from "@effect-atom/atom-react";
import type { WatchVersions } from "@workspace/rpc/contract";
import { useEffect, useRef } from "react";
import { allVaultsAtom, sessionsListAtom } from "./atoms";
import { PeektraceClient } from "./client";

/** Poll cadence for the watch token (ms). */
const POLL_INTERVAL = 1000;

/** Live per-scope watch versions (`watch.poll`). */
export const watchVersionsAtom = PeektraceClient.query("watch.poll", undefined);

/** One scope's version as a plain number — 0 until the first poll lands.
 *  Deriving the number (rather than reading the `Result`) is what makes an
 *  unchanged poll free: equal values never reach subscribers. */
const versionAtom = (scope: keyof WatchVersions) =>
  Atom.map(watchVersionsAtom, (result) =>
    Result.isSuccess(result) ? result.value[scope] : 0
  );

/** Memory-scope version: bumps when a memory `.md` changes on disk. */
export const memoryVersionAtom = versionAtom("memory");

/** Sessions-scope version: bumps when a session `.jsonl` changes on disk. */
export const sessionsVersionAtom = versionAtom("sessions");

/**
 * Run `onBump` whenever `version` advances past the value seen at mount, so the
 * first poll (which only reports where the watcher already was) refreshes
 * nothing. Safe against an unstable `onBump`: the guard has already moved on.
 */
export const useOnVersionBump = (version: number, onBump: () => void) => {
  const seen = useRef(version);
  useEffect(() => {
    if (version > seen.current) {
      seen.current = version;
      onBump();
    }
  }, [version, onBump]);
};

/**
 * Poll `watch.poll` and fan the bumps out to the root atoms. Returns the total
 * bump count across scopes, for a quiet "syncing" hint.
 *
 * Mount this in a LEAF component only — it subscribes to both version atoms, so
 * its caller re-renders on every real change. `WatchStatus` is that leaf.
 */
export const useWatchRefresh = () => {
  const pollAgain = useAtomRefresh(watchVersionsAtom);
  const memory = useAtomValue(memoryVersionAtom);
  const sessions = useAtomValue(sessionsVersionAtom);
  const refreshVaults = useAtomRefresh(allVaultsAtom);
  const refreshSessions = useAtomRefresh(sessionsListAtom);

  useEffect(() => {
    const id = setInterval(pollAgain, POLL_INTERVAL);
    return () => clearInterval(id);
  }, [pollAgain]);

  useOnVersionBump(memory, refreshVaults);
  useOnVersionBump(sessions, refreshSessions);

  return memory + sessions;
};
