/** Integration tests for the OpenCode SQLite/tree reader.
 *
 * Builds a throwaway data dir holding a real SQLite DB (matching the on-disk
 * `session`/`message`/`part` schema) plus a sibling legacy `storage/` tree, then
 * asserts the union/dedup rule (DB wins), tree-only inclusion, and the timestamp
 * trap: serialization must use `data.time.created`, never the migration-doctored
 * `time_created` column.
 */

import { Database } from "bun:sqlite";
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  listSessionRefs,
  loadSessionText,
  resolveDataDir,
} from "../../src/services/sessions/opencode/reader";
import { opencodeParser } from "../../src/services/sessions/parsers/opencode";

let dataDir: string;
const prevEnv = process.env.PEEKTRACE_OPENCODE_DATA;

/** The truthful message time, deliberately far BEFORE the doctored column. */
const REAL_CREATED = 1500;
const DOCTORED_COLUMN = 9_999_999_999;

const writeJson = (path: string, value: unknown) =>
  writeFileSync(path, JSON.stringify(value), "utf8");

/** The on-disk `session`/`message`/`part` DDL, shared by every test DB. */
const SESSION_DDL =
  "CREATE TABLE session (id TEXT PRIMARY KEY, project_id TEXT, directory TEXT, title TEXT, version TEXT, model TEXT, agent TEXT, cost REAL, tokens_input INTEGER, tokens_output INTEGER, tokens_reasoning INTEGER, tokens_cache_read INTEGER, tokens_cache_write INTEGER, time_created INTEGER, time_updated INTEGER)";
const MESSAGE_DDL =
  "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT)";
const PART_DDL =
  "CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT)";

/** Build a standalone opencode DB holding one healthy text-only session. */
const writeGoodDb = (path: string, id: string, directory: string) => {
  const db = new Database(path);
  db.exec(SESSION_DDL);
  db.exec(MESSAGE_DDL);
  db.exec(PART_DDL);
  db.query(
    "INSERT INTO session (id, directory, title, time_created, time_updated) VALUES (?, ?, ?, ?, ?)"
  ).run(id, directory, "Good session", 1000, 2000);
  db.query(
    "INSERT INTO message (id, session_id, time_created, data) VALUES (?, ?, ?, ?)"
  ).run(
    `${id}-m`,
    id,
    1500,
    JSON.stringify({ role: "user", time: { created: 1500 } })
  );
  db.query(
    "INSERT INTO part (id, message_id, session_id, time_created, data) VALUES (?, ?, ?, ?, ?)"
  ).run(
    `${id}-p`,
    `${id}-m`,
    id,
    1501,
    JSON.stringify({ type: "text", text: `hi from ${id}` })
  );
  db.close();
};

/** Run `fn` against a fresh temp data dir, cleaning it up afterward. */
const withTempDir = (fn: (dir: string) => void) => {
  const dir = mkdtempSync(join(tmpdir(), "peektrace-opencode-rb-"));
  try {
    fn(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
};

/** Parse a serialized dialect blob back into its `session`/`message`/`part` lines. */
const dialectLines = (text: string) =>
  text.split("\n").map((l) => JSON.parse(l) as Record<string, unknown>);

beforeAll(() => {
  dataDir = mkdtempSync(join(tmpdir(), "peektrace-opencode-"));
  process.env.PEEKTRACE_OPENCODE_DATA = dataDir;

  const db = new Database(join(dataDir, "opencode.db"));
  db.exec(SESSION_DDL);
  db.exec(MESSAGE_DDL);
  db.exec(PART_DDL);
  db.query(
    "INSERT INTO session (id, directory, title, model, agent, tokens_input, tokens_cache_read, time_created, time_updated) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
  ).run(
    "ses_db1",
    "/db/proj",
    "DB session",
    JSON.stringify({ id: "m1", providerID: "openai" }),
    "build",
    8305,
    8192,
    1000,
    2000
  );
  db.query(
    "INSERT INTO message (id, session_id, time_created, data) VALUES (?, ?, ?, ?)"
  ).run(
    "msg1",
    "ses_db1",
    DOCTORED_COLUMN,
    JSON.stringify({ role: "user", time: { created: REAL_CREATED } })
  );
  db.query(
    "INSERT INTO part (id, message_id, session_id, time_created, data) VALUES (?, ?, ?, ?, ?)"
  ).run(
    "prt1",
    "msg1",
    "ses_db1",
    1501,
    JSON.stringify({ type: "text", text: "hi from db" })
  );
  db.close();

  // Legacy tree: one tree-only session + a duplicate of the DB session.
  const sessionDir = join(dataDir, "storage", "session", "proj");
  mkdirSync(sessionDir, { recursive: true });
  writeJson(join(sessionDir, "ses_tree1.json"), {
    id: "ses_tree1",
    directory: "/tree/proj",
    title: "Tree session",
    time: { created: 100, updated: 200 },
  });
  writeJson(join(sessionDir, "ses_db1.json"), {
    id: "ses_db1",
    directory: "/tree/DUP",
    time: { created: 1, updated: 2 },
  });
  const treeMsgDir = join(dataDir, "storage", "message", "ses_tree1");
  mkdirSync(treeMsgDir, { recursive: true });
  writeJson(join(treeMsgDir, "msgT.json"), {
    role: "user",
    time: { created: 150 },
  });
  const treePartDir = join(dataDir, "storage", "part", "msgT");
  mkdirSync(treePartDir, { recursive: true });
  writeJson(join(treePartDir, "prtT.json"), {
    type: "text",
    text: "hi from tree",
  });
});

afterAll(() => {
  if (prevEnv === undefined) {
    delete process.env.PEEKTRACE_OPENCODE_DATA;
  } else {
    process.env.PEEKTRACE_OPENCODE_DATA = prevEnv;
  }
  rmSync(dataDir, { recursive: true, force: true });
});

describe("resolveDataDir", () => {
  test("honours PEEKTRACE_OPENCODE_DATA", () => {
    expect(resolveDataDir()).toBe(dataDir);
  });
});

describe("listSessionRefs (union + dedup)", () => {
  test("DB wins on an overlapping id; tree-only session is included", () => {
    const refs = listSessionRefs(dataDir);
    const byId = new Map(refs.map((r) => [r.id, r]));
    expect(refs.filter((r) => r.id === "ses_db1")).toHaveLength(1);
    // DB row (directory /db/proj) beats the tree duplicate (/tree/DUP).
    expect(byId.get("ses_db1")?.directory).toBe("/db/proj");
    expect(byId.get("ses_tree1")?.directory).toBe("/tree/proj");
  });
});

describe("loadSessionText (timestamp trap)", () => {
  test("serializes the DB session using data.time.created, not the column", () => {
    const { text, sizeBytes, mtimeMs } = loadSessionText(dataDir, "ses_db1");
    expect(sizeBytes).toBeGreaterThan(0);
    // mtimeMs is the session's time_updated column.
    expect(mtimeMs).toBe(2000);
    const lines = text.split("\n").map((l) => JSON.parse(l));
    const message = lines.find((l) => l.kind === "message");
    expect(message.time.created).toBe(REAL_CREATED);
    expect(message.time.created).not.toBe(DOCTORED_COLUMN);
    const part = lines.find((l) => l.kind === "part");
    expect(part.text).toBe("hi from db");
  });

  test("loads a tree-only session that no DB knows about", () => {
    const { text } = loadSessionText(dataDir, "ses_tree1");
    const lines = text.split("\n").map((l) => JSON.parse(l));
    const session = lines.find((l) => l.kind === "session");
    expect(session.id).toBe("ses_tree1");
    const part = lines.find((l) => l.kind === "part");
    expect(part.text).toBe("hi from tree");
  });
});

describe("reader robustness (never-throw / degrade)", () => {
  test("a corrupt sibling DB does not hide a healthy DB's sessions", () => {
    withTempDir((dir) => {
      writeGoodDb(join(dir, "opencode.db"), "ses_good", "/good/proj");
      writeFileSync(join(dir, "opencode-bad.db"), "not a database at all");
      expect(listSessionRefs(dir).map((r) => r.id)).toContain("ses_good");
      const { text, sizeBytes } = loadSessionText(dir, "ses_good");
      expect(sizeBytes).toBeGreaterThan(0);
      const session = dialectLines(text).find((l) => l.kind === "session");
      expect(session?.id).toBe("ses_good");
    });
  });

  test("a zero-byte DB is skipped, not fatal", () => {
    withTempDir((dir) => {
      writeGoodDb(join(dir, "opencode.db"), "ses_good", "/good/proj");
      writeFileSync(join(dir, "opencode-empty.db"), "");
      expect(listSessionRefs(dir).map((r) => r.id)).toContain("ses_good");
      const part = dialectLines(loadSessionText(dir, "ses_good").text).find(
        (l) => l.kind === "part"
      );
      expect(part?.text).toBe("hi from ses_good");
    });
  });

  test("loadSessionText for an unknown id returns empty text and zero size", () => {
    withTempDir((dir) => {
      writeGoodDb(join(dir, "opencode.db"), "ses_good", "/good/proj");
      const missing = loadSessionText(dir, "nonexistent-id");
      expect(missing.text).toBe("");
      expect(missing.sizeBytes).toBe(0);
      expect(missing.mtimeMs).toBe(0);
    });
  });

  test("a session living only in the second DB still resolves", () => {
    withTempDir((dir) => {
      writeGoodDb(join(dir, "opencode.db"), "ses_a", "/a");
      writeGoodDb(join(dir, "opencode-2.db"), "ses_b", "/b");
      const ids = listSessionRefs(dir).map((r) => r.id);
      expect(ids).toContain("ses_a");
      expect(ids).toContain("ses_b");
      const part = dialectLines(loadSessionText(dir, "ses_b").text).find(
        (l) => l.kind === "part"
      );
      expect(part?.text).toBe("hi from ses_b");
    });
  });
});

describe("round-trip (reader → parser)", () => {
  test("a serialized DB session parses back into the expected structure", () => {
    const { text } = loadSessionText(dataDir, "ses_db1");
    const parsed = opencodeParser.parseSession({
      text,
      path: "/tmp/rt#ses_db1",
      sessionId: "fallback-id",
      slug: "/db/proj",
    });
    expect(parsed.provider).toBe("opencode");
    expect(parsed.sessionId).toBe("ses_db1");
    expect(parsed.cwd).toBe("/db/proj");
    const prompt = parsed.events.find((e) => e.kind === "user-prompt");
    expect(prompt?.body).toBe("hi from db");
    // the message's truthful time.created flows through to the event.
    expect(prompt?.ts).toBe(new Date(REAL_CREATED).toISOString());
  });
});
