/** Release resolution for `peektrace upgrade` and the startup update check.
 *
 * Mirrors the contract of `scripts/install.sh`: the same env overrides, the same
 * per-platform asset names, the same "newest `cli-v*` tag" resolution and the
 * same `SHA256SUMS` line format. Splitting the pure helpers (asset detection, tag
 * and checksum parsing, version comparison) from the IO keeps them unit-testable
 * against fixture strings with no network. The IO honours the same env overrides
 * so it can run against a local fake release server in tests.
 */
import { Effect } from "effect";
import { CliUserError } from "../errors";

const DEFAULT_BASE_URL =
  "https://github.com/Mark-Life/peektrace/releases/download";
const DEFAULT_GITHUB_API = "https://api.github.com/repos/Mark-Life/peektrace";
const RELEASES_PER_PAGE = 100;
const CLI_TAG_PREFIX = /^cli-v/;
const CLI_OR_V_PREFIX = /^(cli-)?v/;
const LEADING_V = /^v/;
const VERSION_SEP = /\./;
const WHITESPACE = /\s+/;
const DECIMAL_RADIX = 10;

/** Human summary of the platforms with a prebuilt binary (kept in sync with the installers). */
export const SUPPORTED_PLATFORMS =
  "macOS (arm64, x64), Linux (x64), Windows (x64)";

/** Resolved release endpoints + optional pinned tag, from env with install.sh defaults. */
export interface ReleaseConfig {
  /** Release-download base; asset URLs are `${baseUrl}/${tag}/${asset}`. */
  readonly baseUrl: string;
  /** GitHub repo API base; releases are listed at `${githubApi}/releases`. */
  readonly githubApi: string;
  /** A pinned release tag from `PEEKTRACE_VERSION`, or `undefined` for latest. */
  readonly pinnedVersion: string | undefined;
}

/**
 * Resolve the release config from the environment, applying the install.sh
 * defaults for anything unset. An empty `PEEKTRACE_VERSION` is treated as unset.
 */
export const resolveReleaseConfig = (
  env: NodeJS.ProcessEnv = process.env
): ReleaseConfig => ({
  baseUrl: env.PEEKTRACE_BASE_URL || DEFAULT_BASE_URL,
  githubApi: env.PEEKTRACE_GITHUB_API || DEFAULT_GITHUB_API,
  pinnedVersion: env.PEEKTRACE_VERSION || undefined,
});

/** Outcome of mapping a host platform/arch to a release asset name. */
export type AssetDetection =
  | {
      readonly _tag: "supported";
      readonly asset: string;
      readonly os: "darwin" | "linux" | "windows";
    }
  | { readonly _tag: "unsupported"; readonly reason: string };

/** Normalise Node (`process.arch`) and uname arch spellings to `arm64` / `x64`. */
const normalizeArch = (arch: string): "arm64" | "x64" | "other" => {
  if (arch === "arm64" || arch === "aarch64") {
    return "arm64";
  }
  if (arch === "x64" || arch === "x86_64" || arch === "amd64") {
    return "x64";
  }
  return "other";
};

/**
 * Map a host platform/arch to its prebuilt asset name, mirroring
 * `scripts/install.sh`' `detect_asset` (plus the Windows `.exe` from
 * `install.ps1`). Accepts both Node (`process.platform`/`process.arch`) and
 * uname spellings. Pure.
 */
export const detectAsset = (platform: string, arch: string): AssetDetection => {
  const a = normalizeArch(arch);
  if (platform === "darwin" || platform === "Darwin") {
    if (a === "arm64") {
      return {
        _tag: "supported",
        asset: "peektrace-darwin-arm64",
        os: "darwin",
      };
    }
    if (a === "x64") {
      return { _tag: "supported", asset: "peektrace-darwin-x64", os: "darwin" };
    }
    return {
      _tag: "unsupported",
      reason: `unsupported macOS architecture: ${arch}`,
    };
  }
  if (platform === "linux" || platform === "Linux") {
    if (a === "x64") {
      return { _tag: "supported", asset: "peektrace-linux-x64", os: "linux" };
    }
    return {
      _tag: "unsupported",
      reason: `unsupported Linux architecture: ${arch} (only linux-x64 has a prebuilt binary)`,
    };
  }
  if (platform === "win32" || platform === "windows") {
    if (a === "x64") {
      return {
        _tag: "supported",
        asset: "peektrace-windows-x64.exe",
        os: "windows",
      };
    }
    return {
      _tag: "unsupported",
      reason: `unsupported Windows architecture: ${arch}`,
    };
  }
  return {
    _tag: "unsupported",
    reason: `unsupported operating system: ${platform}`,
  };
};

/**
 * The newest `cli-v*` tag in a parsed GitHub releases array, or `null`. The API
 * returns releases newest-first, so the first match wins; non-`cli-v*` tags
 * (e.g. `desktop-v*`, which share this repo) are skipped. Pure.
 */
export const parseLatestCliTag = (releases: unknown): string | null => {
  if (!Array.isArray(releases)) {
    return null;
  }
  for (const release of releases) {
    const tag = (release as { readonly tag_name?: unknown }).tag_name;
    if (typeof tag === "string" && CLI_TAG_PREFIX.test(tag)) {
      return tag;
    }
  }
  return null;
};

/**
 * Extract the expected lowercase hex digest for `asset` from `SHA256SUMS` text,
 * or `null` when absent. Lines look like `<hex>  <asset-name>`. Pure.
 */
export const parseChecksum = (
  sumsText: string,
  asset: string
): string | null => {
  for (const line of sumsText.split("\n")) {
    const trimmed = line.trim();
    if (trimmed === "") {
      continue;
    }
    const fields = trimmed.split(WHITESPACE);
    const name = fields.at(-1);
    const hex = fields[0];
    if (name === asset && hex) {
      return hex.toLowerCase();
    }
  }
  return null;
};

/** Numeric cores of a dotted version, stripping any leading `cli-v`/`v`. */
const versionParts = (version: string): readonly number[] =>
  version
    .replace(CLI_OR_V_PREFIX, "")
    .split(VERSION_SEP)
    .map((segment) => {
      const n = Number.parseInt(segment, DECIMAL_RADIX);
      return Number.isNaN(n) ? 0 : n;
    });

/**
 * Compare two dotted numeric versions, ignoring any `cli-v`/`v` prefix. A
 * pre-release suffix (`-dev`, `-beta`) sorts as its numeric core (`0.0.0-dev` →
 * `[0,0,0]`). Returns `-1` when `a < b`, `1` when `a > b`, else `0`. Pure.
 */
export const compareVersions = (a: string, b: string): -1 | 0 | 1 => {
  const pa = versionParts(a);
  const pb = versionParts(b);
  const len = Math.max(pa.length, pb.length);
  for (let i = 0; i < len; i++) {
    const x = pa[i] ?? 0;
    const y = pb[i] ?? 0;
    if (x > y) {
      return 1;
    }
    if (x < y) {
      return -1;
    }
  }
  return 0;
};

/** Coerce any accepted version spelling to a canonical `cli-vX.Y.Z` tag. */
export const normalizeCliTag = (version: string): string => {
  if (CLI_TAG_PREFIX.test(version)) {
    return version;
  }
  return `cli-v${version.replace(LEADING_V, "")}`;
};

const jsonHeaders = {
  "User-Agent": "peektrace-cli",
  Accept: "application/vnd.github+json",
};

/** Fetch and parse the releases list (up to 100), failing as a `CliUserError`. */
export const fetchReleasesJson = (
  config: ReleaseConfig
): Effect.Effect<unknown, CliUserError> =>
  Effect.tryPromise({
    try: async (signal) => {
      const url = `${config.githubApi}/releases?per_page=${RELEASES_PER_PAGE}`;
      const res = await fetch(url, { signal, headers: jsonHeaders });
      if (!res.ok) {
        throw new Error(`GitHub API responded ${res.status}`);
      }
      return (await res.json()) as unknown;
    },
    catch: (cause) =>
      new CliUserError({
        message: `Failed to query GitHub releases: ${String(cause)}`,
      }),
  });

/** Resolve the newest `cli-v*` release tag from the API, or `null` if none. */
export const fetchLatestCliTag = (
  config: ReleaseConfig
): Effect.Effect<string | null, CliUserError> =>
  Effect.map(fetchReleasesJson(config), parseLatestCliTag);

/**
 * The tag to install: the explicit `pinned` flag, else `PEEKTRACE_VERSION`, else
 * the newest `cli-v*` from the API. Fails cleanly when no release can be found.
 */
export const resolveTargetTag = (
  config: ReleaseConfig,
  pinned?: string
): Effect.Effect<string, CliUserError> =>
  Effect.gen(function* () {
    const pin = pinned ?? config.pinnedVersion;
    if (pin) {
      return normalizeCliTag(pin);
    }
    const latest = yield* fetchLatestCliTag(config);
    if (latest === null) {
      return yield* new CliUserError({
        message: "Could not find a cli-v* release to upgrade to.",
      });
    }
    return latest;
  });

/** URL of the release asset for `tag`. */
export const assetUrl = (config: ReleaseConfig, tag: string, asset: string) =>
  `${config.baseUrl}/${tag}/${asset}`;

/** URL of the `SHA256SUMS` manifest for `tag`. */
export const checksumsUrl = (config: ReleaseConfig, tag: string) =>
  `${config.baseUrl}/${tag}/SHA256SUMS`;

/** Download `url` as raw bytes, failing as a `CliUserError`. */
export const downloadBytes = (
  url: string
): Effect.Effect<Uint8Array, CliUserError> =>
  Effect.tryPromise({
    try: async (signal) => {
      const res = await fetch(url, { signal });
      if (!res.ok) {
        throw new Error(`download responded ${res.status}`);
      }
      return new Uint8Array(await res.arrayBuffer());
    },
    catch: (cause) =>
      new CliUserError({
        message: `Download failed (${url}): ${String(cause)}`,
      }),
  });

/** Fetch `url` as text, failing as a `CliUserError`. */
export const fetchText = (url: string): Effect.Effect<string, CliUserError> =>
  Effect.tryPromise({
    try: async (signal) => {
      const res = await fetch(url, { signal });
      if (!res.ok) {
        throw new Error(`fetch responded ${res.status}`);
      }
      return await res.text();
    },
    catch: (cause) =>
      new CliUserError({ message: `Fetch failed (${url}): ${String(cause)}` }),
  });
