/** Live versions: the event stream, the polling fallback, and the refresh
 * after a reconnect. */
import { describe, expect, test } from "bun:test";
import type { WatchVersions } from "../src/lib/api-types.gen";
import { startLive } from "../src/lib/live";

type Listener = (event: { data?: string }) => void;

/** A stand-in `EventSource` the test drives by hand. */
class FakeSource {
  static last: FakeSource | undefined;
  closed = false;
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
    stop();
  });

  test("no EventSource polls at once", async () => {
    const t = track();
    const stop = startLive(t.handlers, {
      poll: () => Promise.resolve({ memory: 1, sessions: 1 }),
    });
    await Promise.resolve();
    expect(t.seen).toEqual([{ memory: 1, sessions: 1 }]);
    stop();
  });
});
