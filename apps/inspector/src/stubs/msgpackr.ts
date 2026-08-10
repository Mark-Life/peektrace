/** Stand-in for `msgpackr`, aliased in by `vite.config.ts`.
 *
 * `@effect/rpc`'s `RpcSerialization` module offers both NDJSON and MessagePack
 * layers from one file, and pulls msgpackr in with a static
 * `import * as Msgpackr from "msgpackr"`. `RpcClient` imports that module, so
 * msgpackr lands in the bundle (~27 KB minified) whichever serialization the
 * app builds — and it is a side-effecting CJS package, so tree shaking cannot
 * drop it. The inspector's transport is NDJSON (`src/lib/client.ts`), so those
 * bytes are pure dead weight.
 *
 * `Packr`/`Unpackr` are only ever constructed inside `RpcSerialization`'s
 * MessagePack factory, never at module scope, so replacing them with throwing
 * constructors is inert unless the transport actually changes. If it ever does,
 * this throws at layer-construction time with an explanation instead of failing
 * obscurely — drop the alias and the real package comes back.
 */

const message =
  "msgpackr is stubbed out of the inspector bundle: the RPC transport is " +
  "NDJSON. Remove the `msgpackr` alias in apps/inspector/vite.config.ts to " +
  "use MessagePack serialization.";

/** Throwing stand-in for `msgpackr`'s encoder. */
export class Packr {
  constructor() {
    throw new Error(message);
  }
}

/** Throwing stand-in for `msgpackr`'s decoder. */
export class Unpackr {
  constructor() {
    throw new Error(message);
  }
}
