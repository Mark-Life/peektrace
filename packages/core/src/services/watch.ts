/** Filesystem-driven freshness for the inspector.
 *
 * `WatchService` runs the `@effect/platform` recursive file watcher over the
 * Claude projects root (resolved by `AgentRegistry`, honoring
 * `PEEPHOLE_CLAUDE_PROJECTS`) and turns raw `WatchEvent`s into coarse
 * invalidation signals the UI can act on: a memory `.md` file changing bumps the
 * `memory` scope, a session `.jsonl` file changing bumps `sessions`.
 *
 * Rapid bursts (editors temp-write + rename, agents appending many JSONL lines)
 * are coalesced with `Stream.groupedWithin`, so a flurry of raw events advances a
 * scope's monotonic version by exactly one. The service exposes both a pull
 * snapshot (`versions`, polled by `watch.poll`) and a push `changes` stream (for
 * a future streaming RPC). Read-only: it never writes to disk.
 */
import { FileSystem } from "@effect/platform";
import {
  Cause,
  Context,
  Duration,
  Effect,
  Layer,
  PubSub,
  Ref,
  Stream,
} from "effect";
import { AgentRegistry } from "./agents";

/** Coarse refresh scopes the UI subscribes to. */
export type WatchScope = "memory" | "sessions";

/** One coalesced invalidation: which scope changed, and (when derivable) where. */
export interface Invalidation {
  readonly project?: string;
  readonly scope: WatchScope;
}

/** Monotonic per-scope version counters; any increase means "refetch this scope". */
export interface WatchVersionsShape {
  readonly memory: number;
  readonly sessions: number;
}

/** Service contract: a pull snapshot plus a push stream of invalidations. */
export interface WatchServiceShape {
  /** Hot stream of coalesced invalidations (post-subscription events only). */
  readonly changes: Stream.Stream<Invalidation>;
  /** Current monotonic versions; the `watch.poll` RPC returns this. */
  readonly versions: Effect.Effect<WatchVersionsShape>;
}

/** Filesystem watcher over the agent roots, emitting coalesced invalidations. */
export class WatchService extends Context.Tag("@peephole/WatchService")<
  WatchService,
  WatchServiceShape
>() {}

/** Coalescing window (ms): raw events within this span fold into one bump. */
const DEBOUNCE_MS = 150;
const DEBOUNCE = Duration.millis(DEBOUNCE_MS);
/** Cap a single batch so a huge burst still flushes promptly. */
const MAX_BATCH = 256;

const JSONL = /\.jsonl$/;
const MARKDOWN = /\.md$/;
const PATH_SEP = /[/\\]/;

/**
 * Classify a changed path into an invalidation scope, or `null` to ignore.
 * `project` is the first path segment under the projects root (the Claude slug),
 * when the path sits inside the root.
 */
const classify = ({
  path,
  root,
}: {
  readonly path: string;
  readonly root: string;
}): Invalidation | null => {
  const rel = path.startsWith(root) ? path.slice(root.length) : path;
  const segments = rel.split(PATH_SEP).filter((s) => s.length > 0);
  const project = segments[0];
  const isMemory = segments.includes("memory") && MARKDOWN.test(path);
  if (isMemory) {
    return project === undefined
      ? { scope: "memory" }
      : { scope: "memory", project };
  }
  if (JSONL.test(path)) {
    return project === undefined
      ? { scope: "sessions" }
      : { scope: "sessions", project };
  }
  return null;
};

/** De-duplicate a batch of invalidations by `scope:project`. */
const dedupe = (batch: readonly Invalidation[]): readonly Invalidation[] => {
  const seen = new Map<string, Invalidation>();
  for (const inv of batch) {
    seen.set(`${inv.scope}:${inv.project ?? ""}`, inv);
  }
  return [...seen.values()];
};

/**
 * Live `WatchService`: forks a scoped fiber that drains the recursive watcher,
 * coalescing bursts into version bumps + published invalidations. If the projects
 * root is absent (fresh machine, or a not-yet-created temp dir) the service is a
 * no-op that reports zero versions and an empty stream rather than failing boot.
 */
export const WatchServiceLive = Layer.scoped(
  WatchService,
  Effect.gen(function* () {
    const fs = yield* FileSystem.FileSystem;
    const agents = yield* AgentRegistry;
    const root = yield* agents
      .projectsRoot("claude")
      .pipe(Effect.orElseSucceed(() => ""));
    const versionsRef = yield* Ref.make<WatchVersionsShape>({
      memory: 0,
      sessions: 0,
    });
    const hub = yield* PubSub.unbounded<Invalidation>();

    const present =
      root === ""
        ? false
        : yield* fs.exists(root).pipe(Effect.orElseSucceed(() => false));

    if (present) {
      const flush = (batch: readonly Invalidation[]) =>
        Effect.gen(function* () {
          const distinct = dedupe(batch);
          const sawMemory = distinct.some((d) => d.scope === "memory");
          const sawSessions = distinct.some((d) => d.scope === "sessions");
          if (sawMemory) {
            yield* Ref.update(versionsRef, (v) => ({
              ...v,
              memory: v.memory + 1,
            }));
          }
          if (sawSessions) {
            yield* Ref.update(versionsRef, (v) => ({
              ...v,
              sessions: v.sessions + 1,
            }));
          }
          yield* Effect.forEach(distinct, (inv) => PubSub.publish(hub, inv), {
            discard: true,
          });
        }).pipe(
          Effect.withSpan("WatchService.flush", {
            attributes: { batch: batch.length },
          })
        );

      const pump = fs.watch(root, { recursive: true }).pipe(
        Stream.map((event) => classify({ path: event.path, root })),
        Stream.filter((inv): inv is Invalidation => inv !== null),
        Stream.groupedWithin(MAX_BATCH, DEBOUNCE),
        Stream.mapEffect((chunk) => flush([...chunk])),
        Stream.runDrain,
        // The watcher is a `forkScoped` daemon: it only ever ends when the scope
        // closes (fiber interruption), so we must not wrap it in a lifetime span
        // — that span would always resolve `fail` on interrupt. Swallow the
        // expected interruption; surface any genuine watch failure as a log.
        Effect.catchAllCause((cause) =>
          Cause.isInterruptedOnly(cause)
            ? Effect.void
            : Effect.logError("WatchService.pump failed", cause)
        )
      );
      yield* Effect.forkScoped(pump);
    }

    return {
      versions: Ref.get(versionsRef),
      changes: Stream.fromPubSub(hub),
    } satisfies WatchServiceShape;
  })
);
