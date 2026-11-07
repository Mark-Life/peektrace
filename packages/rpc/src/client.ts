/** Typed Peektrace RPC clients.
 *
 * Two transports, one contract:
 * - `createPeektraceClient(baseUrl)` — HTTP transport (NDJSON over `fetch`) for the
 *   inspector UI and the CLI `--remote` mode. Returns a scoped `Effect` yielding a
 *   fully-typed client; the caller supplies the `Scope`.
 * - `makeInProcessClient()` — drives the group directly against an in-process
 *   handlers layer (no network), for the CLI in-process mode and tests.
 *
 * Types flow disk -> core -> client: every method returns `Effect` of the core
 * success schema with the typed wire-error channel, zero `any`.
 */
import {
  FetchHttpClient,
  HttpClient,
  HttpClientRequest,
} from "@effect/platform";
import { RpcClient, RpcSerialization, RpcTest } from "@effect/rpc";
import { Effect, Layer } from "effect";
import { PeektraceRpcs } from "./contract";

/** Default mount path for the RPC endpoint on the `serve` HTTP server. */
const RPC_PATH = "/rpc";

/** Trailing-slash matcher for normalizing the base URL. */
const TRAILING_SLASH = /\/$/;

/** Optional client settings. */
export interface ClientOptions {
  /** Sent as `Authorization: Bearer <token>` when the server requires a token. */
  readonly token?: string | undefined;
}

/**
 * Build the HTTP protocol layer for the client: NDJSON serialization over a
 * `fetch`-based HTTP client pointed at `${baseUrl}/rpc`.
 */
const protocolLayer = (baseUrl: string, options?: ClientOptions) => {
  const token = options?.token;
  // Non-2xx (401 bad token, 403 guard) becomes a `ResponseError`; otherwise the
  // NDJSON decoder would wait on a plain-text body forever.
  const httpClient = Layer.effect(
    HttpClient.HttpClient,
    Effect.map(HttpClient.HttpClient, (client) =>
      HttpClient.filterStatusOk(
        token === undefined || token === ""
          ? client
          : HttpClient.mapRequest(client, HttpClientRequest.bearerToken(token))
      )
    )
  ).pipe(Layer.provide(FetchHttpClient.layer));
  return RpcClient.layerProtocolHttp({
    url: `${baseUrl.replace(TRAILING_SLASH, "")}${RPC_PATH}`,
  }).pipe(
    Layer.provide(RpcSerialization.layerNdjson),
    Layer.provide(httpClient)
  );
};

/**
 * Create a typed HTTP RPC client against a running `peektrace serve` at `baseUrl`
 * (e.g. `http://127.0.0.1:4321`). Use within a `Scope`:
 *
 * ```ts
 * const program = Effect.gen(function* () {
 *   const client = yield* createPeektraceClient("http://127.0.0.1:4321");
 *   return yield* client.capabilities.list();
 * }).pipe(Effect.scoped);
 * ```
 */
export const createPeektraceClient = (
  baseUrl: string,
  options?: ClientOptions
) =>
  RpcClient.make(PeektraceRpcs).pipe(
    Effect.provide(protocolLayer(baseUrl, options))
  );

/**
 * Drive the group directly through an in-process handlers layer (no transport).
 * Provide the handlers layer (from `makeHandlersLayer`) plus a `Scope`.
 */
export const makeInProcessClient = () => RpcTest.makeClient(PeektraceRpcs);

/** The protocol layer factory, exposed for advanced wiring/testing. */
export { protocolLayer };
