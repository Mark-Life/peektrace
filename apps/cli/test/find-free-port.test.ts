/** `findFreePort` must not stall when a client connects to the probed port. */
import { expect, test } from "bun:test";
import { connect } from "node:net";
import { Effect } from "effect";
import { findFreePort } from "../src/serve-core";

test("a client connecting during the port probe does not stall it", async () => {
  const port = await Effect.runPromise(findFreePort(44_200, "127.0.0.1"));
  // Keep connecting to the port, like a browser tab polling a restarting server.
  let polling = true;
  const sockets: ReturnType<typeof connect>[] = [];
  const poll = async () => {
    while (polling) {
      const socket = connect(port, "127.0.0.1");
      socket.on("error", () => undefined);
      sockets.push(socket);
      await new Promise((r) => setImmediate(r));
    }
  };
  const poller = poll();
  try {
    const started = performance.now();
    for (let i = 0; i < 200; i++) {
      expect(
        await Effect.runPromise(findFreePort(port, "127.0.0.1"))
      ).toBeGreaterThanOrEqual(port);
    }
    expect(performance.now() - started).toBeLessThan(2000);
  } finally {
    polling = false;
    await poller;
    for (const socket of sockets) {
      socket.destroy();
    }
  }
}, 10_000);
