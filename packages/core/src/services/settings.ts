/** Optional user settings for peektrace: extra agent roots to scan in parallel.
 *
 * Lives at `~/.peektrace/settings.json` (or `$PEEKTRACE_DIR/settings.json`) and
 * lets a user point peektrace at additional config dirs per agent — e.g. a
 * separate work Claude account alongside the personal `~/.claude` — so both show
 * up in one merged session list, each row labeled by its source.
 *
 * ```json
 * { "roots": { "claude": [{ "path": "~/work/.claude", "label": "work" }] } }
 * ```
 *
 * `path` is the agent's config HOME dir (the same thing `CLAUDE_CONFIG_DIR` /
 * `CODEX_HOME` / `XDG_DATA_HOME` point at); the transcript root is derived from
 * it per layout. `label` is optional and defaults to the dir's basename.
 *
 * This module is pure + browser-safe: the schema/parse functions touch no Node
 * builtins, and the one file-reading helper takes an Effect `FileSystem` so IO
 * only happens server-side inside the `AgentRegistry` live layer.
 */
import type { FileSystem } from "@effect/platform";
import { Effect, Schema } from "effect";
import { AGENT_IDS, AgentId } from "./agent-id";

/** One extra on-disk root a user declares for an agent. */
export const RootEntry = Schema.Struct({
  /** The agent's config HOME dir (transcript root derived per layout). */
  path: Schema.String,
  /** Human label for the source badge/facet; defaults to the dir basename. */
  label: Schema.optional(Schema.String),
});
export type RootEntry = typeof RootEntry.Type;

/** The parsed `settings.json`. Every field optional so a partial file is valid. */
export const PeektraceSettings = Schema.Struct({
  /** Extra roots per agent, unioned with each agent's env/default root. A
   * `partial` record so declaring only some agents (or none) is valid. */
  roots: Schema.optional(
    Schema.partial(
      Schema.Record({ key: AgentId, value: Schema.Array(RootEntry) })
    )
  ),
});
export type PeektraceSettings = typeof PeektraceSettings.Type;

/** The empty settings used whenever no file exists or parsing fails. */
export const EMPTY_SETTINGS: PeektraceSettings = {};

const decode = Schema.decodeUnknownEither(PeektraceSettings);
const decodeEntry = Schema.decodeUnknownEither(RootEntry);

const isAgentId = (key: string): key is (typeof AGENT_IDS)[number] =>
  (AGENT_IDS as readonly string[]).includes(key);

/**
 * Parse raw `settings.json` text into a `PeektraceSettings`, best-effort and
 * TOLERANT: a whole-file decode is tried first, and on failure each agent's
 * entries are salvaged individually so one malformed entry never discards the
 * valid roots beside it. Any JSON error yields the empty settings. Pure.
 */
export const parseSettings = (raw: string): PeektraceSettings => {
  const trimmed = raw.trim();
  if (trimmed === "") {
    return EMPTY_SETTINGS;
  }
  let json: unknown;
  try {
    json = JSON.parse(trimmed);
  } catch {
    return EMPTY_SETTINGS;
  }
  const whole = decode(json);
  if (whole._tag === "Right") {
    return whole.right;
  }
  // Salvage: keep the entries that individually validate, per known agent.
  const rootsRaw =
    typeof json === "object" && json !== null
      ? (json as { roots?: unknown }).roots
      : undefined;
  if (typeof rootsRaw !== "object" || rootsRaw === null) {
    return EMPTY_SETTINGS;
  }
  const roots: Partial<Record<(typeof AGENT_IDS)[number], RootEntry[]>> = {};
  for (const [key, value] of Object.entries(rootsRaw)) {
    if (!(isAgentId(key) && Array.isArray(value))) {
      continue;
    }
    const entries = value
      .map((e) => decodeEntry(e))
      .filter((d) => d._tag === "Right")
      .map((d) => d.right);
    if (entries.length > 0) {
      roots[key] = entries;
    }
  }
  return Object.keys(roots).length > 0 ? { roots } : EMPTY_SETTINGS;
};

/**
 * Read + parse the settings file at `path` via the platform FileSystem. Never
 * fails: a missing/unreadable file resolves to the empty settings. A file that
 * is present + non-empty but parses to nothing is logged (so a typo'd file that
 * silently disables the feature is at least visible in `--otel`/debug logs).
 */
export const loadSettings = (
  fs: FileSystem.FileSystem,
  path: string
): Effect.Effect<PeektraceSettings> =>
  fs.readFileString(path).pipe(
    Effect.flatMap((raw) => {
      const parsed = parseSettings(raw);
      return raw.trim() !== "" && parsed.roots === undefined
        ? Effect.logWarning(
            `peektrace: ignoring unparseable settings at ${path}`
          ).pipe(Effect.as(parsed))
        : Effect.succeed(parsed);
    }),
    Effect.orElseSucceed(() => EMPTY_SETTINGS)
  );
