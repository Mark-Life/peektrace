/** CLI error surface: a typed user error plus a clean one-line renderer.
 *
 * The root command's error boundary (`index.ts`) catches these — and the RPC
 * domain errors that flow up through `withClient` — and prints only the rendered
 * message to stderr, never a raw Effect FiberFailure / bunfs stack dump.
 */
import { HttpClientError } from "@effect/platform";
import { RpcClientError } from "@effect/rpc";
import { Data } from "effect";

/** A user-facing CLI error: rendered as a clean one-liner, never a stack dump. */
export class CliUserError extends Data.TaggedError("CliUserError")<{
  readonly message: string;
}> {}

interface Tagged {
  readonly _tag: string;
}

const isRecord = (u: unknown): u is Record<string, unknown> =>
  typeof u === "object" && u !== null;

/** Narrow an unknown value to a tagged (`_tag`) object. */
const isTagged = (u: unknown): u is Tagged => isRecord(u) && "_tag" in u;

/**
 * Human message for a known domain/user error; a generic message fallback for
 * anything else. Used by the CLI error boundary to render expected failures
 * cleanly. Structural (`_tag`-keyed) so it works for both in-process instances
 * and schema-decoded remote errors without importing every error class.
 */
export const formatCliError = (error: unknown): string => {
  if (!isTagged(error)) {
    return error instanceof Error ? error.message : String(error);
  }
  const e = error as Tagged & Record<string, unknown>;
  switch (e._tag) {
    case "CliUserError":
      return String(e.message);
    case "SessionNotFoundError":
      return `Session "${String(e.id)}" not found (searched ${String(e.searchedRoot)}).`;
    case "MemoryNotFoundError":
      return `Memory "${String(e.name)}" not found in project "${String(e.project)}".`;
    case "MemoryValidationError": {
      // `.name` is also the base Error's name (the tag); only treat it as the
      // schema's memory-name field when it differs from the tag.
      const named =
        typeof e.name === "string" && e.name !== e._tag ? ` "${e.name}"` : "";
      return `Invalid memory${named}: ${String(e.reason)}`;
    }
    case "FileChangedError":
      return `File changed on disk (${String(e.reason)}): ${String(e.path)}. Reload and retry.`;
    case "PathOutsideRootError":
      return `Refused: path escapes the allowed roots: ${String(e.path)}`;
    case "CapabilityUnsupportedError":
      return `Unsupported for agent "${String(e.agentId)}": ${String(e.capabilityId)}`;
    case "TranscriptParseError":
      return `Failed to parse transcript ${String(e.path)}: ${String(e.reason)}`;
    case "StatsScanError":
      return `Could not scan the corpus: ${String(e.reason)}`;
    case "StatsRowNotFoundError":
      return `No ${String(e.table)} row with key "${String(e.key)}". Run \`peektrace stats\` and copy a DRILL cell.`;
    default:
      return typeof e.message === "string" ? e.message : `Error: ${e._tag}`;
  }
};

/** True for the `@effect/rpc` transport error. */
export const isRpcClientError = (
  u: unknown
): u is RpcClientError.RpcClientError =>
  u instanceof RpcClientError.RpcClientError;

/** Most specific detail from a transport error: HTTP status or root error code. */
const transportDetail = (error: RpcClientError.RpcClientError): string => {
  const { cause } = error;
  if (HttpClientError.isHttpClientError(cause)) {
    if (cause._tag === "ResponseError") {
      return `HTTP ${cause.response.status}`;
    }
    const root = cause.cause;
    if (isRecord(root) && typeof root.code === "string") {
      return root.code;
    }
  }
  return error.message;
};

const HTTP_PROTOCOL = /^https?:$/;

const isHttpUrl = (url: string): boolean =>
  URL.canParse(url) && HTTP_PROTOCOL.test(new URL(url).protocol);

const UNAUTHORIZED = 401;
const FORBIDDEN = 403;

const httpStatus = (
  error: RpcClientError.RpcClientError
): number | undefined => {
  const { cause } = error;
  return HttpClientError.isHttpClientError(cause) &&
    cause._tag === "ResponseError"
    ? cause.response.status
    : undefined;
};

/** One-line message for a failed `--remote` transport call. */
export const remoteErrorMessage = (
  url: string,
  error: RpcClientError.RpcClientError
): string => {
  if (url.trim() === "") {
    return "Invalid --remote URL: it is empty. Use `http://host:port`.";
  }
  const detail = transportDetail(error);
  const status = httpStatus(error);
  if (status === UNAUTHORIZED) {
    return `Remote request to ${url} was unauthorized (HTTP 401). Set PEEKTRACE_TOKEN or pass the printed ?token= URL.`;
  }
  if (status === FORBIDDEN) {
    return `Remote request to ${url} was refused (HTTP 403). Check that the local and remote ports match when using ssh -L (the Host header must be the server's own address).`;
  }
  if (!url.includes("://")) {
    return `Remote request to ${url} failed (${detail}). Include the scheme, for example \`http://host:port\`.`;
  }
  if (!isHttpUrl(url)) {
    return `Invalid --remote URL ${url}. Use \`http://host:port\`.`;
  }
  return `Remote request to ${url} failed (${detail}). Check that \`peektrace serve\` is running there.`;
};
