/** Live versions: the event stream, the polling fallback, and the refresh
 * after a reconnect. */
import { describe, expect, test } from "bun:test";
import type { WatchVersions } from "../src/lib/api-types.gen";
import { type LiveChannel, type LiveShare, startLive } from "../src/lib/live";

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

const track = () => {
  const seen: WatchVersions[] = [];
  let reconnects = 0;
  return {
    seen,
    reconnects: () => reconnects,
    handlers: {
      onVersions: (v: WatchVersions) => seen.push(v),
      onReconnect: () => {
        reconnects += 1;
      },
    },
  };
};

const never = () => new Promise<WatchVersions>(() => undefined);

describe("startLive", () => {
  test("the snapshot sets versions", () => {
    const t = track();
    const stop = startLive(t.handlers, {
      EventSource: FakeSource as unknown as typeof EventSource,
      poll: never,
    });
    FakeSource.last?.emit("open");
    FakeSource.last?.emit("versions", '{"memory":0,"sessions":3}');
    expect(FakeSource.last?.url).toBe("/api/events");
    expect(t.seen).toEqual([{ memory: 0, sessions: 3 }]);
    expect(t.reconnects()).toBe(0);
    stop();
    expect(FakeSource.last?.closed).toBe(true);
  });

  test("a reconnect after an error refreshes once", () => {
    const t = track();
    const stop = startLive(t.handlers, {
      EventSource: FakeSource as unknown as typeof EventSource,
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
      EventSource: FakeSource as unknown as typeof EventSource,
      poll: () => {
        polls += 1;
        return Promise.resolve({ memory: 2, sessions: 4 });
      },
    });
    const es = FakeSource.last;
    es?.emit("error");
    es?.emit("error");
    expect(polls).toBe(0);
    es?.emit("error");
    expect(es?.closed).toBe(true);
    await Promise.resolve();
    expect(polls).toBe(1);
    expect(t.seen).toEqual([{ memory: 2, sessions: 4 }]);
    expect(t.reconnects()).toBe(1);
    stop();
  });

  test("an HTTP error that closes the stream polls at once and refreshes", async () => {
    const t = track();
    let polls = 0;
    const stop = startLive(t.handlers, {
      EventSource: FakeSource as unknown as typeof EventSource,
      poll: () => {
        polls += 1;
        return Promise.resolve({ memory: 0, sessions: 7 });
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
    await Promise.resolve();
    expect(polls).toBe(1);
    expect(t.seen.at(-1)).toEqual({ memory: 0, sessions: 7 });
    expect(t.reconnects()).toBe(1);
    stop();
  });

  test("no EventSource polls at once", async () => {
    const t = track();
    const stop = startLive(t.handlers, {
      poll: () => Promise.resolve({ memory: 1, sessions: 1 }),
    });
    await Promise.resolve();
    expect(t.seen).toEqual([{ memory: 1, sessions: 1 }]);
    expect(t.reconnects()).toBe(0);
    stop();
  });

  test("tabs share one stream through the lock holder", async () => {
    const share = fakeShare();
    const deps = {
      EventSource: FakeSource as unknown as typeof EventSource,
      poll: () => Promise.resolve({ memory: 0, sessions: 1 }),
      share,
    };
    FakeSource.last = undefined;
    const a = track();
    const stopA = startLive(a.handlers, deps);
    await flush();
    const leader = FakeSource.last as FakeSource | undefined;
    expect(leader).toBeDefined();
    const b = track();
    const stopB = startLive(b.handlers, deps);
    await flush();
    expect(FakeSource.last).toBe(leader);
    leader?.emit("versions", '{"memory":0,"sessions":2}');
    leader?.emit("error");
    leader?.emit("versions", '{"memory":0,"sessions":0}');
    expect(b.seen.at(-1)).toEqual({ memory: 0, sessions: 0 });
    expect(b.reconnects()).toBe(1);
    expect(b.seen).toContainEqual({ memory: 0, sessions: 2 });
    stopA();
    await flush();
    expect(leader?.closed).toBe(true);
    expect(FakeSource.last).not.toBe(leader);
    FakeSource.last?.emit("versions", '{"memory":0,"sessions":3}');
    expect(b.seen.at(-1)).toEqual({ memory: 0, sessions: 3 });
    stopB();
    await flush();
    expect(FakeSource.last?.closed).toBe(true);
  });
});

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

/** One exclusive lock and one broadcast hub, as the tabs of an origin see. */
const fakeShare = (): LiveShare => {
  const members = new Set<(message: unknown) => void>();
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
  const request = (
    _name: string,
    options: { signal?: AbortSignal },
    callback: () => Promise<void>
  ) =>
    new Promise<void>((resolve, reject) => {
      const grant = () => {
        held = true;
        callback().then(() => {
          resolve();
          next();
        });
      };
      options.signal?.addEventListener("abort", () => {
        const i = queue.indexOf(grant);
        if (i >= 0) {
          queue.splice(i, 1);
          reject(new Error("aborted"));
        }
      });
      if (held) {
        queue.push(grant);
      } else {
        grant();
      }
    });
  const channel = (): LiveChannel => {
    let fn: ((event: MessageEvent) => void) | undefined;
    const deliver = (data: unknown) => fn?.({ data } as MessageEvent);
    members.add(deliver);
    return {
      addEventListener: (_type, listener) => {
        fn = listener as (event: MessageEvent) => void;
      },
      close: () => members.delete(deliver),
      postMessage: (message) => {
        for (const m of members) {
          if (m !== deliver) {
            m(message);
          }
        }
      },
    };
  };
  return {
    channel,
    locks: { request } as unknown as LiveShare["locks"],
  };
};
