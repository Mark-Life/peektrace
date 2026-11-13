/** `statusline` parity: the same stdin payload and flags on both CLIs,
 * stdout, stderr and exit compared byte for byte.
 *
 * Fixture: a synthetic home with 50k, 150k and 30M token Claude transcripts
 * and an empty one. Every case that should succeed passes `--timeout 60000`
 * on both sides, since TS can spend ~400 ms gathering context files. The
 * timing cases (`--timeout 0`) and the held-open stdin case are Rust tests.
 * Real corpus: the newest and largest Claude sessions, by path and id, with
 * and without a window; live sessions are skipped. */
import { mkdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import {
  compareRun,
  fixtureEnv,
  newestAndLargest,
  REPO,
  type Run,
  report,
  run,
  rustBin,
  TS_CMD,
  WORK,
} from "./parity-lib";

const ID = "44444444-4444-4444-8444-444444444444";
const BIG = "55555555-5555-4555-8555-555555555555";
const HUGE = "66666666-6666-4666-8666-666666666666";
const EMPTY = "77777777-7777-4777-8777-777777777777";
const SLOW_OK = ["--timeout", "60000"];
const BOM = String.fromCharCode(0xfe_ff);
const DURATION = / \d+(\.\d+)?ms /;
const RPC_SPAN = /^\[otel\] Rpc(Server|Client)\./;
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

const claudeLines = (at: string, contextTokens: number, cwd: string) =>
  [
    { type: "user", cwd, timestamp: at, message: { content: "hello" } },
    {
      type: "assistant",
      cwd,
      timestamp: at,
      requestId: "req-1",
      message: {
        model: "claude-opus-4-1",
        content: [{ type: "text", text: "hi" }],
        usage: {
          input_tokens: contextTokens,
          cache_read_input_tokens: 0,
          cache_creation_input_tokens: 0,
          output_tokens: 10,
        },
      },
    },
  ]
    .map((l) => JSON.stringify(l))
    .join("\n");

const put = (path: string, body: string) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, body);
};

/** The synthetic home; returns it and the transcript paths. */
const buildHome = () => {
  const home = join(WORK, "statusline-home");
  const dir = join(home, ".claude", "projects", "-home-demo-work");
  const p = (id: string) => join(dir, `${id}.jsonl`);
  put(
    p(ID),
    `${claudeLines("2026-03-01T10:00:00.000Z", 50_000, "/home/demo/work")}\n`
  );
  put(
    p(BIG),
    `${claudeLines("2026-03-02T10:00:00.000Z", 150_000, "/home/demo/work")}\n`
  );
  put(
    p(HUGE),
    `${claudeLines("2026-03-03T10:00:00.000Z", 30_000_000, "/home/demo/work")}\n`
  );
  put(p(EMPTY), "");
  return {
    home,
    path: p(ID),
    big: p(BIG),
    huge: p(HUGE),
    empty: p(EMPTY),
    dir,
  };
};

interface Case {
  readonly args?: readonly string[];
  readonly name: string;
  /** Global flags before `statusline`. */
  readonly pre?: readonly string[];
  readonly stdin: string | Uint8Array;
}

const fixtureCases = (h: ReturnType<typeof buildHome>): Case[] => {
  const j = (v: unknown) => JSON.stringify(v);
  const win = (path: string, size: unknown) =>
    j({ transcript_path: path, context_window: { context_window_size: size } });
  const ok = (name: string, stdin: string | Uint8Array): Case => ({
    name,
    stdin,
    args: SLOW_OK,
  });
  const badByte = new Uint8Array([
    ...new TextEncoder().encode(`{"session_id":"${ID}","x":"a`),
    0xff,
    ...new TextEncoder().encode('b"}'),
  ]);
  return [
    ok("path", j({ transcript_path: h.path })),
    ok("id", j({ session_id: ID })),
    ok("id-upper", j({ session_id: ID.toUpperCase() })),
    ok("bom", BOM + j({ session_id: ID })),
    ok("bad-utf8", badByte),
    ok("window200k", win(h.big, 200_000)),
    ok("window-0", win(h.big, 0)),
    ok("window-neg", win(h.big, -5)),
    ok("window-frac", win(h.big, 1000.5)),
    ok("window-str", win(h.big, "200000")),
    ok("window-null", win(h.big, null)),
    ok("window-tiny", win(h.big, 1000)),
    ok("window-not-obj", j({ transcript_path: h.big, context_window: 5 })),
    ok("huge", j({ transcript_path: h.huge })),
    ok("degrading", win(h.big, 300_000)),
    ok("empty-file", j({ transcript_path: h.empty })),
    ok("badpath-id", j({ transcript_path: "/nope/x.jsonl", session_id: ID })),
    ok("badpath-noid", j({ transcript_path: "/nope/x.jsonl" })),
    ok("shortid", j({ session_id: "4444" })),
    ok("shortid-path", j({ session_id: "44444444", transcript_path: h.path })),
    ok("null-fields", j({ session_id: null, transcript_path: null })),
    ok("null-path-id", j({ session_id: ID, transcript_path: null })),
    ok("empty-path", j({ transcript_path: "", session_id: ID })),
    ok("array", "[]"),
    ok("null", "null"),
    ok("number", "5"),
    ok("number-path", j({ transcript_path: 5, session_id: ID })),
    ok("number-id", j({ session_id: 5 })),
    ok("notjson", "not json"),
    ok("empty", ""),
    ok("obj", "{}"),
    ok("ws-json", `  \n${j({ session_id: ID })}\n\n`),
    ok("bare-uuid-path", j({ transcript_path: ID })),
    ok("rel-path", j({ transcript_path: "x.jsonl" })),
    ok("tilde-path", j({ transcript_path: "~/x.jsonl" })),
    ok("dir-path", j({ transcript_path: h.dir })),
    ok("trailing", `${j({ session_id: ID })} x`),
    ok("dup-key", `{"session_id":"${ID}","session_id":5}`),
    ok("oversized", j({ session_id: ID, pad: "x".repeat(3_000_000) })),
    {
      name: "json-flag",
      stdin: j({ transcript_path: h.path }),
      args: [...SLOW_OK, "--json"],
    },
    {
      name: "pretty",
      stdin: j({ transcript_path: h.path }),
      args: [...SLOW_OK, "--pretty"],
    },
    {
      name: "read-only",
      stdin: j({ transcript_path: h.path }),
      args: [...SLOW_OK, "--read-only"],
    },
    { name: "timeout-abc", stdin: "{}", args: ["--timeout", "abc"] },
    { name: "timeout-frac", stdin: "{}", args: ["--timeout", "1.5"] },
    {
      name: "timeout-big",
      stdin: "{}",
      args: ["--timeout", "99999999999999999999"],
    },
    {
      name: "timeout-exp",
      stdin: j({ session_id: ID }),
      args: ["--timeout", "1e5"],
    },
    {
      name: "timeout-eq",
      stdin: j({ session_id: ID }),
      args: ["--timeout=60000"],
    },
    {
      name: "remote-bad",
      stdin: j({ transcript_path: h.path }),
      pre: ["--remote", "http://127.0.0.1:1"],
      args: SLOW_OK,
    },
  ];
};

const runWith = (
  cmd: readonly string[],
  env: Record<string, string>,
  stdin: string | Uint8Array,
  canon?: (r: Run) => Run
): Run => run(cmd, env, { stdin, cwd: REPO, canon });

/** Record-time canon of an `--otel` run: span durations read as 0 ms. */
const zeroDurations = (r: Run): Run => ({
  ...r,
  stderr: r.stderr.replace(new RegExp(DURATION, "g"), " 0ms "),
});

const argvOf = (c: Case) => [
  "--no-telemetry",
  ...(c.pre ?? []),
  "statusline",
  ...(c.args ?? []),
];

const runCase = (label: string, c: Case, env: Record<string, string>) => {
  const argv = argvOf(c);
  const ts = runWith([...TS_CMD, ...argv], env, c.stdin);
  const rust = runWith([rustBin, ...argv], env, c.stdin);
  compareRun(`statusline ${label}`, ts, rust);
};

/** Span names of an `--otel` run, timings dropped and RPC spans removed. */
const spanNames = (stderr: string) =>
  stderr
    .split("\n")
    .filter((l) => l.startsWith("[otel] "))
    .map((l) => l.replace(DURATION, " "))
    .filter((l) => !RPC_SPAN.test(l));

const otelCase = (
  h: ReturnType<typeof buildHome>,
  env: Record<string, string>
) => {
  const stdin = JSON.stringify({ transcript_path: h.path });
  const argv = ["--otel", "--no-telemetry", "statusline", ...SLOW_OK];
  const ts = runWith([...TS_CMD, ...argv], env, stdin, zeroDurations);
  const rust = runWith([rustBin, ...argv], env, stdin);
  compareRun(
    "statusline otel (stdout)",
    { ...ts, stderr: "" },
    { ...rust, stderr: "" }
  );
  const a = spanNames(ts.stderr).join("\n");
  const b = spanNames(rust.stderr).join("\n");
  if (a !== b) {
    report(
      `statusline otel spans: ts=${JSON.stringify(a)} rust=${JSON.stringify(b)}`
    );
  }
};

interface Row {
  readonly id: string;
  readonly path: string;
  readonly sizeBytes: number;
}

const stamp = (path: string) => {
  try {
    const s = statSync(path);
    return `${s.size}:${s.mtimeMs}`;
  } catch {
    return "missing";
  }
};

const realCases = (rows: readonly Row[]): Case[] =>
  rows.flatMap((r) => [
    {
      name: `${r.id} path`,
      stdin: JSON.stringify({ session_id: r.id, transcript_path: r.path }),
      args: SLOW_OK,
    },
    {
      name: `${r.id} window`,
      stdin: JSON.stringify({
        session_id: r.id,
        transcript_path: r.path,
        context_window: { context_window_size: 200_000 },
      }),
      args: SLOW_OK,
    },
    {
      name: `${r.id} id`,
      stdin: JSON.stringify({ session_id: r.id }),
      args: SLOW_OK,
    },
  ]);

const runReal = (count: { cases: number; live: number }) => {
  const env = {
    ...(process.env as Record<string, string>),
    PEEKTRACE_NO_TELEMETRY: "1",
    PEEKTRACE_NO_UPDATE_CHECK: "1",
  };
  const ls = Bun.spawnSync(
    [rustBin, "--json", "sessions", "ls", "--agent", "claude", "--limit", "0"],
    { env }
  );
  const rows = (JSON.parse(ls.stdout.toString() || "[]") as Row[]).filter((r) =>
    r.path.endsWith(".jsonl")
  );
  for (const row of newestAndLargest(rows, 4, 2)) {
    for (const c of realCases([row])) {
      const before = stamp(row.path);
      const argv = argvOf(c);
      const ts = runWith([...TS_CMD, ...argv], env, c.stdin);
      const rust = runWith([rustBin, ...argv], env, c.stdin);
      if (stamp(row.path) !== before) {
        count.live += 1;
        continue;
      }
      count.cases += 1;
      compareRun(`statusline real ${c.name}`, ts, rust);
    }
  }
};

export const runStatuslineStage = ({
  fixtures,
  real,
}: {
  readonly fixtures: boolean;
  readonly real: boolean;
}) => {
  if (fixtures) {
    const h = buildHome();
    const env = fixtureEnv(h.home, {
      CLAUDE_CONFIG_DIR: join(h.home, ".claude"),
      PEEKTRACE_NO_UPDATE_CHECK: "1",
    });
    const cases = fixtureCases(h);
    for (const c of cases) {
      runCase(c.name, c, env);
    }
    otelCase(h, env);
    console.log(`suite statusline: ${cases.length + 1} cases`);
  }
  if (real) {
    const count = { cases: 0, live: 0 };
    runReal(count);
    console.log(
      `suite statusline-real: ${count.cases} cases, ${count.live} skipped as live`
    );
  }
};

/** `statusline --remote` on the serve pair: TS against the TS server, Rust
 * against the Rust one, on the same payloads. Includes the not-found retry
 * by session id. */
export const remoteStatusline = (
  ports: { readonly rust: number; readonly ts: number },
  env: Record<string, string>,
  rows: readonly { readonly id: string; readonly path: string }[]
) => {
  const uuids = rows.filter((r) => UUID.test(r.id)).slice(0, 4);
  const payloads = [
    ...uuids.flatMap((r) => [
      { session_id: r.id },
      { transcript_path: r.path },
      { transcript_path: "/nope/x.jsonl", session_id: r.id },
      { session_id: r.id, context_window: { context_window_size: 200_000 } },
    ]),
    { session_id: "99999999-9999-4999-8999-999999999999" },
    { transcript_path: "/nope/x.jsonl" },
  ];
  let healthy = 0;
  for (const p of payloads) {
    const stdin = JSON.stringify(p);
    const side = (port: number, cmd: readonly string[]) =>
      runWith(
        [
          ...cmd,
          "--no-telemetry",
          "--remote",
          `http://127.0.0.1:${port}`,
          "statusline",
          ...SLOW_OK,
        ],
        env,
        stdin
      );
    const rust = side(ports.rust, [rustBin]);
    healthy += rust.stdout.startsWith("ctx --") ? 0 : 1;
    compareRun(
      `statusline --remote ${stdin.slice(0, 80)}`,
      side(ports.ts, TS_CMD),
      rust
    );
  }
  if (healthy < uuids.length) {
    report(`statusline --remote: only ${healthy} payloads resolved`);
  }
  return payloads.length;
};
