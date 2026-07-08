/** Smoke tests for the `peektrace` CLI building blocks.
 *
 * Covers the pure renderers and one real in-process RPC round-trip
 * (`capabilities.list`) driven through the same client + handlers the CLI uses —
 * no network, no fixtures needed since the matrix is static.
 */
import { describe, expect, test } from "bun:test";
import { makeHandlersLayer, makeInProcessClient } from "@workspace/rpc";
import { Effect } from "effect";
import { bytes, percent, shortId, table, tokens } from "../src/render";

describe("render", () => {
  test("table aligns columns and rules", () => {
    const out = table(["A", "BB"], [["1", "22"]]);
    const [head, rule, row] = out.split("\n");
    expect(head).toBe("A  BB");
    expect(rule).toBe("-  --");
    expect(row).toBe("1  22");
  });

  test("table renders (none) for empty rows", () => {
    expect(table(["X"], []).endsWith("(none)")).toBe(true);
  });

  test("formatters", () => {
    expect(bytes(512)).toBe("512 B");
    expect(bytes(2048)).toBe("2.0 KB");
    expect(tokens(1500)).toBe("1.5k");
    expect(percent(0.5)).toBe("50.0%");
    expect(shortId("abcd-ef-12")).toBe("abcd");
  });
});

describe("in-process client", () => {
  test("capabilities.list round-trips the static matrix", async () => {
    const caps = await Effect.runPromise(
      Effect.gen(function* () {
        const client = yield* makeInProcessClient();
        return yield* client.capabilities.list();
      }).pipe(Effect.scoped, Effect.provide(makeHandlersLayer()))
    );
    expect(caps.length).toBeGreaterThan(0);
    const crud = caps.find((c) => c.id === "memory.crud");
    expect(crud?.perAgent.claude.level).toBe("supported");
  });
});
