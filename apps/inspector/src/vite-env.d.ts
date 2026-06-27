/// <reference types="vite/client" />

/** Typed inspector build-time env. Keeps the RPC base URL `string`, not `any`. */
interface ImportMetaEnv {
  /** Optional absolute base URL for the RPC client; default `""` (same origin). */
  readonly VITE_PEEPHOLE_BASE_URL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
