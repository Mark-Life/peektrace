/** Shared builders for throwaway OpenCode SQLite DBs in tests. */
import { Database } from "bun:sqlite";

/** The on-disk `session`/`message`/`part` DDL. */
const SESSION_DDL =
  "CREATE TABLE session (id TEXT PRIMARY KEY, project_id TEXT, directory TEXT, title TEXT, version TEXT, model TEXT, agent TEXT, cost REAL, tokens_input INTEGER, tokens_output INTEGER, tokens_reasoning INTEGER, tokens_cache_read INTEGER, tokens_cache_write INTEGER, time_created INTEGER, time_updated INTEGER)";
const MESSAGE_DDL =
  "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT)";
const PART_DDL =
  "CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT)";

/** Fixed timestamps for the healthy session, in ms. */
const CREATED = 1000;
const UPDATED = 2000;
const MESSAGE_AT = 1500;
const PART_AT = 1501;

/** Create the `session`/`message`/`part` tables on an open DB. */
export const createOpencodeSchema = (db: Database) => {
  db.exec(SESSION_DDL);
  db.exec(MESSAGE_DDL);
  db.exec(PART_DDL);
};

/** Build a standalone opencode DB holding one healthy text-only session. */
export const writeGoodDb = (path: string, id: string, directory: string) => {
  const db = new Database(path);
  createOpencodeSchema(db);
  db.query(
    "INSERT INTO session (id, directory, title, time_created, time_updated) VALUES (?, ?, ?, ?, ?)"
  ).run(id, directory, "Good session", CREATED, UPDATED);
  db.query(
    "INSERT INTO message (id, session_id, time_created, data) VALUES (?, ?, ?, ?)"
  ).run(
    `${id}-m`,
    id,
    MESSAGE_AT,
    JSON.stringify({ role: "user", time: { created: MESSAGE_AT } })
  );
  db.query(
    "INSERT INTO part (id, message_id, session_id, time_created, data) VALUES (?, ?, ?, ?, ?)"
  ).run(
    `${id}-p`,
    `${id}-m`,
    id,
    PART_AT,
    JSON.stringify({ type: "text", text: `hi from ${id}` })
  );
  db.close();
};
