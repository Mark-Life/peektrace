/** The `/api` fetch client: typed JSON errors and transport failures. */
import { afterEach, describe, expect, test } from "bun:test";
import { Effect, Either } from "effect";
import { apiGet, apiPath, apiUrl } from "../src/lib/api";

const realFetch = globalThis.fetch;

const answer = (body: string, status: number, type: string) => {
  globalThis.fetch = (() =>
    Promise.resolve(
      new Response(body, { status, headers: { "content-type": type } })
    )) as unknown as typeof fetch;
};

const run = (path: string) => Effect.runPromise(Effect.either(apiGet(path)));

afterEach(() => {
  globalThis.fetch = realFetch;
});

describe("apiGet", () => {
  test("a JSON body decodes", async () => {
    answer('{"memory":1,"sessions":2}', 200, "application/json");
    expect(await run("/api/watch")).toEqual(
      Either.right({ memory: 1, sessions: 2 })
    );
  });

  test("a JSON error body becomes the typed error", async () => {
    answer(
      '{"error":{"_tag":"SessionNotFoundError","id":"x","searchedRoot":"all agents"}}',
      404,
      "application/json"
    );
    expect(await run("/api/sessions/x")).toEqual(
      Either.left({
        _tag: "SessionNotFoundError",
        id: "x",
        searchedRoot: "all agents",
      })
    );
  });

  test("a plain-text 401 becomes a TransportError", async () => {
    answer("Peektrace needs the token", 401, "text/plain;charset=utf-8");
    expect(await run("/api/server")).toEqual(
      Either.left({
        _tag: "TransportError",
        message: "Peektrace needs the token",
        status: 401,
      })
    );
  });

  test("a network fault becomes a TransportError", async () => {
    globalThis.fetch = (() =>
      Promise.reject(new Error("offline"))) as unknown as typeof fetch;
    expect(await run("/api/server")).toEqual(
      Either.left({ _tag: "TransportError", message: "offline" })
    );
  });
});

describe("urls", () => {
  test("segments are encoded and undefined params dropped", () => {
    expect(apiPath("sessions", "a/b c", "analysis")).toBe(
      "/api/sessions/a%2Fb%20c/analysis"
    );
    expect(apiUrl("/api/x", { parent: undefined, redact: "false" })).toBe(
      "/api/x?redact=false"
    );
  });
});
