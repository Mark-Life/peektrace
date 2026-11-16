/**
 * OpenCode data dirs for the parity harness: SQLite stores (`opencode*.db`)
 * and a legacy `storage/` tree, built at run time with `bun:sqlite`. Never
 * write `opencode.DB`: on a case-insensitive disk it is `opencode.db`.
 */
import { Database } from "bun:sqlite";
import { cpSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { createOpencodeSchema } from "../../packages/core/test/fixtures/opencode-db";
import {
  costSessions,
  edgeSessions,
  message,
  open,
  orderSession,
  part,
  richSession,
  session,
  textSession,
} from "./parity-opencode-records";

const writeTree = (st: string) => {
  const tree = (rel: string, v: unknown) => {
    const p = join(st, rel);
    mkdirSync(dirname(p), { recursive: true });
    writeFileSync(p, typeof v === "string" ? v : JSON.stringify(v));
  };
  tree("session/projA/ses_tree1.json", {
    id: "ses_tree1",
    directory: "/tree/proj",
    title: "Tree session",
    version: "0.1",
    model: { id: "claude-sonnet-4-6" },
    cost: 0,
    time: { created: 100, updated: 200 },
  });
  tree("session/projA/ses_dup.json", {
    id: "ses_dup",
    directory: "/tree/DUP",
    time: { created: 1, updated: 2 },
  });
  tree("session/projA/ses_named.json", {
    id: "ses_other_name",
    directory: "/tree/named",
    time: { created: 1, updated: 3 },
  });
  tree("session/projA/bad.json", "{nope");
  tree("session/projB/ses_tree2.json", {
    id: "ses_tree2",
    cost: 0.2,
    time: { created: 300 },
  });
  tree("message/ses_tree1/msg_b.json", {
    role: "assistant",
    modelID: "claude-sonnet-4-6",
    id: "other",
    tokens: { input: 10, output: 5, cache: { read: 0, write: 0 } },
    time: { created: 160 },
  });
  tree("message/ses_tree1/msg_a.json", {
    role: "user",
    time: { created: 150 },
  });
  tree("message/ses_tree1/msg_c.json", "garbage");
  tree("message/ses_tree1/msg_d.json", { role: "user" });
  tree("part/msg_a/prt_2.json", { type: "text", text: "second part" });
  tree("part/msg_a/prt_1.json", { type: "text", text: "hi from tree" });
  tree("part/msg_b/prt_3.json", "{broken");
  tree("part/msg_b/prt_4.json", {
    type: "tool",
    tool: "bash",
    callID: "c1",
    state: { status: "completed", input: { command: "ls" }, output: "a\nb" },
  });
  tree("part/msg_d/prt_5.json", { type: "text", text: "no time user" });
};

/**
 * A WAL-mode store whose writer stays open: one committed session lives only
 * in the `-wal` file, and an open write transaction holds another. Call the
 * returned function to commit and close.
 */
const walStore = (path: string) => {
  const d = new Database(path);
  d.exec("PRAGMA journal_mode=WAL");
  d.exec("PRAGMA wal_autocheckpoint=0");
  createOpencodeSchema(d);
  session(d, {
    id: "ses_wal",
    directory: "/w/wal",
    title: "in the wal",
    time_created: 1,
    time_updated: 7200,
  });
  message(d, "ses_wal", "m1", 1, { role: "user", time: { created: 5 } });
  part(d, "ses_wal", "m1", "p1", { type: "text", text: "from the wal" });
  d.exec("BEGIN IMMEDIATE");
  session(d, { id: "ses_wal_pending", time_created: 1, time_updated: 7300 });
  return () => {
    d.exec("COMMIT");
    d.close();
  };
};

export interface OpencodeData {
  readonly close: () => void;
  /** A second data dir for `settings.json` `roots.opencode`. */
  readonly extra: string;
  /** A time past what a JS `Date` holds (a TS defect). */
  readonly far: string;
  /** Every edge case: several DBs, junk files, the tree, a live WAL store. */
  readonly main: string;
  /** OpenCode data beside `mixedClaude`, holding `ses_amb2`. */
  readonly mixed: string;
  /** A Claude projects dir: the golden fixture plus `ses_amb1.jsonl`. */
  readonly mixedClaude: string;
  /** `<xdg>/opencode`, found through `XDG_DATA_HOME`. */
  readonly xdg: string;
}

/** Build every OpenCode data dir under `root`. */
export const buildOpencodeData = (
  root: string,
  goldenProjects: string
): OpencodeData => {
  const main = join(root, "main");
  const a = open(join(main, "opencode.db"));
  richSession(a);
  costSessions(a);
  orderSession(a);
  textSession(a);
  edgeSessions(a);
  a.close();
  const b = open(join(main, "opencode-2.db"));
  session(b, {
    id: "ses_dup",
    directory: "/w/dup-db2",
    title: "from db2",
    time_created: 1,
    time_updated: 7000,
  });
  session(b, {
    id: "ses_b",
    directory: "/w/b",
    title: "only in db2",
    time_created: 1,
    time_updated: 7100,
  });
  b.close();
  writeFileSync(join(main, "opencode-bad.db"), "not a database at all");
  writeFileSync(join(main, "opencode-empty.db"), "");
  writeFileSync(join(main, "opencode.db-wal"), "");
  mkdirSync(join(main, "opencode-dir.db"));
  const c = open(join(main, "other.db"));
  session(c, { id: "ses_other", time_created: 1, time_updated: 1 });
  c.close();
  writeTree(join(main, "storage"));
  const closeWal = walStore(join(main, "opencode-wal.db"));

  const far = join(root, "far");
  const f = open(join(far, "opencode.db"));
  session(f, {
    id: "ses_far",
    directory: "/w/far",
    time_created: 1,
    time_updated: 9e15,
  });
  f.close();

  const extra = join(root, "extra");
  const e = open(join(extra, "opencode.db"));
  session(e, {
    id: "ses_rich",
    directory: "/w/extra",
    title: "same id, other root",
    time_created: 1,
    time_updated: 7400,
  });
  session(e, { id: "ses_ext", time_created: 1, time_updated: 7500 });
  e.close();

  const xdg = join(root, "xdg");
  const x = open(join(xdg, "opencode", "opencode.db"));
  session(x, {
    id: "ses_xdg",
    directory: "/w/xdg",
    title: "found through XDG_DATA_HOME",
    time_created: 1,
    time_updated: 7600,
  });
  x.close();

  const mixedClaude = join(root, "mixed-claude");
  cpSync(goldenProjects, mixedClaude, { recursive: true });
  const amb = join(mixedClaude, "-w-amb", "ses_amb1.jsonl");
  mkdirSync(dirname(amb), { recursive: true });
  writeFileSync(
    amb,
    `${JSON.stringify({ type: "user", timestamp: "2026-11-01T00:00:00.000Z", cwd: "/w/amb", message: { role: "user", content: "claude side" } })}\n`
  );
  const mixed = join(root, "mixed");
  const m = open(join(mixed, "opencode.db"));
  session(m, {
    id: "ses_amb2",
    directory: "/w/amb",
    title: "opencode side",
    time_created: 1,
    time_updated: 1_793_000_000_000,
  });
  message(m, "ses_amb2", "m1", 1, { role: "user", time: { created: 5 } });
  part(m, "ses_amb2", "m1", "p1", { type: "text", text: "opencode side" });
  m.close();

  return { main, far, extra, xdg, mixedClaude, mixed, close: closeWal };
};
