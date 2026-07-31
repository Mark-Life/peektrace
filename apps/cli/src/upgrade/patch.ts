/** Delta (binary-patch) upgrade path for `peektrace upgrade`.
 *
 * A release ships `<asset>.patch` — a TRDIFF10 (bsdiff + zstd) delta from the
 * previous `cli-v*` release — alongside the full binary. `binpatch` walks the
 * releases between the installed version and the target, applies each patch in
 * order, and verifies the result against the target asset's sha256 digest. A
 * typical hop is tens of kilobytes against a ~110 MB binary; a release that
 * bumps Bun's own runtime bytes blows the size gate and ships no patch.
 *
 * Everything here is best-effort: any miss (no chain, network failure, corrupt
 * patch) resolves to `null` and the caller falls back to the full download,
 * which stays the only path that must work.
 */
import { githubReleaseSource, resolveAndApply } from "binpatch";
import { Effect } from "effect";
import { compareVersions, type ReleaseConfig } from "./release";

/** Marker Bun stamps into `Bun.main` inside a `--compile`d single-file binary. */
const BUNFS_PREFIX = "/$bunfs/";
const CLI_TAG_PREFIX = "cli-v";
/** Releases to pull when filtering the listing down to `cli-v*` (see `cliReleasesFetch`). */
const CLI_RELEASE_WINDOW = 100;

/** A GitHub release as far as the tag filter is concerned. */
interface TaggedRelease {
  readonly tag_name?: unknown;
}

/**
 * Keep only `cli-v*` entries of a parsed GitHub releases listing.
 *
 * `binpatch` builds its chain from the slice of releases between the installed
 * and target tags, and treats any release in that slice without a `.patch`
 * asset as a broken publish. This repo also ships `desktop-v*` releases from
 * the same tag namespace, so they have to be dropped before the chain is
 * derived or every CLI upgrade spanning a desktop release looks malformed.
 * Non-array input is passed through untouched. Pure.
 */
export const filterCliReleases = (releases: unknown): unknown => {
  if (!Array.isArray(releases)) {
    return releases;
  }
  return releases.filter((release) => {
    const tag = (release as TaggedRelease).tag_name;
    return typeof tag === "string" && tag.startsWith(CLI_TAG_PREFIX);
  });
};

/** Read the request URL out of any `fetch` input spelling. */
const requestUrl = (input: RequestInfo | URL): string => {
  if (typeof input === "string") {
    return input;
  }
  if (input instanceof URL) {
    return input.href;
  }
  return input.url;
};

/**
 * A `fetch` that narrows the releases listing to `cli-v*` and leaves every
 * other request (the patch downloads) alone. It also widens the page size,
 * because `binpatch`' own window is sized in releases, not in *CLI* releases —
 * interleaved `desktop-v*` tags would otherwise push the installed version out
 * of view and silently disable delta upgrades.
 */
export const cliReleasesFetch = (releasesUrl: string): typeof fetch =>
  // `preconnect` is carried over so the wrapper is a complete stand-in for the
  // global `fetch`, which is what the source's config asks for.
  Object.assign(
    async (...args: Parameters<typeof fetch>) => {
      const [input, init] = args;
      if (!requestUrl(input).startsWith(releasesUrl)) {
        return await fetch(...args);
      }
      const response = await fetch(
        `${releasesUrl}?per_page=${CLI_RELEASE_WINDOW}`,
        init
      );
      if (!response.ok) {
        return response;
      }
      return Response.json(filterCliReleases(await response.json()));
    },
    { preconnect: fetch.preconnect }
  );

/** `true` when this process is a `bun build --compile` binary rather than `bun run`. */
export const isCompiledBinary = (): boolean =>
  Bun.main.startsWith(BUNFS_PREFIX);

/**
 * Try to reach `targetTag` by patching `oldPath` into `destPath`, returning the
 * number of patch bytes downloaded, or `null` when no delta path was usable.
 *
 * Only ever attempted for a forward upgrade of a compiled binary: patches are
 * generated old→new, and under `bun run` the running executable is Bun itself,
 * not something any chain can be applied to. Never fails — a `null` means
 * "fall back to the full download".
 */
export const tryDeltaUpgrade = ({
  config,
  asset,
  currentTag,
  targetTag,
  oldPath,
  destPath,
}: {
  readonly config: ReleaseConfig;
  readonly asset: string;
  readonly currentTag: string;
  readonly targetTag: string;
  readonly oldPath: string;
  readonly destPath: string;
}): Effect.Effect<number | null> =>
  Effect.tryPromise(async () => {
    if (compareVersions(targetTag, currentTag) <= 0) {
      return null;
    }
    const releasesUrl = `${config.githubApi}/releases`;
    const result = await resolveAndApply({
      source: githubReleaseSource({
        releasesUrl,
        binaryName: asset,
        userAgent: "peektrace-cli",
        fetch: cliReleasesFetch(releasesUrl),
      }),
      currentVersion: currentTag,
      targetVersion: targetTag,
      oldPath,
      destPath,
    });
    return result === null ? null : result.patchBytes;
  }).pipe(Effect.orElseSucceed(() => null));
