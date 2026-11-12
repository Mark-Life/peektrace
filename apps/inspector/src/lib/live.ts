/** Live change versions from the server.
 *
 * One `EventSource` on `/api/events` pushes `{memory, sessions}` on connect
 * and on every change. When the stream cannot open (3 failed tries in a row,
 * or no `EventSource`), `/api/watch` is polled every second instead. After
 * the stream reconnects from an error, every live atom refreshes once: a
 * restarted server counts from 0 again, so no version bump would fire.
 */
import { Effect } from "effect";
import { apiGet, apiUrl } from "./api";
import type { WatchVersions } from "./api-types.gen";

const POLL_INTERVAL = 1000;
const MAX_FAILED_OPENS = 3;

export interface LiveHandlers {
  readonly onReconnect: () => void;
  readonly onVersions: (v: WatchVersions) => void;
}

/** What `startLive` needs from the browser; swapped in tests. */
export interface LiveDeps {
  readonly EventSource?: typeof EventSource;
  readonly poll: () => Promise<WatchVersions>;
}

const defaultDeps = (): LiveDeps => ({
  EventSource: globalThis.EventSource,
  poll: () => Effect.runPromise(apiGet<WatchVersions>("/api/watch")),
});

/** Start following the server; returns the stop function. */
export const startLive = (
  handlers: LiveHandlers,
  deps: LiveDeps = defaultDeps()
) => {
  let timer: ReturnType<typeof setInterval> | undefined;
  let source: EventSource | undefined;

  const startPolling = () => {
    const tick = () => {
      deps.poll().then(handlers.onVersions, () => undefined);
    };
    tick();
    timer = setInterval(tick, POLL_INTERVAL);
  };

  const Source = deps.EventSource;
  if (Source === undefined) {
    startPolling();
  } else {
    let failures = 0;
    let recovering = false;
    const es = new Source(apiUrl("/api/events"));
    source = es;
    es.addEventListener("open", () => {
      failures = 0;
    });
    es.addEventListener("versions", (event) => {
      handlers.onVersions(
        JSON.parse((event as MessageEvent<string>).data) as WatchVersions
      );
      if (recovering) {
        recovering = false;
        handlers.onReconnect();
      }
    });
    es.addEventListener("error", () => {
      failures += 1;
      recovering = true;
      if (failures >= MAX_FAILED_OPENS) {
        es.close();
        source = undefined;
        startPolling();
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
