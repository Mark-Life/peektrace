/** Effect-Atom data layer for the inspector.
 *
 * One configurable RPC client, exposed as an `AtomRpc.Tag`. Every procedure
 * becomes either:
 * - `PeektraceClient.query(tag, payload)` → `Atom<Result<Success, Error>>`, where
 *   `Result` is the loading/success/failure discriminated union (no flag bags);
 * - `PeektraceClient.mutation(tag)` → a writable `AtomResultFn` for create/update/
 *   delete (used by the Memory UI in the next phase).
 *
 * Transport is a single same-origin HTTP/NDJSON protocol layer pointed at
 * `${BASE_URL}/rpc`. `BASE_URL` defaults to `""` (same origin) so:
 * - dev: Vite proxies `/rpc` → a running `peektrace serve` (see `vite.config.ts`);
 * - prod: `peektrace serve` hosts both `dist/` and `/rpc` on the same origin.
 *
 * Override with `VITE_PEEKTRACE_BASE_URL` (e.g. an absolute `http://host:port`)
 * to point the UI at a remote server. The base URL is the single knob; nothing
 * else changes.
 */

import { FetchHttpClient, HttpClient } from "@effect/platform";
import { RpcClient, RpcSerialization } from "@effect/rpc";
import { AtomRpc } from "@effect-atom/atom-react";
import { PeektraceRpcs } from "@workspace/rpc/contract";
import { Effect, Layer } from "effect";

/** Same-origin by default; override to target a remote `peektrace serve`. */
const BASE_URL = import.meta.env.VITE_PEEKTRACE_BASE_URL ?? "";

/** Strip a trailing slash so `${BASE_URL}/rpc` never doubles up. */
const TRAILING_SLASH = /\/$/;

/**
 * Non-2xx (a 401 after the token rotates, a 403 guard) becomes a
 * `ResponseError`; otherwise the NDJSON decoder waits on a plain-text body
 * forever.
 */
const httpClient = Layer.effect(
  HttpClient.HttpClient,
  Effect.map(HttpClient.HttpClient, HttpClient.filterStatusOk)
).pipe(Layer.provide(FetchHttpClient.layer));

/** HTTP/NDJSON protocol layer pointed at `${BASE_URL}/rpc`. */
const protocol = RpcClient.layerProtocolHttp({
  url: `${BASE_URL.replace(TRAILING_SLASH, "")}/rpc`,
}).pipe(Layer.provide(RpcSerialization.layerNdjson), Layer.provide(httpClient));

/**
 * The single typed Peektrace RPC client atom. Exposes `.query` / `.mutation`
 * helpers that yield `Result`-typed atoms over the contract procedures.
 */
export class PeektraceClient extends AtomRpc.Tag<PeektraceClient>()(
  "PeektraceClient",
  {
    group: PeektraceRpcs,
    protocol,
  }
) {}
