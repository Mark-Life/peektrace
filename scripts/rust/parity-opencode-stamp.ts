/**
 * Change stamps for the real OpenCode data dir, so the parity harness can skip
 * what OpenCode wrote while it ran. `-shm` files change on every read and are
 * never stamped.
 */
import { Database } from "bun:sqlite";
import { readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const JSON_EXT = /\.json$/;
const isDb = (name: string) =>
  name.startsWith("opencode") && name.endsWith(".db");

const listDir = (dir: string) => {
  try {
    return readdirSync(dir).sort();
  } catch {
    return [];
  }
};

/** Size and mtime of every file under `paths`. */
const fileStamp = (paths: readonly string[]) => {
  const parts: string[] = [];
  const walk = (p: string) => {
    try {
      const s = statSync(p);
      if (s.isDirectory()) {
        for (const name of listDir(p)) {
          walk(join(p, name));
        }
      } else {
        parts.push(`${p}:${s.size}:${s.mtimeMs}`);
      }
    } catch {
      parts.push(`${p}:missing`);
    }
  };
  for (const p of paths) {
    walk(p);
  }
  return parts.join("|");
};

/** The DB and WAL files and the whole `storage/` tree. */
export const storeStamp = (dir: string) =>
  fileStamp([
    ...listDir(dir)
      .filter((n) => n.startsWith("opencode") && !n.endsWith("-shm"))
      .map((n) => join(dir, n)),
    join(dir, "storage"),
  ]);

const SESSION_SQL = [
  "SELECT time_updated FROM session WHERE id = ?1",
  "SELECT count(*), sum(time_updated) FROM message WHERE session_id = ?1",
  "SELECT count(*), sum(time_updated) FROM part WHERE session_id = ?1",
];

/**
 * Stamps one session at a time: its rows in each DB and its files in the
 * legacy tree. A write to another session leaves the stamp as it was.
 */
export const openSessionStamper = (dir: string) => {
  const dbs = listDir(dir)
    .filter(isDb)
    .flatMap((name) => {
      try {
        return [new Database(join(dir, name), { readonly: true })];
      } catch {
        return [];
      }
    });
  const dbStamp = (id: string) =>
    dbs
      .flatMap((db) =>
        SESSION_SQL.map((sql) => {
          try {
            return JSON.stringify(db.query(sql).values(id));
          } catch {
            return "error";
          }
        })
      )
      .join("|");
  const tree = join(dir, "storage");
  const treeStamp = (id: string) => {
    const messageDir = join(tree, "message", id);
    return fileStamp([
      ...listDir(join(tree, "session")).map((p) =>
        join(tree, "session", p, `${id}.json`)
      ),
      messageDir,
      ...listDir(messageDir).map((m) =>
        join(tree, "part", m.replace(JSON_EXT, ""))
      ),
    ]);
  };
  const ids = () => {
    const fromDb = dbs.flatMap((db) => {
      try {
        return db
          .query("SELECT id FROM session")
          .values()
          .map(([id]) => String(id));
      } catch {
        return [];
      }
    });
    const fromTree = listDir(join(tree, "session")).flatMap((p) =>
      listDir(join(tree, "session", p))
        .filter((n) => JSON_EXT.test(n))
        .map((n) => n.replace(JSON_EXT, ""))
    );
    return [...new Set([...fromDb, ...fromTree])];
  };
  return {
    /** Every session id the store holds, loadable or not. */
    ids,
    stamp: (id: string) => `${dbStamp(id)}#${treeStamp(id)}`,
    close: () => {
      for (const db of dbs) {
        db.close();
      }
    },
  };
};
