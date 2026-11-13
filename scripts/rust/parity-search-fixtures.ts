/** Generated corpora for the `sessions search` parity stage: the
 * `search.test.ts` sessions under a fake HOME, and lines that stress the
 * matcher and the snippet builder. */
import { chmodSync, mkdirSync, utimesSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { writeGoodDb } from "../../packages/core/test/fixtures/opencode-db";
import { FAKE_SECRETS } from "./parity-fake-secrets";
import { WORK } from "./parity-lib";

const FIXED_MTIME = new Date("2026-11-11T16:42:21.851Z");
const NO_ACCESS = 0o000;
const FILLER = 30;
const DEPTH_IN = 64;
const DEPTH_OUT = 65;
const FAKE_KEY = FAKE_SECRETS.anthropicFixture;
const JWT = FAKE_SECRETS.jwtSigned;
/** Files made unreadable; the stage restores them. */
export const locked: string[] = [];

const put = (path: string, body: string | Uint8Array) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, body);
  utimesSync(path, FIXED_MTIME, FIXED_MTIME);
};
const jsonl = (rows: readonly unknown[]) =>
  `${rows.map((r) => JSON.stringify(r)).join("\n")}\n`;
const uuid = (c: string) =>
  `${c.repeat(8)}-${c.repeat(4)}-4${c.repeat(3)}-8${c.repeat(3)}-${c.repeat(12)}`;
const claudeLine = (cwd: string, at: string, content: unknown) => ({
  type: "user",
  cwd,
  gitBranch: "feature/zebra-branch",
  version: "2.0.0",
  timestamp: at,
  sessionId: "s",
  uuid: `u-${at}`,
  message: { role: "user", content },
});

/** The `search.test.ts` corpus, plus home paths and a locked transcript,
 * under a HOME the home collapser knows. */
export const buildSynthetic = () => {
  const home = join(WORK, "search-home");
  const proj = join(home, ".claude", "projects");
  const cwd = join(home, "work");
  const slug = cwd.replace(/[^A-Za-z0-9]/g, "-");
  const at = (d: string) => `2026-${d}T10:00:00.000Z`;
  put(
    join(proj, slug, `${uuid("a")}.jsonl`),
    jsonl([
      claudeLine(cwd, at("05-03"), "Fix the Quokka parser"),
      claudeLine(cwd, at("05-03"), [
        { type: "text", text: `Looking at ${cwd}/src/app.ts quokka` },
      ]),
      claudeLine(cwd, at("05-03"), `key ${FAKE_KEY} quokka`),
      claudeLine(cwd, at("05-03"), 'quokka again\nsecond "quote" line a"b'),
      claudeLine(cwd, at("05-03"), `another quokka in ${slug}-x`),
      { type: "summary", summary: "Fix the Quokka parser", leafUuid: "x" },
      { type: "ai-title", aiTitle: `Fix parser in ${home}/tools/app` },
    ])
  );
  put(
    join(proj, slug, `${uuid("b")}.jsonl`),
    jsonl([
      claudeLine(cwd, at("04-03"), "quokka in the middle, an old walrus"),
      claudeLine(cwd, at("04-03"), "then git push --force -rf"),
    ])
  );
  put(
    join(proj, "-home-demo-other", `${uuid("c")}.jsonl`),
    jsonl([claudeLine("/home/demo/other", at("01-03"), "quokka long ago")])
  );
  const codex = uuid("d");
  put(
    join(
      home,
      ".codex",
      "sessions",
      "2026",
      "05",
      "01",
      `rollout-2026-05-01T10-00-00-${codex}.jsonl`
    ),
    jsonl([
      {
        timestamp: at("05-01"),
        type: "session_meta",
        payload: { id: codex, timestamp: at("05-01"), cwd: "/home/demo/cx" },
      },
      {
        timestamp: at("05-01"),
        type: "response_item",
        payload: {
          type: "message",
          role: "user",
          content: [
            {
              type: "input_text",
              text: "find the wombat and the config loader",
            },
          ],
        },
      },
      {
        timestamp: at("05-01"),
        type: "event_msg",
        payload: {
          type: "user_message",
          message: "find the wombat and the config loader",
        },
      },
    ])
  );
  const pi = uuid("e");
  put(
    join(
      home,
      ".pi",
      "agent",
      "sessions",
      "--home-demo-pi--",
      `2026-02-01T10-00-00-000Z_${pi}.jsonl`
    ),
    jsonl([
      {
        type: "session",
        version: 3,
        id: pi,
        timestamp: at("02-01"),
        cwd: "/home/demo/pi",
      },
      {
        type: "message",
        id: "a1",
        parentId: null,
        timestamp: at("02-01"),
        message: {
          role: "user",
          content: [{ type: "text", text: "a pi quokka, THE config" }],
        },
      },
    ])
  );
  for (let i = 0; i < FILLER; i++) {
    const n = String(i).padStart(2, "0");
    put(
      join(proj, "-home-demo-filler", `${uuid("f")}-${n}.jsonl`),
      jsonl([
        claudeLine(
          "/home/demo/filler",
          `2025-01-0${i === 0 ? 1 : 2}T10:00:00.000Z`,
          `filler ${n}: the walrus`
        ),
      ])
    );
  }
  const lockedFile = join(proj, "-home-demo-locked", `${uuid("9")}.jsonl`);
  put(
    lockedFile,
    jsonl([claudeLine("/home/demo/locked", at("05-09"), "quokka")])
  );
  chmodSync(lockedFile, NO_ACCESS);
  locked.push(lockedFile);
  const oc = join(home, ".local", "share", "opencode");
  mkdirSync(oc, { recursive: true });
  writeGoodDb(join(oc, "opencode.db"), "ses_search1", "/home/demo/oc");
  return home;
};

const nest = (n: number, inner: unknown): unknown =>
  n === 0 ? inner : [nest(n - 1, inner)];

/** Lines that stress the matcher and the snippet builder. */
const edgeLines = (home: string) => [
  {
    type: "user",
    message: {
      content:
        "ok \u001b[31mERROR walrus\u001b[0m \u001b]0;pwned\u0007 done\u0000",
    },
  },
  { type: "user", message: { content: "evil ‮ txt.exe ⁦ walrus ‏" } },
  { b: "walrus b", 1: "walrus one" },
  "walrus top-level string",
  { type: "session_meta", payload: { text: "walrus meta" } },
  { type: "turn_context", payload: { text: "walrus turn" } },
  { t: "page one\fwalrus two\u000bend" },
  { t: "café walrus" },
  nest(DEPTH_IN, "walrus deep in"),
  nest(DEPTH_OUT, "walrus too deep"),
  { t: `${"x".repeat(300)}walrus${"y".repeat(300)}` },
  { t: `${"😀".repeat(81)}walrus${"😀".repeat(81)}` },
  { t: `a${"😀".repeat(80)}walrus${"😀".repeat(80)}b` },
  { t: `key ${FAKE_KEY} walrus` },
  { t: `jwt ${JWT} walrus` },
  {
    t: `see ${home}/proj/src/app.ts and ${home.replace(/[^A-Za-z0-9]/g, "-")}-proj walrus`,
  },
  { t: "see -home-jane-doe-work walrus" },
  { t: 12_345, walrus: "key only" },
  { t: "a\u0001b walrus" },
  { id: "walrus-id", uuid: "walrus", cwd: "walrus", t: "the end" },
  { t: "привет мир walrus ПРИВЕТ МИР" },
  { t: "a\"b walrus 'single'" },
  { t: "an old\n\twalrus" },
  { t: "\u212A-sign \u017F-long \u1E9E-sharp οδος walrus" },
];

/** Generated edge transcripts: one big edge file, CRLF, BOM, invalid UTF-8,
 * an empty and a locked file. */
export const buildEdge = () => {
  const home = join(WORK, "search-edge-home");
  const proj = join(home, ".claude", "projects", "-edge-s");
  const rows = edgeLines(home).map((r) => JSON.stringify(r));
  const raw = `${rows.join("\n")}\n{"t":"\\ud800 walrus"}\n{"t":"lone \\udc00\\ud83d walrus"}\nnot json walrus\n\n`;
  put(join(proj, "edge.jsonl"), raw);
  // One session per line, so every snippet is a shown hit.
  for (const [i, line] of raw.split("\n").entries()) {
    put(join(proj, `line-${String(i).padStart(2, "0")}.jsonl`), line);
  }
  put(join(proj, "crlf.jsonl"), rows.slice(0, 4).join("\r\n"));
  put(join(proj, "bom.jsonl"), `﻿${rows[2]}\n`);
  put(
    join(proj, "bad.jsonl"),
    new Uint8Array([
      ...Buffer.from('{"t":"bad '),
      0xff,
      0xfe,
      ...Buffer.from(' walrus"}\n'),
    ])
  );
  put(join(proj, "empty.jsonl"), "");
  const lockedFile = join(proj, "locked.jsonl");
  put(lockedFile, '{"t":"walrus"}\n');
  chmodSync(lockedFile, NO_ACCESS);
  locked.push(lockedFile);
  return home;
};
