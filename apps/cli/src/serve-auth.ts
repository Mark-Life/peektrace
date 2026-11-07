/** Token auth for non-loopback `serve` / `tui` binds.
 *
 * Loopback binds run without auth, as before. A bind to any other interface
 * requires a 256-bit token, accepted as a `Bearer` header, as an HttpOnly cookie,
 * or once as `?token=` (which sets the cookie and redirects the token out of the
 * URL). Everything here is pure except `resolveAuthToken`, which reads env.
 */

import { createHash, timingSafeEqual } from "node:crypto";
import { Options } from "@effect/cli";
import { Effect } from "effect";
import { CliUserError } from "./errors";

/** Env var that supplies the token (server) or sends it (`--remote`). */
export const TOKEN_ENV = "PEEKTRACE_TOKEN";
/** Shortest token accepted from `PEEKTRACE_TOKEN`. */
const MIN_TOKEN_LENGTH = 32;
const TOKEN_BYTES = 32;
const QUERY_PARAM = "token";
const BEARER_PREFIX = /^Bearer\s+/i;

/** True when `host` is a loopback interface (no off-box exposure). */
export const isLoopbackHost = (host: string) =>
  host === "127.0.0.1" || host === "localhost" || host === "::1";

/** Random 256-bit token, base64url (cookie- and URL-safe). */
export const makeToken = () =>
  Buffer.from(crypto.getRandomValues(new Uint8Array(TOKEN_BYTES))).toString(
    "base64url"
  );

const digest = (value: string) => createHash("sha256").update(value).digest();

/** Constant-time equality: both sides are hashed first, so length never leaks. */
export const tokensEqual = (a: string, b: string) =>
  timingSafeEqual(digest(a), digest(b));

/** Cookies are shared across ports on one host, so the port is part of the name. */
export const cookieName = (port: number) => `peektrace_token_${port}`;

/** Auth and write posture derived from the bind host and flags. */
export interface ServePosture {
  readonly auth: boolean;
  readonly readOnly: boolean;
}

/**
 * Non-loopback binds require a token and are read-only unless `--allow-write`.
 * `--read-only` always wins.
 */
export const servePosture = (args: {
  readonly host: string;
  readonly readOnly: boolean;
  readonly allowWrite: boolean;
}): ServePosture => {
  const auth = !isLoopbackHost(args.host);
  return {
    auth,
    readOnly: args.readOnly || (auth && !args.allowWrite),
  };
};

/** Token for an authed bind: `PEEKTRACE_TOKEN` when set (checked), else random. */
export const resolveAuthToken = (env: Record<string, string | undefined>) =>
  Effect.gen(function* () {
    const fromEnv = env[TOKEN_ENV];
    if (fromEnv === undefined || fromEnv === "") {
      return makeToken();
    }
    if (fromEnv.length < MIN_TOKEN_LENGTH) {
      return yield* new CliUserError({
        message: `${TOKEN_ENV} must be at least ${MIN_TOKEN_LENGTH} characters. Unset it to get a random token.`,
      });
    }
    return fromEnv;
  });

/** The slice of a request the auth decision reads. */
export interface AuthRequest {
  readonly cookies: Readonly<Record<string, string | undefined>>;
  readonly headers: Readonly<Record<string, string | undefined>>;
  readonly method: string;
  readonly url: string;
}

export type AuthOutcome =
  | { readonly _tag: "ok" }
  | { readonly _tag: "unauthorized" }
  | { readonly _tag: "redirect"; readonly location: string };

const bearerOf = (headers: AuthRequest["headers"]) => {
  const value = headers.authorization;
  return value !== undefined && BEARER_PREFIX.test(value)
    ? value.replace(BEARER_PREFIX, "")
    : undefined;
};

const LEADING_SLASHES = /^\/{2,}/;

/** Same path and query as `url`, minus the `token` param. */
const stripToken = (url: URL) => {
  const params = new URLSearchParams(url.search);
  params.delete(QUERY_PARAM);
  const query = params.toString();
  // A leading `//` would make the redirect protocol-relative (off-site).
  const path = url.pathname.replace(LEADING_SLASHES, "/");
  return `${path}${query === "" ? "" : `?${query}`}`;
};

/** Decide a request: Bearer header, then cookie, then `?token=` on GET. */
export const authOutcome = (
  req: AuthRequest,
  token: string,
  port: number
): AuthOutcome => {
  const bearer = bearerOf(req.headers);
  if (bearer !== undefined && tokensEqual(bearer, token)) {
    return { _tag: "ok" };
  }
  const cookie = req.cookies[cookieName(port)];
  if (cookie !== undefined && tokensEqual(cookie, token)) {
    return { _tag: "ok" };
  }
  if (req.method === "GET") {
    const url = new URL(req.url, "http://peektrace.invalid");
    const queryToken = url.searchParams.get(QUERY_PARAM);
    if (queryToken !== null && tokensEqual(queryToken, token)) {
      return { _tag: "redirect", location: stripToken(url) };
    }
  }
  return { _tag: "unauthorized" };
};

/** URL a human opens: the base URL with the token as a query param. */
export const tokenUrl = (baseUrl: string, token: string) =>
  `${baseUrl}/?${QUERY_PARAM}=${encodeURIComponent(token)}`;

const HTTP_PROTOCOL = /^https?:$/;
const TOKEN_PARAM_VALUE = /([?&]token=)[^&#]*/g;

/**
 * Split a `--remote` value into a clean base URL and the `token` query param
 * (if any). An empty token counts as none. Anything that is not an http(s) URL
 * passes through with any `token` value masked, so validation errors never
 * echo the secret.
 */
export const splitRemote = (remote: string) => {
  if (URL.canParse(remote)) {
    const parsed = new URL(remote);
    if (HTTP_PROTOCOL.test(parsed.protocol)) {
      return parsed.searchParams.has(QUERY_PARAM)
        ? {
            url: parsed.origin,
            token: parsed.searchParams.get(QUERY_PARAM) || undefined,
          }
        : { url: remote, token: undefined };
    }
  }
  return {
    url: remote.replace(TOKEN_PARAM_VALUE, "$1***"),
    token: undefined,
  };
};

/** `--allow-write` flag shared by `serve` and `tui`. */
export const allowWriteOpt = Options.boolean("allow-write").pipe(
  Options.withDescription(
    "Allow writes on a non-loopback --host (read-only by default there)"
  )
);

/** Posture plus the token to enforce (set only when auth is on). */
export const resolveServeAuth = (args: {
  readonly host: string;
  readonly readOnly: boolean;
  readonly allowWrite: boolean;
  readonly env: Record<string, string | undefined>;
}) =>
  Effect.gen(function* () {
    const posture = servePosture(args);
    const token = posture.auth ? yield* resolveAuthToken(args.env) : undefined;
    return { ...posture, token };
  });
