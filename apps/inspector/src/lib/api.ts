/** Typed fetch client for the `/api` JSON routes of `peektrace serve`.
 *
 * Every call is an `Effect` that fails with the server's typed `ApiError`
 * (`{"error": {...}}` bodies) or a `TransportError` (network faults and the
 * plain-text 401/403 guard answers). `BASE_URL` defaults to `""` (same
 * origin); set `VITE_PEEKTRACE_BASE_URL` to target another server.
 */
import type { ApiError } from "@workspace/view-core/api-types.gen";
import { Effect } from "effect";

/** A failure below the API: no answer, or an answer that is not JSON. */
export interface TransportError {
  readonly _tag: "TransportError";
  readonly message: string;
  readonly status?: number;
}

export type ApiFailure = ApiError | TransportError;

type QueryValue = string | number | undefined;

export type Query = Readonly<Record<string, QueryValue>>;

const TRAILING_SLASH = /\/$/u;

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
  typeof u.error === "object" &&
  u.error !== null &&
  "_tag" in u.error &&
  typeof u.error._tag === "string";

const isNonEmptyText = (u: unknown): u is string =>
  typeof u === "string" && u !== "";

const isJson = (res: Response) =>
  (res.headers.get("content-type") ?? "").startsWith("application/json");

/** Turn a non-2xx answer into its typed failure. */
const failureOf = (res: Response) =>
  Effect.tryPromise({
    catch: () => transport(`HTTP ${res.status}`, res.status),
    try: async (): Promise<ApiFailure> => {
      const fallback = `HTTP ${res.status}`;

      if (isJson(res)) {
        const body: unknown = await res.json();

        if (isApiErrorBody(body)) {
          return body.error;
        }

        return transport(isNonEmptyText(body) ? body : fallback, res.status);
      }

      const text = await res.text();

      return transport(text === "" ? fallback : text, res.status);
    },
  }).pipe(Effect.flatMap((failure) => Effect.fail(failure)));

const send = (url: string, init: RequestInit) =>
  Effect.tryPromise({
    catch: (e) => transport(e instanceof Error ? e.message : String(e)),
    try: async (signal) =>
      await fetch(url, { ...init, credentials: "same-origin", signal }),
  });

const readJson = <A>(res: Response) =>
  Effect.tryPromise({
    catch: () => transport("unexpected response", res.status),
    try: async () => {
      const body: unknown = await res.json();

      // SAFETY: the caller names the route's response type; the server answers 2xx with that JSON shape.
      // oxlint-disable-next-line typescript/no-unsafe-type-assertion -- generic response type is chosen by the caller per route
      return body as A;
    },
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
  // oxlint-disable-next-line anti-slop/no-object-parameters -- payloads are caller-owned interfaces of varying shape, only JSON-serialized here
  body?: object
): Effect.Effect<A, ApiFailure> =>
  send(apiUrl(path), {
    body: body === undefined ? undefined : JSON.stringify(body),
    headers: { accept: "application/json", "content-type": "application/json" },
    method,
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
            catch: () => transport("unexpected response", res.status),
            try: async () => await res.blob(),
          })
        : failureOf(res)
    )
  );
