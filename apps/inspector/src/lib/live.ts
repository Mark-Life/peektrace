/** Live change versions from the server.
 *
 * One `EventSource` on `/api/events` pushes `{memory, sessions}` on connect
 * and on every change. When the stream cannot open (3 failed tries in a row,
 * the browser gives up on it after an HTTP error, or no `EventSource`),
 * `/api/watch` is polled every second instead. After the stream reconnects
 * from an error, or the first poll after it succeeds, every live atom
 * refreshes once: a restarted server counts from 0 again, so no version bump
 * would fire.
 *
 * Tabs of one origin share a single stream: the tab holding the `LIVE_LOCK`
 * Web Lock follows the server and relays to the others over a
 * `BroadcastChannel`. An HTTP/1.1 browser allows 6 connections per host, so
 * one open stream per tab would block requests once 6 tabs are open.
 */
import type { WatchVersions } from "@workspace/view-core/api-types.gen";
import { Effect } from "effect";
import { constVoid } from "effect/Function";

import { apiGet, apiUrl } from "./api";

const POLL_INTERVAL = 1000;
const MAX_FAILED_OPENS = 3;

/** `EventSource.CLOSED`: the browser will not retry this stream. */
const SOURCE_CLOSED = 2;
const LIVE_LOCK = "peektrace-live";
const LIVE_CHANNEL = "peektrace-live";

export interface LiveHandlers {
  readonly onReconnect: () => void;
  readonly onVersions: (v: WatchVersions) => void;
}

/** A relayed update from the tab that follows the server. */
type LiveMessage =
  | { readonly _tag: "Versions"; readonly versions: WatchVersions }
  | { readonly _tag: "Reconnect" };

/** The `BroadcastChannel` surface `startLive` uses. */
export interface LiveChannel {
  addEventListener: (
    type: "message",
    fn: (event: MessageEvent<LiveMessage>) => void
  ) => void;
  close: () => void;
  postMessage: (message: LiveMessage) => void;
}

/** Cross-tab sharing: a lock picks the leader, a channel relays. */
export interface LiveShare {
  readonly channel: () => LiveChannel;
  readonly locks: Pick<LockManager, "request">;
}

/** What `startLive` needs from the browser; swapped in tests. */
export interface LiveDeps {
  readonly EventSource?: typeof EventSource;
  readonly poll: () => Promise<WatchVersions>;
  /** Absent: this tab follows the server on its own. */
  readonly share?: LiveShare;
}

const defaultShare = (): LiveShare | undefined => {
  const locks = globalThis.navigator?.locks;
  const Channel = globalThis.BroadcastChannel;

  if (locks === undefined || Channel === undefined) {
    return undefined;
  }

  return { channel: () => new Channel(LIVE_CHANNEL), locks };
};

/** Swallows a failure the caller does not act on. */
const ignore = constVoid;

const defaultDeps = (): LiveDeps => ({
  EventSource: globalThis.EventSource,
  poll: async () =>
    await Effect.runPromise(apiGet<WatchVersions>("/api/watch")),
  share: defaultShare(),
});

/** Follow the server from this tab; returns the stop function. */
const follow = (handlers: LiveHandlers, deps: LiveDeps) => {
  let timer: ReturnType<typeof setInterval> | undefined;
  let source: EventSource | undefined;

  const startPolling = (recovering: boolean) => {
    let pending = recovering;

    const poll = async () => {
      const versions = await deps.poll().catch(ignore);

      if (versions === undefined) {
        return;
      }

      handlers.onVersions(versions);

      if (pending) {
        pending = false;
        handlers.onReconnect();
      }
    };

    const tick = () => {
      void poll();
    };

    tick();
    timer = setInterval(tick, POLL_INTERVAL);
  };

  const Source = deps.EventSource;

  if (Source === undefined) {
    startPolling(false);
  } else {
    let failures = 0;
    let recovering = false;
    const es = new Source(apiUrl("/api/events"));
    source = es;
    es.addEventListener("open", () => {
      failures = 0;
    });
    es.addEventListener("versions", (event) => {
      // SAFETY: the server sends each `versions` event as the JSON of a `WatchVersions`.
      // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- JSON.parse is typed `any`
      const versions = JSON.parse(String(event.data)) as WatchVersions;

      handlers.onVersions(versions);

      if (recovering) {
        recovering = false;
        handlers.onReconnect();
      }
    });
    es.addEventListener("error", () => {
      failures += 1;
      recovering = true;

      if (failures >= MAX_FAILED_OPENS || es.readyState === SOURCE_CLOSED) {
        es.close();
        source = undefined;
        startPolling(true);
      }
    });
  }

  return () => {
    source?.close();

    if (timer !== undefined) {
      clearInterval(timer);
    }
  };
};

/** Follow while this tab holds the lock, relay to the other tabs, and take
 *  what the leader relays otherwise. */
const shared = (handlers: LiveHandlers, deps: LiveDeps, share: LiveShare) => {
  const channel = share.channel();
  const abort = new AbortController();
  let release: (() => void) | undefined;
  let stopFollow: (() => void) | undefined;

  channel.addEventListener("message", ({ data }) => {
    if (data._tag === "Versions") {
      handlers.onVersions(data.versions);
    } else {
      handlers.onReconnect();
    }
  });

  const relay: LiveHandlers = {
    onReconnect: () => {
      handlers.onReconnect();
      // oxlint-disable-next-line unicorn/require-post-message-target-origin -- BroadcastChannel takes no target origin
      channel.postMessage({ _tag: "Reconnect" });
    },
    onVersions: (versions) => {
      handlers.onVersions(versions);
      // oxlint-disable-next-line unicorn/require-post-message-target-origin -- BroadcastChannel takes no target origin
      channel.postMessage({ _tag: "Versions", versions });
    },
  };

  // The leader relays only changes, so a new follower reads the versions now.
  const syncNow = async () => {
    const versions = await deps.poll().catch(ignore);

    if (versions !== undefined) {
      handlers.onVersions(versions);
    }
  };

  void syncNow();
  share.locks
    .request(LIVE_LOCK, { signal: abort.signal }, async () => {
      // oxlint-disable-next-line promise/avoid-new -- the lock is held until `release` settles this promise
      await new Promise<void>((resolve) => {
        release = resolve;
        stopFollow = follow(relay, deps);
      });
    })
    .catch(ignore);

  return () => {
    abort.abort();
    stopFollow?.();
    release?.();
    channel.close();
  };
};

/** Start following the server; returns the stop function. */
export const startLive = (
  handlers: LiveHandlers,
  deps: LiveDeps = defaultDeps()
) =>
  deps.share === undefined
    ? follow(handlers, deps)
    : shared(handlers, deps, deps.share);
