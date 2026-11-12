/** Parity stage for `sessions search`: the fixture suites, a synthetic
 * corpus (the `search.test.ts` one, under a fake HOME), generated edge lines,
 * and the real corpus (`--json` only).
 *
 * Normalisation: when TS reports `truncated`, its scan stopped early with up
 * to 8 loads still in flight, so `scannedSessions` and `unreadable` vary run
 * to run. Both are dropped from both sides then, with the stderr
 * `N sessions could not be read` line. Real runs also drop sessions written
 * to during the run. */
import {
  chmodSync,
  existsSync,
  mkdirSync,
  statSync,
  utimesSync,
  writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { writeGoodDb } from "../../packages/core/test/fixtures/opencode-db";
import {
  compareRun,
  firstLineDiff,
  fixtureEnv,
  pool,
  REPO,
  type Run,
  report,
  run,
  runAsync,
  rustBin,
  type Suite,
  TS_CMD,
  WORK,
} from "./parity-lib";
import { storeStamp } from "./parity-opencode-stamp";

const FIXED_MTIME = new Date("2026-11-11T16:42:21.851Z");
const NO_ACCESS = 0o000;
const READ_WRITE = 0o644;
const FILLER = 30;
const DEPTH_IN = 64;
const DEPTH_OUT = 65;
const SEARCH = ["--no-telemetry", "sessions", "search"];
const QUERIES = [
  "the",
  "THE CONFIG",
  "config loader",
  'a"b',
  "walrus",
  "quokka",
  "привет мир",
  "src/app.ts",
  "sk-ant-api03",
  "an old walrus",
  "k-sign",
  "s-long ß-sharp",
  "ΟΔΟΣ",
];
const FAKE_KEY = "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234";
const JWT =
  "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4ifQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
const UNREADABLE_LINE = /^\d+ sessions? could not be read\n/m;
const locked: string[] = [];

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
const buildSynthetic = () => {
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
    jsonl([claudeLine(cwd, at("04-03"), "quokka in the middle, an old walrus")])
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
const buildEdge = () => {
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

const search = (...rest: string[]) => [...SEARCH, ...rest];

/** Args for one suite: every query in three renderings, then flags. */
const suiteArgs = (projects: readonly string[]) => [
  ...QUERIES.flatMap((q) => [
    search(q, "--json", "--limit", "0"),
    search(q, "--limit", "0"),
    ["--no-telemetry", "--pretty", "sessions", "search", q, "--limit", "0"],
  ]),
  search("--", "-v", "--json", "--limit", "0"),
  search("an", "old", "walrus", "--limit", "0"),
  search("the", "--limit", "1", "--json"),
  search("the", "--limit", "2"),
  search("walrus"),
  search("quokka", "--json"),
  ...["claude", "codex", "pi", "opencode", "all"].map((a) =>
    search("the", "--agent", a, "--json", "--limit", "0")
  ),
  ...projects.flatMap((p) => [
    search("the", `--project=${p}`, "--json", "--limit", "0"),
    search("walrus", `--project=${p}`, "--limit", "0"),
  ]),
  search("the", "--since", "2026-04-01", "--json", "--limit", "0"),
  search("the", "--since", "100000w", "--limit", "0"),
  search("sk-ant", "--no-redact", "--json", "--limit", "0"),
  search("walrus", "--no-redact", "--limit", "0"),
  search("zzz-no-such-text"),
  search("zzz-no-such-text", "--json"),
  search("walrus", "--agent", "bogus"),
  search("   "),
  search(""),
  search("walrus", "--limit", "-1"),
  search("walrus", "--limit", "1.5"),
  search("walrus", "--since", "bogus"),
  search("walrus", "--since", "99999999999w"),
];

const truncated = (stdout: string) => {
  try {
    return (JSON.parse(stdout) as { truncated?: boolean }).truncated === true;
  } catch {
    return false;
  }
};

/** Drop what a truncated TS scan leaves to timing. */
const settle = (r: Run, json: boolean): Run => {
  const stderr = r.stderr.replace(UNREADABLE_LINE, "");
  if (!json) {
    return { ...r, stderr };
  }
  try {
    const v = JSON.parse(r.stdout) as Record<string, unknown>;
    v.scannedSessions = undefined;
    v.unreadable = undefined;
    return { ...r, stdout: `${JSON.stringify(v, null, 2)}\n`, stderr };
  } catch {
    return { ...r, stderr };
  }
};

const runCase = async (
  name: string,
  env: Record<string, string>,
  args: string[]
) => {
  const [ts, rust] = await Promise.all([
    runAsync([...TS_CMD, ...args], env),
    runAsync([rustBin, ...args], env),
  ]);
  const json = args.includes("--json");
  const early = json
    ? truncated(ts.stdout)
    : ts.stderr.includes("more sessions match");
  const label = `${name} [${args.join(" ")}]`;
  if (early) {
    compareRun(label, settle(ts, json), settle(rust, json));
  } else {
    compareRun(label, ts, rust);
  }
};

/** Projects each suite filters on: raw slugs and their redacted forms. */
const projectsOf = (env: Record<string, string>) => {
  const out = run(
    [rustBin, ...SEARCH, "the", "--json", "--limit", "0", "--no-redact"],
    env
  );
  const raw =
    out.code === 0
      ? (JSON.parse(out.stdout) as { sessions: { project: string }[] })
      : { sessions: [] };
  const red = run([rustBin, ...SEARCH, "the", "--json", "--limit", "0"], env);
  const shown =
    red.code === 0
      ? (JSON.parse(red.stdout) as { sessions: { project: string }[] })
      : { sessions: [] };
  const picks = new Set(
    [...raw.sessions.slice(0, 2), ...shown.sessions.slice(0, 2)].map(
      (s) => s.project
    )
  );
  return [...picks, "-home-demo-work"];
};

const fixtureSuites = (suites: readonly Suite[]): Suite[] => {
  const emptyHome = join(WORK, "empty-home");
  mkdirSync(emptyHome, { recursive: true });
  const fixtures = join(REPO, "packages/core/test/fixtures");
  const reuse = suites.filter((s) =>
    ["agents-fixtures", "edge"].includes(s.name)
  );
  const ocMain = join(WORK, "oc", "main");
  const oc: Suite[] = existsSync(ocMain)
    ? [
        {
          name: "search-oc-main",
          env: fixtureEnv(join(WORK, "oc-home"), {
            PEEKTRACE_CLAUDE_PROJECTS: join(WORK, "oc-empty"),
            PEEKTRACE_CODEX_SESSIONS: join(WORK, "oc-empty"),
            PEEKTRACE_PI_SESSIONS: join(WORK, "oc-empty"),
            PEEKTRACE_OPENCODE_DATA: ocMain,
          }),
          args: [],
        },
      ]
    : [];
  return [
    {
      name: "search-ts-fixtures",
      env: fixtureEnv(emptyHome, {
        PEEKTRACE_CLAUDE_PROJECTS: fixtures,
        PEEKTRACE_CODEX_SESSIONS: fixtures,
        PEEKTRACE_PI_SESSIONS: fixtures,
        PEEKTRACE_OPENCODE_DATA: join(fixtures, "sessions-opencode"),
      }),
      args: [],
    },
    ...reuse.map((s) => ({ ...s, name: `search-${s.name}` })),
    ...oc,
    { name: "search-synthetic", env: fixtureEnv(buildSynthetic()), args: [] },
    { name: "search-lines", env: fixtureEnv(buildEdge()), args: [] },
  ];
};

const runFixtures = async (suites: readonly Suite[]) => {
  for (const suite of fixtureSuites(suites)) {
    const args = suiteArgs(projectsOf(suite.env));
    console.log(`suite ${suite.name}: ${args.length} cases`);
    await pool(args, (a) => runCase(suite.name, suite.env, a));
  }
};

// Real corpus

interface Listed {
  readonly agent: string;
  readonly id: string;
  readonly path: string;
}

interface Found {
  readonly agent: string;
  readonly id: string;
}

const OPENCODE_ROW = /#[^/\\]+(?<!\.jsonl)$/;

const stampOf = (path: string) => {
  const at = path.search(OPENCODE_ROW);
  if (at >= 0) {
    return storeStamp(path.slice(0, at));
  }
  try {
    const s = statSync(path);
    return `${s.size}:${s.mtimeMs}`;
  } catch {
    return "missing";
  }
};

const stamps = (env: Record<string, string>) => {
  const out = run(
    [rustBin, "--no-telemetry", "--json", "sessions", "ls", "--limit", "0"],
    env
  );
  const rows = JSON.parse(out.stdout) as Listed[];
  return new Map(rows.map((r) => [`${r.agent}:${r.id}`, stampOf(r.path)]));
};

/** Each session's raw JSON text in 2-space `--json` search output. */
const chunks = (stdout: string) => {
  const out: string[] = [];
  let current: string[] | undefined;
  for (const text of stdout.split("\n")) {
    if (text === "    {") {
      current = [];
    } else if (current && (text === "    }" || text === "    },")) {
      out.push(current.join("\n"));
      current = undefined;
    } else {
      current?.push(text);
    }
  }
  return out;
};

const sessionsOf = (r: Run, live: ReadonlySet<string>) => {
  const v = JSON.parse(r.stdout) as { sessions: Found[] };
  const raw = chunks(r.stdout);
  return v.sessions
    .map((s, i) => ({ key: `${s.agent}:${s.id}`, raw: raw[i] ?? "" }))
    .filter((s) => !live.has(s.key));
};

const compareReal = (
  label: string,
  ts: Run,
  rust: Run,
  live: ReadonlySet<string>
) => {
  if (ts.code !== rust.code) {
    report(`${label}: exit ts=${ts.code} rust=${rust.code}`);
    return 0;
  }
  const a = sessionsOf(ts, live);
  const b = sessionsOf(rust, live);
  const n =
    live.size > 0 ? Math.min(a.length, b.length) : Math.max(a.length, b.length);
  for (let i = 0; i < n; i++) {
    if (a[i]?.key !== b[i]?.key) {
      report(`${label}: session ${i} ts=${a[i]?.key} rust=${b[i]?.key}`);
      return 0;
    }
    if (a[i]?.raw !== b[i]?.raw) {
      report(
        `${label}: ${a[i]?.key} ${firstLineDiff(a[i]?.raw ?? "", b[i]?.raw ?? "")}`
      );
    }
  }
  if (live.size === 0) {
    const tail = (r: Run) => (truncated(r.stdout) ? settle(r, true) : r);
    const [t, r] = [tail(ts), tail(rust)];
    const meta = (s: string) => {
      const { sessions: _, ...rest } = JSON.parse(s) as Record<string, unknown>;
      return JSON.stringify(rest);
    };
    if (meta(t.stdout) !== meta(r.stdout)) {
      report(`${label}: ts=${meta(t.stdout)} rust=${meta(r.stdout)}`);
    }
    if (t.stderr !== r.stderr) {
      report(`${label}: stderr ${firstLineDiff(t.stderr, r.stderr)}`);
    }
  }
  return n;
};

const REAL_QUERIES = [
  "error",
  "parity harness",
  "TODO",
  "the",
  "ключ",
  '"',
  "sk-ant",
];
const REAL_AGENTS = ["all", "claude", "codex", "pi", "opencode"];

const runReal = async () => {
  const env = { ...process.env, PEEKTRACE_NO_TELEMETRY: "1" } as Record<
    string,
    string
  >;
  const cases = REAL_QUERIES.flatMap((q) =>
    REAL_AGENTS.flatMap((agent) =>
      ["0", "5"].map((limit) =>
        search(q, "--agent", agent, "--limit", limit, "--json")
      )
    )
  );
  console.log(`suite search-real: ${cases.length} cases`);
  const before = stamps(env);
  const runs = await pool(
    cases,
    async (args) => {
      const [ts, rust] = await Promise.all([
        runAsync([...TS_CMD, ...args], env),
        runAsync([rustBin, ...args], env),
      ]);
      return { args, ts, rust };
    },
    4
  );
  const after = stamps(env);
  const live = new Set(
    [...new Set([...before.keys(), ...after.keys()])].filter(
      (k) => before.get(k) !== after.get(k)
    )
  );
  let compared = 0;
  for (const { args, ts, rust } of runs) {
    compared += compareReal(`search-real [${args.join(" ")}]`, ts, rust, live);
  }
  console.log(
    `  compared ${compared} session results, ${live.size} live sessions skipped`
  );
};

export const runSearchStage = async (args: {
  readonly fixtures: boolean;
  readonly real: boolean;
  readonly suites: readonly Suite[];
}) => {
  try {
    if (args.fixtures) {
      await runFixtures(args.suites);
    }
    if (args.real) {
      await runReal();
    }
  } finally {
    for (const path of locked.filter((p) => existsSync(p))) {
      chmodSync(path, READ_WRITE);
    }
  }
};
