/** `SessionsService.search` over a throwaway corpus of synthetic sessions from
 * every agent, read through the real registry. */
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { BunFileSystem } from "@effect/platform-bun";
import { Effect, Layer } from "effect";
import { AgentRegistryLive } from "../../src/services/agents";
import { redactDisplay } from "../../src/services/sessions/search-match";
import type { SearchRequest } from "../../src/services/sessions/search-schema";
import {
  SessionsService,
  SessionsServiceLive,
} from "../../src/services/sessions/service";
import { writeGoodDb } from "../fixtures/opencode-db";

const CWD = "/home/demo/work";
const RAW_TITLE = "Fix parser in /home/demo/tools/app";
const BRANCH = "feature/zebra-branch";
const FAKE_KEY = "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234";

const id = (c: string) =>
  `${c.repeat(8)}-${c.repeat(4)}-4${c.repeat(3)}-8${c.repeat(3)}-${c.repeat(12)}`;
const CLAUDE_NEW = id("a");
const CLAUDE_MID = id("b");
const CLAUDE_OLD = id("c");
const CODEX = id("d");
const PI = id("e");
const FILLER = 30;
const OC = "ses_search1";

const claudeLine = (type: string, at: string, content: unknown) => ({
  type,
  cwd: CWD,
  gitBranch: BRANCH,
  version: "2.0.0",
  timestamp: at,
  sessionId: "s",
  uuid: `u-${at}`,
  message: { role: type, content },
});

let base = "";
const keys = [
  "PEEKTRACE_DIR",
  "PEEKTRACE_CLAUDE_PROJECTS",
  "PEEKTRACE_CODEX_SESSIONS",
  "PEEKTRACE_PI_SESSIONS",
  "PEEKTRACE_OPENCODE_DATA",
] as const;
const prev = Object.fromEntries(keys.map((k) => [k, process.env[k]]));

const put = (path: string, lines: readonly unknown[] | string) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(
    path,
    typeof lines === "string"
      ? lines
      : `${lines.map((l) => JSON.stringify(l)).join("\n")}\n`
  );
};

beforeAll(() => {
  base = mkdtempSync(join(tmpdir(), "peektrace-search-"));
  const proj = join(base, "claude");
  put(join(proj, "-home-demo-work", `${CLAUDE_NEW}.jsonl`), [
    claudeLine("user", "2026-05-03T10:00:00.000Z", "Fix the Quokka parser"),
    claudeLine("assistant", "2026-05-03T10:00:01.000Z", [
      { type: "text", text: "Looking at the quokka parser now" },
    ]),
    claudeLine("user", "2026-05-03T10:00:02.000Z", `key ${FAKE_KEY} quokka`),
    claudeLine(
      "user",
      "2026-05-03T10:00:03.000Z",
      'quokka again\nsecond "quote" line'
    ),
    claudeLine("user", "2026-05-03T10:00:04.000Z", "another quokka"),
    { type: "summary", summary: "Fix the Quokka parser", leafUuid: "x" },
    { type: "ai-title", aiTitle: RAW_TITLE },
  ]);
  put(join(proj, "-home-demo-work", `${CLAUDE_MID}.jsonl`), [
    claudeLine("user", "2026-04-03T10:00:00.000Z", "quokka in the middle"),
  ]);
  put(join(proj, "-home-demo-other", `${CLAUDE_OLD}.jsonl`), [
    claudeLine("user", "2026-01-03T10:00:00.000Z", "quokka long ago"),
  ]);
  put(
    join(
      base,
      "codex",
      "2026",
      "05",
      "01",
      `rollout-2026-05-01T10-00-00-${CODEX}.jsonl`
    ),
    [
      {
        timestamp: "2026-05-01T10:00:00.000Z",
        type: "session_meta",
        payload: {
          id: CODEX,
          timestamp: "2026-05-01T10:00:00.000Z",
          cwd: "/home/demo/cx",
        },
      },
      {
        timestamp: "2026-05-01T10:00:01.000Z",
        type: "response_item",
        payload: {
          type: "message",
          role: "user",
          content: [{ type: "input_text", text: "find the wombat" }],
        },
      },
      {
        timestamp: "2026-05-01T10:00:01.000Z",
        type: "event_msg",
        payload: { type: "user_message", message: "find the wombat" },
      },
    ]
  );
  put(
    join(
      base,
      "pi",
      "--home-demo-pi--",
      `2026-02-01T10-00-00-000Z_${PI}.jsonl`
    ),
    [
      {
        type: "session",
        version: 3,
        id: PI,
        timestamp: "2026-02-01T10:00:00.000Z",
        cwd: "/home/demo/pi",
      },
      {
        type: "message",
        id: "a1",
        parentId: null,
        timestamp: "2026-02-01T10:00:01.000Z",
        message: {
          role: "user",
          content: [{ type: "text", text: "a pi quokka" }],
        },
      },
    ]
  );
  for (let i = 0; i < FILLER; i++) {
    const n = String(i).padStart(2, "0");
    put(join(proj, "-home-demo-filler", `${id("f")}-${n}.jsonl`), [
      claudeLine(
        "user",
        `2025-01-${n === "00" ? "01" : "02"}T10:00:00.000Z`,
        `filler ${n}`
      ),
    ]);
  }
  const oc = join(base, "opencode");
  mkdirSync(oc, { recursive: true });
  writeGoodDb(join(oc, "opencode.db"), OC, "/home/demo/oc");
  mkdirSync(join(base, "pt"), { recursive: true });
  process.env.PEEKTRACE_DIR = join(base, "pt");
  process.env.PEEKTRACE_CLAUDE_PROJECTS = proj;
  process.env.PEEKTRACE_CODEX_SESSIONS = join(base, "codex");
  process.env.PEEKTRACE_PI_SESSIONS = join(base, "pi");
  process.env.PEEKTRACE_OPENCODE_DATA = oc;
});

afterAll(() => {
  for (const k of keys) {
    if (prev[k] === undefined) {
      delete process.env[k];
    } else {
      process.env[k] = prev[k];
    }
  }
  rmSync(base, { recursive: true, force: true });
});

const layer = SessionsServiceLive.pipe(
  Layer.provide(AgentRegistryLive),
  Layer.provide(BunFileSystem.layer)
);

const search = (req: SearchRequest) =>
  Effect.runPromise(
    Effect.flatMap(SessionsService, (s) => s.search(req)).pipe(
      Effect.provide(layer)
    )
  );

const ids = (r: Awaited<ReturnType<typeof search>>) =>
  r.sessions.map((s) => s.id);

describe("sessions search", () => {
  test("returns matching sessions newest first, across agents", async () => {
    const r = await search({ q: "quokka" });
    expect(ids(r)).toEqual([CLAUDE_NEW, CLAUDE_MID, PI, CLAUDE_OLD]);
    expect(r.truncated).toBe(false);
    expect(r.scannedSessions).toBe(6 + FILLER);
  });

  test("is case-insensitive and reports the line and match range", async () => {
    const r = await search({ q: "QUOKKA PARSER" });
    const [s] = r.sessions;
    expect(s?.id).toBe(CLAUDE_NEW);
    const hit = s?.hits[0];
    expect(hit?.line).toBe(1);
    expect(hit?.snippet).toBe("Fix the Quokka parser");
    expect(hit?.snippet.slice(hit.matchStart, hit.matchEnd)).toBe(
      "Quokka parser"
    );
  });

  test("caps hits per session but counts every matching line", async () => {
    const r = await search({ q: "quokka" });
    const s = r.sessions[0];
    expect(s?.hits.length).toBe(3);
    expect(s?.matchCount).toBe(6);
  });

  test("a query equal to metadata values matches nothing", async () => {
    for (const q of [CWD, BRANCH, "2.0.0", "user", "2026-05-03T10"]) {
      expect((await search({ q })).sessions).toEqual([]);
    }
  });

  test("a query with a double quote matches the escaped JSON text", async () => {
    const r = await search({ q: 'second "quote"' });
    expect(ids(r)).toEqual([CLAUDE_NEW]);
    expect(r.sessions[0]?.hits[0]?.snippet).toContain('second "quote"');
  });

  test("Codex text written twice yields one hit", async () => {
    const r = await search({ q: "wombat" });
    expect(ids(r)).toEqual([CODEX]);
    expect(r.sessions[0]?.hits).toHaveLength(1);
  });

  test("reads OpenCode sessions through the registry", async () => {
    const r = await search({ q: `hi from ${OC}` });
    expect(ids(r)).toEqual([OC]);
    expect(r.sessions[0]?.agent).toBe("opencode");
  });

  test("filters by agent, project and since", async () => {
    expect(ids(await search({ q: "quokka", agent: "pi" }))).toEqual([PI]);
    expect(
      ids(await search({ q: "quokka", project: "-home-demo-other" }))
    ).toEqual([CLAUDE_OLD]);
    expect(
      ids(await search({ q: "quokka", since: "2026-04-01T00:00:00Z" }))
    ).toEqual([CLAUDE_NEW, CLAUDE_MID]);
  });

  test("limit counts sessions; truncated is true only when more matched", async () => {
    const some = await search({ q: "quokka", limit: 2 });
    expect(ids(some)).toEqual([CLAUDE_NEW, CLAUDE_MID]);
    expect(some.truncated).toBe(true);
    const exact = await search({ q: "quokka", limit: 4 });
    expect(exact.sessions).toHaveLength(4);
    expect(exact.truncated).toBe(false);
    expect(ids(await search({ q: "quokka", limit: 0 }))).toHaveLength(4);
  });

  test("stops scanning once the limit is met", async () => {
    const r = await search({ q: "quokka", limit: 1 });
    expect(r.scannedSessions).toBeLessThan(6 + FILLER);
  });

  test("redacts secrets by default and keeps them on request", async () => {
    const on = await search({ q: "quokka", limit: 1 });
    const text = JSON.stringify(on);
    expect(text).not.toContain(FAKE_KEY);
    expect(text).toContain("[REDACTED");
    const off = await search({ q: FAKE_KEY, redact: false });
    expect(off.sessions[0]?.hits[0]?.snippet).toContain(FAKE_KEY);
  });

  test("redacts project and title, and leaves them raw on request", async () => {
    const on = await search({ q: "quokka", agent: "claude" });
    expect(on.sessions[0]?.project).not.toContain("/home/demo");
    expect(on.sessions[0]?.project).toBe(redactDisplay("-home-demo-work"));
    expect(redactDisplay(RAW_TITLE)).not.toBe(RAW_TITLE);
    expect(on.sessions[0]?.title).toBe(redactDisplay(RAW_TITLE));
    const off = await search({ q: "quokka", agent: "claude", redact: false });
    expect(off.sessions[0]?.project).toBe("-home-demo-work");
    expect(off.sessions[0]?.title).toBe(RAW_TITLE);
  });

  test("a redacted project name filters like the raw one", async () => {
    const shown = redactDisplay("-home-demo-other");
    expect(shown).not.toBe("-home-demo-other");
    expect(ids(await search({ q: "quokka", project: shown }))).toEqual([
      CLAUDE_OLD,
    ]);
  });

  test("metadata such as a git branch is not searched", async () => {
    expect(ids(await search({ q: "zebra-branch" }))).toEqual([]);
  });

  test("a blank query or a negative limit does not scan everything", async () => {
    expect(await search({ q: "   " })).toEqual({
      sessions: [],
      truncated: false,
      scannedSessions: 0,
      unreadable: 0,
    });
    const neg = await search({ q: "quokka", limit: -1 });
    expect(ids(neg)).toEqual([CLAUDE_NEW, CLAUDE_MID, PI, CLAUDE_OLD]);
  });

  test.skipIf(process.getuid?.() === 0)(
    "counts a session it cannot read",
    async () => {
      const file = join(
        base,
        "claude",
        "-home-demo-locked",
        `${id("9")}.jsonl`
      );
      put(file, [claudeLine("user", "2026-05-09T10:00:00.000Z", "quokka")]);
      chmodSync(file, 0o000);
      try {
        const r = await search({ q: "quokka" });
        expect(r.unreadable).toBe(1);
        expect(ids(r)).not.toContain(id("9"));
      } finally {
        chmodSync(file, 0o644);
        rmSync(file);
      }
    }
  );

  test("no match gives an empty result", async () => {
    const r = await search({ q: "nothing-like-this" });
    expect(r).toMatchObject({ sessions: [], truncated: false, unreadable: 0 });
  });
});
