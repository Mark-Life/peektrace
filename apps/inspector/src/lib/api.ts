/** Typed fetch client for the `/api` JSON routes of `peektrace serve`.
 *
 * Every call is an `Effect` that fails with the server's typed `ApiError`
 * (`{"error": {...}}` bodies) or a `TransportError` (network faults and the
 * plain-text 401/403 guard answers). `BASE_URL` defaults to `""` (same
 * origin); set `VITE_PEEKTRACE_BASE_URL` to target another server.
 */
import { Effect } from "effect";
import type { ApiError } from "./api-types.gen";

/** A failure below the API: no answer, or an answer that is not JSON. */
export interface TransportError {
  readonly _tag: "TransportError";
  readonly message: string;
  readonly status?: number;
}

export type ApiFailure = ApiError | TransportError;

type QueryValue = string | number | undefined;
export type Query = Readonly<Record<string, QueryValue>>;

const TRAILING_SLASH = /\/$/;

/** Same-origin by default; override to target a remote `peektrace serve`. */
export const BASE_URL = (import.meta.env.VITE_PEEKTRACE_BASE_URL ?? "").replace(
  TRAILING_SLASH,
  ""
);

/** `/api/<segments>`, each segment URI-encoded. */
export const apiPath = (...segments: readonly (string | number)[]) =>
  `/api/${segments.map((s) => encodeURIComponent(String(s))).join("/")}`;

/** Absolute request URL with the defined query params. */
export const apiUrl = (path: string, query: Query = {}) => {
  const params = new URLSearchParams();
  for (const [key, value] of Object.entries(query)) {
    if (value !== undefined) {
      params.set(key, String(value));
    }
  }
  const search = params.toString();
  return `${BASE_URL}${path}${search === "" ? "" : `?${search}`}`;
};

const transport = (message: string, status?: number): TransportError =>
  status === undefined
    ? { _tag: "TransportError", message }
    : { _tag: "TransportError", message, status };

const isApiErrorBody = (u: unknown): u is { readonly error: ApiError } =>
  typeof u === "object" &&
  u !== null &&
  "error" in u &&
  typeof (u as { error: unknown }).error === "object" &&
  (u as { error: { _tag?: unknown } | null }).error !== null &&
  typeof (u as { error: { _tag?: unknown } }).error._tag === "string";

const isJson = (res: Response) =>
  (res.headers.get("content-type") ?? "").startsWith("application/json");

/** Turn a non-2xx answer into its typed failure. */
const failureOf = (res: Response) =>
  Effect.tryPromise({
    try: () => (isJson(res) ? res.json() : res.text()),
    catch: () => transport(`HTTP ${res.status}`, res.status),
  }).pipe(
    Effect.flatMap((body: unknown) =>
      Effect.fail<ApiFailure>(
        isApiErrorBody(body)
          ? body.error
          : transport(
              typeof body === "string" && body !== ""
                ? body
                : `HTTP ${res.status}`,
              res.status
            )
      )
    )
  );

const send = (url: string, init: RequestInit) =>
  Effect.tryPromise({
    try: (signal) =>
      fetch(url, { ...init, credentials: "same-origin", signal }),
    catch: (e) => transport(e instanceof Error ? e.message : String(e)),
  });

const readJson = <A>(res: Response) =>
  Effect.tryPromise({
    try: () => res.json() as Promise<A>,
    catch: () => transport("unexpected response", res.status),
  });

/** `GET` a JSON route. */
export const apiGet = <A>(
  path: string,
  query?: Query
): Effect.Effect<A, ApiFailure> =>
  send(apiUrl(path, query), { headers: { accept: "application/json" } }).pipe(
    Effect.flatMap((res) => (res.ok ? readJson<A>(res) : failureOf(res)))
  );

/** Send a JSON body with `method` to a JSON route. */
export const apiSend = <A>(
  method: "POST" | "PUT" | "PATCH" | "DELETE",
  path: string,
  body?: unknown
): Effect.Effect<A, ApiFailure> =>
  send(apiUrl(path), {
    method,
    headers: { accept: "application/json", "content-type": "application/json" },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
  }).pipe(
    Effect.flatMap((res) => (res.ok ? readJson<A>(res) : failureOf(res)))
  );

/** `GET` raw bytes (images). */
export const apiBlob = (
  path: string,
  query?: Query
): Effect.Effect<Blob, ApiFailure> =>
  send(apiUrl(path, query), {}).pipe(
    Effect.flatMap((res) =>
      res.ok
        ? Effect.tryPromise({
            try: () => res.blob(),
            catch: () => transport("unexpected response", res.status),
          })
        : failureOf(res)
    )
  );
