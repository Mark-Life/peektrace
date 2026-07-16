/** Best-effort "a newer version exists" notice for `peektrace serve` startup.
 *
 * A privacy departure from the otherwise-offline CLI: it makes one outbound call
 * to the GitHub releases API. So it is gated behind `PEEKTRACE_NO_UPDATE_CHECK`
 * (any non-empty value skips the network entirely), cached to a file under
 * `PEEKTRACE_DIR` for ~24h to avoid hammering the API, and wrapped so it can
 * never block startup or surface an error — offline, rate-limited (403) or any
 * failure is swallowed silently.
 */
import { homedir } from "node:os";
import { join } from "node:path";
import { Console, Effect } from "effect";
import {
  compareVersions,
  fetchLatestCliTag,
  type ReleaseConfig,
  resolveReleaseConfig,
} from "./release";

/** One day in milliseconds — the passive check runs at most once per this window. */
const CACHE_TTL_MS = 86_400_000;
const CHECK_TIMEOUT_MS = 1500;

/** Persisted result of the last update check (timestamp + last tag seen). */
interface UpdateCheckCache {
  readonly checkedAt: number;
  readonly latestTag: string | null;
}

/** The `update-check.json` cache path under `PEEKTRACE_DIR` (or `~/.peektrace`). */
const cachePath = (): string =>
  join(
    process.env.PEEKTRACE_DIR ?? join(homedir(), ".peektrace"),
    "update-check.json"
  );

/** Read the cache, or `null` on any missing/corrupt/unreadable file. */
const readCache = (): Effect.Effect<UpdateCheckCache | null> =>
  Effect.tryPromise(async () => {
    const file = Bun.file(cachePath());
    if (!(await file.exists())) {
      return null;
    }
    const parsed = (await file.json()) as Partial<UpdateCheckCache>;
    if (typeof parsed.checkedAt !== "number") {
      return null;
    }
    return {
      checkedAt: parsed.checkedAt,
      latestTag: typeof parsed.latestTag === "string" ? parsed.latestTag : null,
    };
  }).pipe(Effect.orElseSucceed(() => null));

/** Write the cache best-effort; any failure is ignored. */
const writeCache = (cache: UpdateCheckCache): Effect.Effect<void> =>
  Effect.tryPromise(async () => {
    const path = cachePath();
    await Bun.write(path, JSON.stringify(cache));
  }).pipe(Effect.ignore);

/**
 * The newest `cli-v*` tag if it is strictly newer than `currentVersion`, else
 * `null`. Uses the cached tag when the last check was < 24h ago, otherwise hits
 * the API and refreshes the cache. Any network failure resolves to `null`.
 */
const findAvailableUpdate = ({
  currentVersion,
  config,
  now = Date.now(),
  ttlMs = CACHE_TTL_MS,
}: {
  readonly currentVersion: string;
  readonly config: ReleaseConfig;
  readonly now?: number;
  readonly ttlMs?: number;
}): Effect.Effect<string | null> =>
  Effect.gen(function* () {
    const cache = yield* readCache();
    let latest: string | null;
    if (cache !== null && now - cache.checkedAt < ttlMs) {
      latest = cache.latestTag;
    } else {
      latest = yield* fetchLatestCliTag(config).pipe(
        Effect.orElseSucceed(() => null)
      );
      // Only a successful lookup opens the 24h window; a transient failure must
      // not cache `null` and suppress the notice until connectivity returns.
      if (latest !== null) {
        yield* writeCache({ checkedAt: now, latestTag: latest });
      }
    }
    if (latest === null) {
      return null;
    }
    return compareVersions(latest, currentVersion) > 0 ? latest : null;
  });

/**
 * Print a single update hint to stdout when a newer release exists, silent when
 * current. Gated by `PEEKTRACE_NO_UPDATE_CHECK`, bounded by a short timeout, and
 * fully error-swallowing — safe to `Effect.fork` at startup without ever
 * blocking or failing the server.
 */
export const startupUpdateNotice = (
  currentVersion: string
): Effect.Effect<void> =>
  Effect.gen(function* () {
    if (process.env.PEEKTRACE_NO_UPDATE_CHECK) {
      return;
    }
    const config = resolveReleaseConfig();
    const latest = yield* findAvailableUpdate({ currentVersion, config });
    if (latest !== null) {
      yield* Console.log(
        `A newer version (${latest}) is available — run 'peektrace upgrade'`
      );
    }
  }).pipe(Effect.timeout(CHECK_TIMEOUT_MS), Effect.ignore);
