import { describe, expect, test } from "bun:test";
import { TS_PORT_BASE, TS_PORT_SPAN } from "./parity-serve-lib";

/** Lowest port any OS hands to client sockets: Linux 32768, macOS and
 * Windows 49152. */
const EPHEMERAL_START = 32_768;

describe("fixed TS ports", () => {
  test("sit below every ephemeral range, so no client socket holds one", () => {
    expect(TS_PORT_BASE).toBeGreaterThanOrEqual(1024);
    expect(TS_PORT_BASE + TS_PORT_SPAN).toBeLessThanOrEqual(EPHEMERAL_START);
  });
});
