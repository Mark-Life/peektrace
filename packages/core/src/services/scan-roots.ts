import { FileSystem } from "@effect/platform";
import { Effect } from "effect";
import { AGENT_IDS } from "./agent-id";
import { AgentRegistry, type AgentSource, sourcesOf } from "./agents";

/**
 * Every transcript folder the running server scans, per supported agent in
 * `AGENT_IDS` order, each flagged with whether it exists on disk right now.
 */
export const scannedRoots = Effect.gen(function* () {
  const registry = yield* AgentRegistry;
  const fs = yield* FileSystem.FileSystem;
  const exists = (path: string) =>
    fs.exists(path).pipe(Effect.orElseSucceed(() => false));
  const toRoot = (source: AgentSource) =>
    exists(source.projectsRoot).pipe(
      Effect.map((found) => ({
        isDefault: source.isDefault,
        home: source.home,
        path: source.projectsRoot,
        exists: found,
      }))
    );
  const supported = AGENT_IDS.map((id) => registry.roots(id)).filter(
    (r) => r.supported
  );
  return yield* Effect.forEach(supported, (roots) =>
    Effect.forEach(sourcesOf(roots), toRoot).pipe(
      Effect.map((scanned) => ({ agent: roots.id, roots: scanned }))
    )
  );
});
