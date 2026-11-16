/** Live versions: the event stream, the polling fallback, and the refresh
 * after a reconnect. */
import { describe, expect, test } from "bun:test";
import { setTimeout as sleep } from "node:timers/promises";

import type { WatchVersions } from "@workspace/view-core/api-types.gen";

import { startLive } from "../src/lib/live";
import type { LiveChannel, LiveShare } from "../src/lib/live";

type Listener = (event: { data?: string }) => void;

/** A stand-in `EventSource` the test drives by hand. */
class FakeSource {
  static last: FakeSource | undefined;
  closed = false;
  /** 2 is `EventSource.CLOSED`: the browser stopped retrying. */
  readyState = 0;
  readonly listeners = new Map<string, Listener[]>();
  readonly url: string;
  constructor(url: string) {
    this.url = url;
    FakeSource.last = this;
  }
  addEventListener(type: string, fn: Listener) {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), fn]);
  }
  close() {
    this.closed = true;
  }
  emit(type: string, data?: string) {
    for (const fn of this.listeners.get(type) ?? []) {
      fn(data === undefined ? {} : { data });
    }
  }
}

// SAFETY: `startLive` only constructs the source and calls `addEventListener`, `close` and reads `readyState`; `FakeSource` provides those.
// oxlint-disable-next-line anti-slop/no-chained-type-assertions, typescript/no-unsafe-type-assertion -- the DOM `EventSource` has members the stand-in omits
const FakeEventSource = FakeSource as unknown as typeof EventSource;

const isGranted = <T>(
  v: LockOptions | LockGrantedCallback<T>
): v is LockGrantedCallback<T> => typeof v === "function";

type Message = Parameters<LiveChannel["postMessage"]>[0];

const lastSource = () => FakeSource.last;

const flush = async () => {
  await sleep(0);
};

/** One exclusive lock and one broadcast hub, as the tabs of an origin see. */
const fakeShare = (): LiveShare => {
  const members = new Set<(message: Message) => void>();
  const queue: (() => void)[] = [];
  let held = false;

  const next = () => {
    const grant = queue.shift();

    if (grant === undefined) {
      held = false;
    } else {
      grant();
    }
  };

  const request: LockManager["request"] = async <T>(
    _name: string,
    optionsOrGranted: LockOptions | LockGrantedCallback<T>,
    maybeGranted?: LockGrantedCallback<T>
  ) => {
    const options = isGranted(optionsOrGranted) ? undefined : optionsOrGranted;

    const onGranted = isGranted(optionsOrGranted)
      ? optionsOrGranted
      : maybeGranted;

    const { promise, resolve, reject } = Promise.withResolvers<Awaited<T>>();

    const grant = () => {
      held = true;
      void (async () => {
        resolve(await Promise.resolve(onGranted?.(null)));
        next();
      })();
    };

    options?.signal?.addEventListener("abort", () => {
      const i = queue.indexOf(grant);

      if (i !== -1) {
        queue.splice(i, 1);
        reject(new Error("aborted"));
      }
    });

    if (held) {
      queue.push(grant);
    } else {
      grant();
    }

    return await promise;
  };

  const channel = (): LiveChannel => {
    let fn: ((event: MessageEvent<Message>) => void) | undefined;

    const deliver = (data: Message) => {
      fn?.(new MessageEvent("message", { data }));
    };

    members.add(deliver);

    return {
      addEventListener: (_type, listener) => {
        fn = listener;
      },
      close: () => {
        members.delete(deliver);
      },
      postMessage: (message) => {
        for (const m of members) {
          if (m !== deliver) {
            m(message);
          }
        }
      },
    };
  };

  return { channel, locks: { request } };
};

const track = () => {
  const seen: WatchVersions[] = [];
  let reconnects = 0;

  return {
    handlers: {
      onReconnect: () => {
        reconnects += 1;
      },
      onVersions: (v: WatchVersions) => seen.push(v),
    },
    reconnects: () => reconnects,
    seen,
  };
};

const never = async () => await Promise.withResolvers<WatchVersions>().promise;

describe("startLive", () => {
  test("the snapshot sets versions", () => {
    const t = track();

    const stop = startLive(t.handlers, {
      EventSource: FakeEventSource,
      poll: never,
    });

    FakeSource.last?.emit("open");
    lastSource()?.emit("versions", '{"memory":0,"sessions":3}');
    expect(FakeSource.last?.url).toBe("/api/events");
    expect(t.seen).toEqual([{ memory: 0, sessions: 3 }]);
    expect(t.reconnects()).toBe(0);
    stop();
    expect(FakeSource.last?.closed).toBe(true);
  });

  test("a reconnect after an error refreshes once", () => {
    const t = track();

    const stop = startLive(t.handlers, {
      EventSource: FakeEventSource,
      poll: never,
    });

    const es = FakeSource.last;
    es?.emit("versions", '{"memory":0,"sessions":5}');
    es?.emit("error");
    es?.emit("open");
    es?.emit("versions", '{"memory":0,"sessions":0}');
    es?.emit("versions", '{"memory":0,"sessions":1}');
    expect(t.reconnects()).toBe(1);
    expect(t.seen.length).toBe(3);
    stop();
  });

  test("three failed opens fall back to polling", async () => {
    const t = track();
    let polls = 0;

    const stop = startLive(t.handlers, {
      EventSource: FakeEventSource,
      poll: async () => {
        polls += 1;

        return await Promise.resolve({ memory: 2, sessions: 4 });
      },
    });

    const es = FakeSource.last;
    es?.emit("error");
    es?.emit("error");
    expect(polls).toBe(0);
    es?.emit("error");
    expect(es?.closed).toBe(true);
    await flush();
    expect(polls).toBe(1);
    expect(t.seen).toEqual([{ memory: 2, sessions: 4 }]);
    expect(t.reconnects()).toBe(1);
    stop();
  });

  test("an HTTP error that closes the stream polls at once and refreshes", async () => {
    const t = track();
    let polls = 0;

    const stop = startLive(t.handlers, {
      EventSource: FakeEventSource,
      poll: async () => {
        polls += 1;

        return await Promise.resolve({ memory: 0, sessions: 7 });
      },
    });

    const es = FakeSource.last;
    es?.emit("versions", '{"memory":0,"sessions":5}');
    es?.emit("error");
    expect(polls).toBe(0);

    if (es !== undefined) {
      es.readyState = 2;
    }

    es?.emit("error");
    expect(es?.closed).toBe(true);
    await flush();
    expect(polls).toBe(1);
    expect(t.seen.at(-1)).toEqual({ memory: 0, sessions: 7 });
    expect(t.reconnects()).toBe(1);
    stop();
  });

  test("no EventSource polls at once", async () => {
    const t = track();

    const stop = startLive(t.handlers, {
      poll: async () => await Promise.resolve({ memory: 1, sessions: 1 }),
    });

    await flush();
    expect(t.seen).toEqual([{ memory: 1, sessions: 1 }]);
    expect(t.reconnects()).toBe(0);
    stop();
  });

  test("tabs share one stream through the lock holder", async () => {
    const share = fakeShare();

    const deps = {
      EventSource: FakeEventSource,
      poll: async () => await Promise.resolve({ memory: 0, sessions: 1 }),
      share,
    };

    FakeSource.last = undefined;
    const a = track();
    const stopA = startLive(a.handlers, deps);
    await flush();
    const leader = lastSource();
    expect(leader).toBeDefined();
    const b = track();
    const stopB = startLive(b.handlers, deps);
    await flush();
    expect(lastSource()).toBe(leader);
    leader?.emit("versions", '{"memory":0,"sessions":2}');
    leader?.emit("error");
    leader?.emit("versions", '{"memory":0,"sessions":0}');
    expect(b.seen.at(-1)).toEqual({ memory: 0, sessions: 0 });
    expect(b.reconnects()).toBe(1);
    expect(b.seen).toContainEqual({ memory: 0, sessions: 2 });
    stopA();
    await flush();
    expect(leader?.closed).toBe(true);
    expect(lastSource()).not.toBe(leader);
    lastSource()?.emit("versions", '{"memory":0,"sessions":3}');
    expect(b.seen.at(-1)).toEqual({ memory: 0, sessions: 3 });
    stopB();
    await flush();
    expect(lastSource()?.closed).toBe(true);
  });
});
