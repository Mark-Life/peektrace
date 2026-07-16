/** OpenCode storage reader — the server/bun-only IO seam for OpenCode sessions.
 *
 * OpenCode stores conversations in a SQLite DB (`opencode.db`, plus one file
 * per non-default channel) and, for users who never crossed the migration
 * window, a legacy content-addressed JSON tree under `storage/`. This module
 * reads BOTH, deduped by session id (DB wins), and serializes one session into
 * a self-defined JSONL "dialect" that the pure `opencodeParser` consumes just
 * like Codex/Pi consume their own text.
 *
 * IMPORTANT: this file imports `bun:sqlite` + `node:fs`/`os`/`path` and must
 * never reach the browser bundle. It is imported only by `agents.ts` (already
 * node-only) and is deliberately kept out of the package's browser exports.
 * Every function the registry calls is defensive: it returns `[]`/empty on any
 * failure rather than throwing.
 */
import { Database } from "bun:sqlite";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

/** A JSON object, untyped. */
type Json = Record<string, unknown>;

/** One hydrated message plus its ordered parts (ids already reattached). */
interface LoadedMessage {
  /** Hydrated message = `{...data, id, sessionID}`. */
  readonly message: Json;
  /** Hydrated parts = `{...data, id, sessionID, messageID}`, in id order. */
  readonly parts: readonly Json[];
}

/** A single OpenCode session resolved from the DB or the legacy tree. */
interface LoadedSession {
  readonly agent?: string;
  readonly cost: number;
  readonly directory?: string;
  readonly id: string;
  readonly messages: readonly LoadedMessage[];
  /** Parsed session model (`{id,providerID,...}`) or the raw JSON string. */
  readonly model?: unknown;
  readonly projectId?: string;
  readonly timeCreated: number;
  readonly timeUpdated: number;
  readonly title?: string;
  readonly tokensCacheRead: number;
  readonly tokensCacheWrite: number;
  readonly tokensInput: number;
  readonly tokensOutput: number;
  readonly tokensReasoning: number;
  readonly version?: string;
}

/** Lightweight session pointer for listing (no message bodies loaded). */
export interface SessionRef {
  readonly directory?: string;
  readonly id: string;
  readonly timeUpdated: number;
}

/** The serialized-session payload the registry hands to the parser. */
export interface SessionText {
  readonly mtimeMs: number;
  readonly sizeBytes: number;
  readonly text: string;
}

/** Spread helper that drops a key when its value is undefined. */
const opt = <K extends string, V>(
  key: K,
  value: V | undefined
): Partial<Record<K, V>> =>
  value === undefined ? {} : ({ [key]: value } as Record<K, V>);

/** Read a numeric column/field, defaulting to 0 when null/non-numeric. */
const num = (v: unknown): number => (typeof v === "number" ? v : 0);

/** Read a string column/field, or `undefined` when null/non-string. */
const optStr = (v: unknown): string | undefined =>
  typeof v === "string" && v.length > 0 ? v : undefined;

/** Parse a JSON string into an object, or `undefined` on any failure. */
const parseJson = (v: unknown): Json | undefined => {
  if (typeof v !== "string") {
    return;
  }
  try {
    const parsed = JSON.parse(v) as unknown;
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Json)
      : undefined;
  } catch {
    return;
  }
};

/**
 * Resolve the OpenCode data dir: `PEEKTRACE_OPENCODE_DATA` when set, else the
 * XDG default `~/.local/share/opencode`.
 */
export const resolveDataDir = (): string =>
  process.env.PEEKTRACE_OPENCODE_DATA ??
  join(homedir(), ".local", "share", "opencode");

/** A raw `session` table row (only the columns this module reads). */
interface SessionRow {
  readonly agent: unknown;
  readonly cost: unknown;
  readonly directory: unknown;
  readonly id: string;
  readonly model: unknown;
  readonly project_id: unknown;
  readonly time_created: unknown;
  readonly time_updated: unknown;
  readonly title: unknown;
  readonly tokens_cache_read: unknown;
  readonly tokens_cache_write: unknown;
  readonly tokens_input: unknown;
  readonly tokens_output: unknown;
  readonly tokens_reasoning: unknown;
  readonly version: unknown;
}

/** A raw `message` table row. */
interface MessageRow {
  readonly data: unknown;
  readonly id: string;
  readonly session_id: string;
  readonly time_created: unknown;
}

/** A raw `part` table row. */
interface PartRow {
  readonly data: unknown;
  readonly id: string;
  readonly message_id: string;
  readonly session_id: string;
}

/**
 * Open every `opencode*.db` in the data dir read-only. Excludes the `-wal` /
 * `-shm` sidecars (extension must be exactly `.db`). Never throws — a dir that
 * is absent or holds an unopenable file yields the databases that did open.
 */
const openDbsReadonly = (dataDir: string): Database[] => {
  let names: string[];
  try {
    names = readdirSync(dataDir);
  } catch {
    return [];
  }
  const dbs: Database[] = [];
  for (const name of names) {
    if (!(name.startsWith("opencode") && name.endsWith(".db"))) {
      continue;
    }
    try {
      dbs.push(new Database(join(dataDir, name), { readonly: true }));
    } catch {
      /* skip a locked / corrupt db, keep the rest */
    }
  }
  return dbs;
};

/** Close a batch of databases, swallowing any close error. */
const closeDbs = (dbs: readonly Database[]): void => {
  for (const db of dbs) {
    try {
      db.close();
    } catch {
      /* best-effort */
    }
  }
};

/** Map a `session` row into the `LoadedSession` header (no messages yet). */
const sessionFromRow = (row: SessionRow): Omit<LoadedSession, "messages"> => ({
  id: row.id,
  timeCreated: num(row.time_created),
  timeUpdated: num(row.time_updated),
  cost: num(row.cost),
  tokensInput: num(row.tokens_input),
  tokensOutput: num(row.tokens_output),
  tokensReasoning: num(row.tokens_reasoning),
  tokensCacheRead: num(row.tokens_cache_read),
  tokensCacheWrite: num(row.tokens_cache_write),
  ...opt("projectId", optStr(row.project_id)),
  ...opt("directory", optStr(row.directory)),
  ...opt("title", optStr(row.title)),
  ...opt("version", optStr(row.version)),
  ...opt("agent", optStr(row.agent)),
  ...opt("model", parseJson(row.model) ?? optStr(row.model)),
});

/**
 * Hydrate one message row into `{...data, id, sessionID}`, backfilling
 * `time.created` from the column ONLY when the JSON omits it. The column is a
 * migration-time fallback; `data.time.created` is the truthful timestamp.
 */
const hydrateMessage = (row: MessageRow): Json => {
  const data = parseJson(row.data) ?? {};
  const time =
    data.time && typeof data.time === "object" && !Array.isArray(data.time)
      ? (data.time as Json)
      : undefined;
  const hasCreated = time && typeof time.created === "number";
  const withTime = hasCreated
    ? data
    : { ...data, time: { ...(time ?? {}), created: num(row.time_created) } };
  return { ...withTime, id: row.id, sessionID: row.session_id };
};

/** Hydrate one part row into `{...data, id, sessionID, messageID}`. */
const hydratePart = (row: PartRow): Json => ({
  ...(parseJson(row.data) ?? {}),
  id: row.id,
  sessionID: row.session_id,
  messageID: row.message_id,
});

/** Load one session's messages + parts from an open DB, ordered per the rule. */
const loadMessagesFromDb = (
  db: Database,
  sessionId: string
): LoadedMessage[] => {
  const messageRows = db
    .query(
      "SELECT id, session_id, time_created, data FROM message WHERE session_id = ? ORDER BY COALESCE(json_extract(data, '$.time.created'), time_created), id"
    )
    .all(sessionId) as MessageRow[];
  const partRows = db
    .query(
      "SELECT id, message_id, session_id, data FROM part WHERE session_id = ? ORDER BY message_id, id"
    )
    .all(sessionId) as PartRow[];
  const partsByMessage = new Map<string, Json[]>();
  for (const row of partRows) {
    const list = partsByMessage.get(row.message_id) ?? [];
    list.push(hydratePart(row));
    partsByMessage.set(row.message_id, list);
  }
  return messageRows
    .filter((row) => typeof row.id === "string")
    .map((row) => ({
      message: hydrateMessage(row),
      parts: partsByMessage.get(row.id) ?? [],
    }));
};

/** Find + fully load one session from any DB, or `undefined` if not present. */
const loadSessionFromDbs = (
  dbs: readonly Database[],
  sessionId: string
): LoadedSession | undefined => {
  for (const db of dbs) {
    try {
      const row = db
        .query("SELECT * FROM session WHERE id = ?")
        .get(sessionId) as SessionRow | null;
      if (row) {
        return {
          ...sessionFromRow(row),
          messages: loadMessagesFromDb(db, sessionId),
        };
      }
    } catch {
      /* skip a locked / corrupt db, keep looking in the rest */
    }
  }
  return;
};

// ── Legacy JSON tree (best-effort; DEAD but still parses) ──────────────

const JSON_EXT = /\.json$/;
const PATH_SEP = /[\\/]/;

/** The stem of a `*.json` file path (basename minus the extension). */
const jsonStem = (path: string): string =>
  (path.split(PATH_SEP).pop() ?? "").replace(JSON_EXT, "");

/** Absolute paths of `*.json` files directly in `dir` (non-recursive). */
const jsonFilesIn = (dir: string): string[] => {
  try {
    return readdirSync(dir)
      .filter((n) => n.endsWith(".json"))
      .map((n) => join(dir, n));
  } catch {
    return [];
  }
};

/** Immediate subdirectory names of `dir` (empty on any failure). */
const subdirsOf = (dir: string): string[] => {
  try {
    return readdirSync(dir).filter((n) => {
      try {
        return statSync(join(dir, n)).isDirectory();
      } catch {
        return false;
      }
    });
  } catch {
    return [];
  }
};

/** Read + parse one JSON file into an object, or `undefined` on any failure. */
const readJsonFile = (path: string): Json | undefined => {
  try {
    return parseJson(readFileSync(path, "utf8"));
  } catch {
    return;
  }
};

/** Enumerate legacy-tree session pointers under storage/session/<proj>/<id>. */
const listTreeSessionInfos = (dataDir: string): Json[] => {
  const root = join(dataDir, "storage", "session");
  const infos: Json[] = [];
  for (const projectDir of subdirsOf(root)) {
    for (const file of jsonFilesIn(join(root, projectDir))) {
      const info = readJsonFile(file);
      if (info && typeof info.id === "string") {
        infos.push(info);
      }
    }
  }
  return infos;
};

/** The `time.updated` (or `time.created`) ms epoch on a tree SessionInfo. */
const treeTime = (info: Json, key: "updated" | "created"): number => {
  const time = info.time as Json | undefined;
  return num(time?.[key]);
};

/** Fully load one session from the legacy tree, or `undefined` if absent. */
const loadSessionFromTree = (
  dataDir: string,
  sessionId: string
): LoadedSession | undefined => {
  const root = join(dataDir, "storage", "session");
  let info: Json | undefined;
  for (const projectDir of subdirsOf(root)) {
    const file = join(root, projectDir, `${sessionId}.json`);
    if (!existsSync(file)) {
      continue;
    }
    const parsed = readJsonFile(file);
    if (parsed && parsed.id === sessionId) {
      info = parsed;
      break;
    }
  }
  if (!info) {
    return;
  }
  const messageDir = join(dataDir, "storage", "message", sessionId);
  const messages: LoadedMessage[] = [];
  for (const file of jsonFilesIn(messageDir).sort()) {
    const data = readJsonFile(file);
    if (!data) {
      continue;
    }
    const messageId = jsonStem(file);
    const partDir = join(dataDir, "storage", "part", messageId);
    const parts = jsonFilesIn(partDir)
      .sort()
      .map((pf) => {
        const pdata = readJsonFile(pf) ?? {};
        const partId = jsonStem(pf);
        return {
          ...pdata,
          id: partId,
          sessionID: sessionId,
          messageID: messageId,
        };
      });
    messages.push({
      message: { ...data, id: messageId, sessionID: sessionId },
      parts,
    });
  }
  return {
    id: sessionId,
    timeCreated: treeTime(info, "created"),
    timeUpdated: treeTime(info, "updated") || treeTime(info, "created"),
    cost: num(info.cost),
    tokensInput: 0,
    tokensOutput: 0,
    tokensReasoning: 0,
    tokensCacheRead: 0,
    tokensCacheWrite: 0,
    messages,
    ...opt("directory", optStr(info.directory)),
    ...opt("title", optStr(info.title)),
    ...opt("version", optStr(info.version)),
    ...opt("model", info.model),
  };
};

/**
 * Serialize one loaded session into the JSONL dialect: a `session` line, then
 * each message line immediately followed by its part lines. This is the exact
 * text the pure `opencodeParser` consumes.
 */
export const serializeSessionToDialect = (session: LoadedSession): string => {
  const lines: string[] = [
    JSON.stringify({
      kind: "session",
      id: session.id,
      timeCreated: session.timeCreated,
      timeUpdated: session.timeUpdated,
      cost: session.cost,
      tokensInput: session.tokensInput,
      tokensOutput: session.tokensOutput,
      tokensReasoning: session.tokensReasoning,
      tokensCacheRead: session.tokensCacheRead,
      tokensCacheWrite: session.tokensCacheWrite,
      ...opt("projectId", session.projectId),
      ...opt("directory", session.directory),
      ...opt("title", session.title),
      ...opt("version", session.version),
      ...opt("model", session.model),
      ...opt("agent", session.agent),
    }),
  ];
  for (const m of session.messages) {
    lines.push(JSON.stringify({ ...m.message, kind: "message" }));
    for (const part of m.parts) {
      lines.push(JSON.stringify({ ...part, kind: "part" }));
    }
  }
  return lines.join("\n");
};

/**
 * List every OpenCode session across all DBs and the legacy tree, deduped by
 * id (DB wins; a tree session id already in a DB is skipped). Never throws.
 */
export const listSessionRefs = (dataDir: string): SessionRef[] => {
  const dbs = openDbsReadonly(dataDir);
  const byId = new Map<string, SessionRef>();
  try {
    for (const db of dbs) {
      try {
        const rows = db
          .query("SELECT id, directory, time_updated FROM session")
          .all() as Pick<SessionRow, "id" | "directory" | "time_updated">[];
        for (const row of rows) {
          if (typeof row.id === "string" && !byId.has(row.id)) {
            byId.set(row.id, {
              id: row.id,
              timeUpdated: num(row.time_updated),
              ...opt("directory", optStr(row.directory)),
            });
          }
        }
      } catch {
        /* skip a locked / corrupt db, keep enumerating the rest */
      }
    }
  } finally {
    closeDbs(dbs);
  }
  for (const info of listTreeSessionInfos(dataDir)) {
    const id = info.id as string;
    if (!byId.has(id)) {
      byId.set(id, {
        id,
        timeUpdated: treeTime(info, "updated") || treeTime(info, "created"),
        ...opt("directory", optStr(info.directory)),
      });
    }
  }
  return [...byId.values()];
};

/**
 * Load + serialize one session (DB preferred, tree as a gap-filler) into the
 * dialect. `mtimeMs` is the session's truthful updated time. Returns empty text
 * with zero size when the id resolves to nothing. Never throws.
 */
export const loadSessionText = (
  dataDir: string,
  sessionId: string
): SessionText => {
  let session: LoadedSession | undefined;
  const dbs = openDbsReadonly(dataDir);
  try {
    session = loadSessionFromDbs(dbs, sessionId);
  } catch {
    session = undefined;
  } finally {
    closeDbs(dbs);
  }
  session ??= loadSessionFromTree(dataDir, sessionId);
  if (!session) {
    return { text: "", sizeBytes: 0, mtimeMs: 0 };
  }
  const text = serializeSessionToDialect(session);
  return {
    text,
    sizeBytes: Buffer.byteLength(text, "utf8"),
    mtimeMs: session.timeUpdated,
  };
};

/** True when the OpenCode data dir exists (DB or tree). Cheap existence probe. */
export const dataDirExists = (dataDir: string): boolean => {
  try {
    return existsSync(dataDir);
  } catch {
    return false;
  }
};
