/** Effect-Atom data layer for the inspector.
 *
 * One configurable RPC client, exposed as an `AtomRpc.Tag`. Every procedure
 * becomes either:
 * - `PeepholeClient.query(tag, payload)` → `Atom<Result<Success, Error>>`, where
 *   `Result` is the loading/success/failure discriminated union (no flag bags);
 * - `PeepholeClient.mutation(tag)` → a writable `AtomResultFn` for create/update/
 *   delete (used by the Memory UI in the next phase).
 *
 * Transport is a single same-origin HTTP/NDJSON protocol layer pointed at
 * `${BASE_URL}/rpc`. `BASE_URL` defaults to `""` (same origin) so:
 * - dev: Vite proxies `/rpc` → a running `peephole serve` (see `vite.config.ts`);
 * - prod: `peephole serve` hosts both `dist/` and `/rpc` on the same origin.
 *
 * Override with `VITE_PEEPHOLE_BASE_URL` (e.g. an absolute `http://host:port`)
 * to point the UI at a remote server. The base URL is the single knob; nothing
 * else changes.
 */

import { FetchHttpClient } from "@effect/platform";
import { RpcClient, RpcSerialization } from "@effect/rpc";
import { AtomRpc } from "@effect-atom/atom-react";
import { PeepholeRpcs } from "@workspace/rpc/contract";
import { Layer } from "effect";

/** Same-origin by default; override to target a remote `peephole serve`. */
const BASE_URL = import.meta.env.VITE_PEEPHOLE_BASE_URL ?? "";

/** Strip a trailing slash so `${BASE_URL}/rpc` never doubles up. */
const TRAILING_SLASH = /\/$/;

/** HTTP/NDJSON protocol layer pointed at `${BASE_URL}/rpc`. */
const protocol = RpcClient.layerProtocolHttp({
  url: `${BASE_URL.replace(TRAILING_SLASH, "")}/rpc`,
}).pipe(
  Layer.provide(RpcSerialization.layerNdjson),
  Layer.provide(FetchHttpClient.layer)
);

/**
 * The single typed Peephole RPC client atom. Exposes `.query` / `.mutation`
 * helpers that yield `Result`-typed atoms over the contract procedures.
 */
export class PeepholeClient extends AtomRpc.Tag<PeepholeClient>()(
  "PeepholeClient",
  {
    group: PeepholeRpcs,
    protocol,
  }
) {}
