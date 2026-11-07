/** Token auth: pure helpers plus a real in-process Bun server gated by a token.
 * The auth gate keys on the `token` option, so a loopback socket exercises it
 * without binding a real off-box interface. */
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import {
  HttpClientError,
  HttpClientRequest,
  HttpClientResponse,
  HttpServer,
} from "@effect/platform";
import { BunHttpServer } from "@effect/platform-bun";
import { RpcClientError, RpcSerialization } from "@effect/rpc";
import { createPeektraceClient, makeHandlersLayer } from "@workspace/rpc";
import { Effect, Layer, Scope } from "effect";
import { isRpcClientError, remoteErrorMessage } from "../src/errors";
import {
  authOutcome,
  cookieName,
  makeToken,
  resolveAuthToken,
  servePosture,
  splitRemote,
  tokensEqual,
  tokenUrl,
} from "../src/serve-auth";
import { buildServeApp, findFreePort } from "../src/serve-core";

const TOKEN_SHAPE = /^[A-Za-z0-9_-]{43}$/;
const TOKEN = "t".repeat(43);
const RPC_BODY = `${JSON.stringify({
  _tag: "Request",
  id: "1",
  tag: "sessions.list",
  payload: {},
  traceId: "0",
  spanId: "0",
  sampled: false,
  headers: [],
})}\n`;

describe("pure helpers", () => {
  test("makeToken is 256-bit base64url and unique", () => {
    const a = makeToken();
    expect(a).toMatch(TOKEN_SHAPE);
    expect(makeToken()).not.toBe(a);
  });

  test("tokensEqual", () => {
    expect(tokensEqual("abc", "abc")).toBe(true);
    expect(tokensEqual("abc", "abd")).toBe(false);
    expect(tokensEqual("abc", "abcd")).toBe(false);
    expect(tokensEqual("", "x")).toBe(false);
  });

  test("servePosture table", () => {
    const row = (host: string, readOnly: boolean, allowWrite: boolean) =>
      servePosture({ host, readOnly, allowWrite });
    expect(row("127.0.0.1", false, false)).toEqual({
      auth: false,
      readOnly: false,
    });
    expect(row("localhost", false, true)).toEqual({
      auth: false,
      readOnly: false,
    });
    expect(row("127.0.0.1", true, false)).toEqual({
      auth: false,
      readOnly: true,
    });
    expect(row("0.0.0.0", false, false)).toEqual({
      auth: true,
      readOnly: true,
    });
    expect(row("0.0.0.0", false, true)).toEqual({
      auth: true,
      readOnly: false,
    });
    expect(row("0.0.0.0", true, true)).toEqual({ auth: true, readOnly: true });
    expect(row("::ffff:127.0.0.1", false, false).auth).toBe(true);
  });

  test("authOutcome strips only the token param", () => {
    const outcome = authOutcome(
      {
        cookies: {},
        headers: {},
        method: "GET",
        url: `/sessions?titlebar=inset&token=${TOKEN}&x=1`,
      },
      TOKEN,
      4321
    );
    expect(outcome).toEqual({
      _tag: "redirect",
      location: "/sessions?titlebar=inset&x=1",
    });
  });

  test("authOutcome ignores ?token= on non-GET", () => {
    const outcome = authOutcome(
      { cookies: {}, headers: {}, method: "POST", url: `/?token=${TOKEN}` },
      TOKEN,
      4321
    );
    expect(outcome._tag).toBe("unauthorized");
  });

  test("cookie name carries the port", () => {
    expect(cookieName(4321)).toBe("peektrace_token_4321");
  });

  test("splitRemote separates the token from the URL", () => {
    expect(splitRemote(`http://h:1/?token=${TOKEN}`)).toEqual({
      url: "http://h:1",
      token: TOKEN,
    });
    expect(splitRemote("http://h:1")).toEqual({
      url: "http://h:1",
      token: undefined,
    });
    expect(splitRemote("h:1").token).toBeUndefined();
    expect(tokenUrl("http://h:1", "a b")).toBe("http://h:1/?token=a%20b");
  });

  test("resolveAuthToken honours a long env token and rejects a short one", async () => {
    const long = "x".repeat(40);
    expect(
      await Effect.runPromise(resolveAuthToken({ PEEKTRACE_TOKEN: long }))
    ).toBe(long);
    expect(
      await Effect.runPromise(resolveAuthToken({})).then((t) => t.length)
    ).toBe(43);
    const err = await Effect.runPromise(
      resolveAuthToken({ PEEKTRACE_TOKEN: "short" }).pipe(Effect.flip)
    );
    expect(err._tag).toBe("CliUserError");
  });
});

interface Running {
  readonly base: string;
  readonly port: number;
}

const scope = Effect.runSync(Scope.make());

/** Serve the real app on a free loopback port; torn down with `scope`. */
const startServer = async (token: string | undefined): Promise<Running> => {
  const port = await Effect.runPromise(findFreePort(41_000, "127.0.0.1"));
  const layer = Layer.mergeAll(
    BunHttpServer.layer({ port, hostname: "127.0.0.1" }),
    RpcSerialization.layerNdjson,
    makeHandlersLayer()
  );
  const context = await Effect.runPromise(Layer.buildWithScope(layer, scope));
  await Effect.runPromise(
    Effect.gen(function* () {
      const server = yield* HttpServer.HttpServer;
      const { app } = yield* buildServeApp("127.0.0.1", port, { token });
      yield* server.serve(app);
    }).pipe(Effect.provide(context), Scope.extend(scope))
  );
  return { base: `http://127.0.0.1:${port}`, port };
};

let authed: Running;
let open: Running;

beforeAll(async () => {
  authed = await startServer(TOKEN);
  open = await startServer(undefined);
});

afterAll(async () => {
  await Effect.runPromise(
    Scope.close(scope, { _tag: "Success", value: undefined } as never)
  );
});

const rpc = (base: string, headers: Record<string, string> = {}) =>
  fetch(`${base}/rpc`, { method: "POST", body: RPC_BODY, headers });

describe("authed server", () => {
  test("no credentials gives 401 on /rpc, / and /health", async () => {
    for (const res of [
      await rpc(authed.base),
      await fetch(`${authed.base}/`),
      await fetch(`${authed.base}/health`),
    ]) {
      expect(res.status).toBe(401);
      expect(res.headers.get("cache-control")).toBe("no-store");
      expect(res.headers.get("referrer-policy")).toBe("no-referrer");
    }
    const body = await (await fetch(`${authed.base}/`)).text();
    expect(body).toContain("peektrace serve");
  });

  test("wrong Bearer and wrong ?token= give 401", async () => {
    const bad = await rpc(authed.base, { authorization: "Bearer nope" });
    expect(bad.status).toBe(401);
    const q = await fetch(`${authed.base}/?token=nope`, { redirect: "manual" });
    expect(q.status).toBe(401);
  });

  test("right ?token= redirects, sets an HttpOnly cookie, keeps other params", async () => {
    const res = await fetch(`${authed.base}/?titlebar=inset&token=${TOKEN}`, {
      redirect: "manual",
    });
    expect(res.status).toBe(302);
    expect(res.headers.get("location")).toBe("/?titlebar=inset");
    expect(res.headers.get("referrer-policy")).toBe("no-referrer");
    const cookie = res.headers.get("set-cookie") ?? "";
    expect(cookie).toContain(`${cookieName(authed.port)}=${TOKEN}`);
    expect(cookie).toContain("HttpOnly");
    expect(cookie).toContain("SameSite=Lax");
    expect(cookie).toContain("Path=/");

    const pair = cookie.split(";")[0] ?? "";
    expect((await rpc(authed.base, { cookie: pair })).status).toBe(200);
    expect(
      (await fetch(`${authed.base}/health`, { headers: { cookie: pair } }))
        .status
    ).toBe(200);
  });

  test("right Bearer gives 200", async () => {
    const res = await rpc(authed.base, { authorization: `Bearer ${TOKEN}` });
    expect(res.status).toBe(200);
  });

  test("a LAN-style Host passes with a token and fails without", async () => {
    const host = `192.0.2.10:${authed.port}`;
    const ok = await rpc(authed.base, {
      authorization: `Bearer ${TOKEN}`,
      host,
    });
    expect(ok.status).toBe(200);
    const none = await rpc(authed.base, { host });
    expect(none.status).toBe(401);
  });

  test("Origin must equal the request Host", async () => {
    const host = `192.0.2.10:${authed.port}`;
    const good = await rpc(authed.base, {
      authorization: `Bearer ${TOKEN}`,
      host,
      origin: `http://${host}`,
    });
    expect(good.status).toBe(200);
    const bad = await rpc(authed.base, {
      authorization: `Bearer ${TOKEN}`,
      host,
      origin: "http://evil.example",
    });
    expect(bad.status).toBe(403);
  });

  test("client with the token succeeds, without it fails with 401", async () => {
    const list = (token: string | undefined) =>
      Effect.gen(function* () {
        const client = yield* createPeektraceClient(authed.base, { token });
        return yield* client.sessions.list({});
      }).pipe(Effect.scoped);
    expect(Array.isArray(await Effect.runPromise(list(TOKEN)))).toBe(true);
    const err = await Effect.runPromise(list(undefined).pipe(Effect.flip));
    expect(isRpcClientError(err)).toBe(true);
    if (isRpcClientError(err)) {
      expect(remoteErrorMessage(authed.base, err)).toContain("PEEKTRACE_TOKEN");
    }
  });
});

describe("loopback server without a token", () => {
  test("403 maps to a port hint", async () => {
    const res = await rpc(open.base, { host: `evil.example:${open.port}` });
    const request = HttpClientRequest.post(`${open.base}/rpc`);
    const err = new RpcClientError.RpcClientError({
      reason: "Protocol",
      message: "Failed",
      cause: new HttpClientError.ResponseError({
        request,
        response: HttpClientResponse.fromWeb(request, res),
        reason: "StatusCode",
      }),
    });
    expect(remoteErrorMessage(open.base, err)).toContain("ports match");
  });

  test("serves with no credentials and strict Host guard", async () => {
    expect((await rpc(open.base)).status).toBe(200);
    expect((await fetch(`${open.base}/health`)).status).toBe(200);
    expect(
      (await fetch(`${open.base}/health`)).headers.get("referrer-policy")
    ).toBeNull();
    const rebound = await rpc(open.base, { host: `evil.example:${open.port}` });
    expect(rebound.status).toBe(403);
  });
});
