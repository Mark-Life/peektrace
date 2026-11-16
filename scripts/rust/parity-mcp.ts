/** `mcp` parity: one process per side reads the same request script; the
 * reply lines are keyed by their JSON id (duplicates counted, since both
 * servers answer in any order) and compared byte for byte. A stats tool's
 * text is compared after masking its clock (window line, `generatedAt`,
 * `fromIso`, `toIso`). stderr must be empty and the exit code 0.
 *
 * Fixture: a synthetic home with two Claude sessions and a Codex rollout.
 * Real corpus: listing, search on a word from a real title and analyze on
 * the newest and largest sessions; a mismatch is run again once (live
 * sessions). */
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import {
  REPO,
  type Run,
  report,
  runAsync,
  rustBin,
  TS_CMD,
  WORK,
} from "./parity-lib";
import { canonStatsClock, maskStatsClock } from "./parity-stats-cases";

const U2028 = String.fromCharCode(0x20_28);
const STATS_TOOLS = new Set(["stats_report", "stats_drill"]);
const SCANNED = /"(scannedSessions|unreadable)": \d+/g;
const TRUNCATED = '"truncated": true';
const MIN_WORD = 5;
const NON_LETTERS = /[^A-Za-z]+/;

const call = (id: unknown, name: string, args?: unknown) =>
  JSON.stringify({
    jsonrpc: "2.0",
    id,
    method: "tools/call",
    params: args === undefined ? { name } : { name, arguments: args },
  });

const rpc = (v: Record<string, unknown>) =>
  JSON.stringify({ jsonrpc: "2.0", ...v });

/** The envelope, protocol and validation requests. */
const protocolLines = () => [
  rpc({
    id: 1,
    method: "initialize",
    params: { protocolVersion: "2025-06-18", capabilities: {} },
  }),
  rpc({ method: "notifications/initialized" }),
  rpc({ id: 2, method: "tools/list" }),
  rpc({ id: 3, method: "ping" }),
  rpc({
    id: "s",
    method: "initialize",
    params: { protocolVersion: "1999-01-01" },
  }),
  rpc({ id: "ini-null", method: "initialize", params: null }),
  rpc({ id: "ini-num", method: "initialize", params: { protocolVersion: 5 } }),
  rpc({
    id: "ini-2024",
    method: "initialize",
    params: { protocolVersion: "2024-11-05" },
  }),
  rpc({ id: "ini-none", method: "initialize" }),
  rpc({ id: 5, method: "nope" }),
  JSON.stringify({ jsonrpc: "1.0", id: 6, method: "ping" }),
  rpc({ id: null, method: "ping" }),
  rpc({ id: 1.5, method: "ping" }),
  rpc({ id: true, method: "ping" }),
  rpc({ id: "b1", method: 5 }),
  JSON.stringify([{ jsonrpc: "2.0", id: "batch", method: "ping" }]),
  rpc({ id: "noMethod" }),
  JSON.stringify({ id: "noVersion", method: "ping" }),
  rpc({ id: 7, method: "tools/call", params: { name: "bogus" } }),
  rpc({ id: 8, method: "tools/call", params: {} }),
  rpc({ id: "call-arr", method: "tools/call", params: [] }),
  rpc({ id: "call-num-name", method: "tools/call", params: { name: 5 } }),
  rpc({ id: "notif-call", method: "tools/list", extra: 1 }),
  rpc({ id: "dup", method: "ping" }),
  rpc({ id: "dup", method: "ping" }),
  '{"jsonrpc":"2.0","id":"crlf","method":"ping"}\r',
  "   ",
  "not json",
  "",
];

const validationLines = () => [
  call("args-null", "sessions_list", null),
  call("args-arr", "sessions_list", []),
  call("args-str", "sessions_list", "x"),
  call("limit-str", "sessions_list", { limit: "5" }),
  call("limit-frac", "sessions_list", { limit: 1.5 }),
  call("limit-null", "sessions_list", { limit: null }),
  call("limit-big", "sessions_list", { limit: 1e300 }),
  call("limit-huge", "sessions_list", { limit: "HUGE" }).replace(
    '"HUGE"',
    "12345678901234567890"
  ),
  call("limit-neg", "sessions_list", { limit: -1 }),
  call("limit-negzero", "sessions_list", { limit: "NEGZERO" }).replace(
    '"NEGZERO"',
    "-0"
  ),
  call("two-bad", "sessions_list", { redact: false, limit: -1, agent: "x" }),
  call("two-bad2", "sessions_list", { limit: -1, agent: "x" }),
  call("project-arr", "sessions_list", { project: [1, true] }),
  call("since-obj", "sessions_list", { since: { a: [1, "x"] } }),
  call("latest-str", "sessions_analyze", { latest: "yes" }),
  call("q-num", "sessions_search", { q: 5 }),
  call("drill-missing", "stats_drill", {}),
  call("drill-key-num", "stats_drill", { table: "cause", key: 5 }),
  call("drill-bad-table", "stats_drill", { table: "bogus", key: "k" }),
  call("drill-page0", "stats_drill", { table: "cause", key: "k", pageSize: 0 }),
  call("format-bad", "stats_report", { format: "yaml" }),
  call("days0", "stats_report", { days: 0 }),
  call("rows-frac", "stats_report", { limit: 2.5 }),
];

const toolLines = () => [
  call(10, "sessions_list", { limit: 5 }),
  call(11, "sessions_list", { format: "json", limit: 1 }),
  call(12, "sessions_list", { since: "bogus" }),
  call(15, "sessions_list", { agent: "nope" }),
  call(16, "sessions_list", { cwd: "~/nowhere" }),
  call("lim1", "sessions_list", { limit: 1 }),
  call("since-7d", "sessions_list", { since: "7d" }),
  call("proj", "sessions_list", {
    project: "-home-demo-work",
    agent: "claude",
  }),
  call("codex", "sessions_list", { agent: "codex" }),
  call("all", "sessions_list", { agent: "all", limit: 0, format: "json" }),
  call("cwd-home", "sessions_list", { cwd: "/home/demo" }),
  call(20, "sessions_search", { q: "hello" }),
  call(21, "sessions_search", { q: "zzzznomatch" }),
  call(22, "sessions_search", { q: "hello", limit: 1, format: "json" }),
  call(23, "sessions_search", { q: "  " }),
  call(24, "sessions_search", {}),
  call("search-since", "sessions_search", { q: "hello", since: "nope" }),
  call("search-codex", "sessions_search", { q: "codex", agent: "codex" }),
  call(30, "sessions_analyze", { latest: true }),
  call(31, "sessions_analyze", {}),
  call(32, "sessions_analyze", { id: "44444444", format: "json" }),
  call(33, "sessions_analyze", { id: "x", latest: true }),
  call(34, "sessions_analyze", { parent: "x" }),
  call(35, "sessions_analyze", { id: "nosuch" }),
  call("dash-id", "sessions_analyze", { id: "-x" }),
  call("cwd-rel", "sessions_analyze", { cwd: "." }),
  call("cwd-work", "sessions_analyze", { cwd: "/home/demo/work" }),
  call(40, "stats_report", { days: 3650 }),
  call(41, "stats_report", { days: 3650, format: "json", limit: 2 }),
  call(42, "stats_drill", { table: "cause", key: "nope", days: 3650 }),
  call("drill-fail", "stats_drill", { table: "fail", key: "nope", days: 3650 }),
  call("u2028", "sessions_search", { q: `a${U2028}b` }).replace(
    "\\u2028",
    U2028
  ),
];

const claudeLines = (at: string, contextTokens: number, cwd: string) => [
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
];

const codexLines = (id: string, cwd: string) => [
  {
    timestamp: "2026-06-01T10:00:00.000Z",
    type: "session_meta",
    payload: { id, timestamp: "2026-06-01T10:00:00.000Z", cwd },
  },
  {
    timestamp: "2026-06-01T10:00:00.000Z",
    type: "response_item",
    payload: {
      type: "message",
      role: "user",
      content: [{ type: "input_text", text: "hello codex" }],
    },
  },
];

const put = (path: string, lines: readonly unknown[]) => {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${lines.map((l) => JSON.stringify(l)).join("\n")}\n`);
};

const buildHome = () => {
  const home = join(WORK, "mcp-home");
  const claude = (slug: string, id: string) =>
    join(home, ".claude", "projects", slug, `${id}.jsonl`);
  put(
    claude("-home-demo-work", "44444444-4444-4444-8444-444444444444"),
    claudeLines("2026-03-01T10:00:00.000Z", 50_000, "/home/demo/work")
  );
  put(
    claude("-home-demo-other", "55555555-5555-4555-8555-555555555555"),
    claudeLines("2026-03-02T10:00:00.000Z", 150_000, "/home/demo/other")
  );
  const codexId = "019a0000-0000-7000-8000-000000000001";
  put(
    join(
      home,
      ".codex",
      "sessions",
      "2026",
      "06",
      "01",
      `rollout-2026-06-01T10-00-00-${codexId}.jsonl`
    ),
    codexLines(codexId, "/home/demo/work")
  );
  return home;
};

/** A stats tool's clock, and the search counts that vary when a search is
 * truncated (a known difference). */
const mask = (tool: string, text: string) => {
  if (STATS_TOOLS.has(tool)) {
    return maskStatsClock(text);
  }
  return text.includes(TRUNCATED) ? text.replace(SCANNED, '"$1": <n>') : text;
};

interface Reply {
  readonly key: string;
  readonly line: string;
  readonly masked: string;
}

/** Each reply line, keyed by its id; a duplicate id gets `#n`. */
const replies = (stdout: string, names: Map<string, string>) => {
  const seen = new Map<string, number>();
  const out = new Map<string, Reply>();
  for (const line of stdout.split("\n").filter((l) => l !== "")) {
    const v = JSON.parse(line) as {
      id: unknown;
      result?: { content?: { text: string }[] };
    };
    const id = JSON.stringify(v.id);
    const n = seen.get(id) ?? 0;
    seen.set(id, n + 1);
    const content = v.result?.content;
    const masked = content
      ? JSON.stringify({
          ...v,
          result: {
            ...v.result,
            content: content.map((c) => ({
              ...c,
              text: mask(names.get(id) ?? "", c.text),
            })),
          },
        })
      : line;
    out.set(`${id}#${n}`, { key: id, line, masked });
  }
  return out;
};

/** Tool name per request id, for the stats masking. */
const toolNames = (lines: readonly string[]) => {
  const names = new Map<string, string>();
  for (const l of lines) {
    try {
      const v = JSON.parse(l) as { id?: unknown; params?: { name?: unknown } };
      if (typeof v.params?.name === "string") {
        names.set(JSON.stringify(v.id), v.params.name);
      }
    } catch {
      // not JSON: a parse-error case
    }
  }
  return names;
};

const runSide = (
  cmd: readonly string[],
  env: Record<string, string>,
  input: string,
  cwd: string
) => runAsync([...cmd, "--no-telemetry", "mcp"], env, input, cwd, byReplyId);

interface ReplyLine {
  readonly id?: unknown;
  readonly result?: { content?: { text: string }[] };
}

/** One reply line with stats clocks masked in its text content. */
const canonReply = (line: string) => {
  try {
    const v = JSON.parse(line) as ReplyLine;
    const content = v.result?.content;
    if (!content) {
      return line;
    }
    const texts = content.map((c) => ({
      ...c,
      text: canonStatsClock({ code: 0, stdout: c.text, stderr: "" }).stdout,
    }));
    return texts.every((c, i) => c.text === content[i]?.text)
      ? line
      : JSON.stringify({ ...v, result: { ...v.result, content: texts } });
  } catch {
    return line;
  }
};

const idOf = (line: string) => {
  try {
    return JSON.stringify((JSON.parse(line) as ReplyLine).id) ?? "";
  } catch {
    return "";
  }
};

/** Record-time canon: replies sorted by id (stable, so repeats of one id
 * keep their order), since concurrent tool calls answer in any order and
 * the compare matches replies by id; stats clocks masked as the compare
 * masks them. */
const byReplyId = (r: Run): Run => {
  const sorted = r.stdout
    .split("\n")
    .filter((l) => l !== "")
    .map((line, i) => ({ line: canonReply(line), i, id: idOf(line) }))
    .sort((a, b) => a.id.localeCompare(b.id, "en") || a.i - b.i)
    .map((x) => x.line);
  return {
    ...r,
    stdout: sorted.length > 0 ? `${sorted.join("\n")}\n` : r.stdout,
  };
};

/** Diffs between the two sides' replies; empty when they match. */
const compare = (
  label: string,
  lines: readonly string[],
  ts: Awaited<ReturnType<typeof runSide>>,
  rust: Awaited<ReturnType<typeof runSide>>
) => {
  const found: string[] = [];
  if (ts.code !== rust.code) {
    found.push(`${label}: exit ts=${ts.code} rust=${rust.code}`);
  }
  for (const [side, r] of [
    ["ts", ts],
    ["rust", rust],
  ] as const) {
    if (r.stderr !== "") {
      found.push(
        `${label}: ${side} stderr ${JSON.stringify(r.stderr.slice(0, 300))}`
      );
    }
  }
  const names = toolNames(lines);
  const a = replies(ts.stdout, names);
  const b = replies(rust.stdout, names);
  for (const [k, r] of a) {
    const other = b.get(k);
    if (other === undefined) {
      found.push(`${label} ${k}: missing in rust`);
    } else if (r.masked !== other.masked) {
      found.push(
        `${label} ${k}: ts=${r.masked.slice(0, 400)} rust=${other.masked.slice(0, 400)}`
      );
    }
  }
  for (const k of b.keys()) {
    if (!a.has(k)) {
      found.push(
        `${label} ${k}: extra in rust ${b.get(k)?.line.slice(0, 200)}`
      );
    }
  }
  return { found, count: a.size };
};

const runScript = async (
  label: string,
  lines: readonly string[],
  env: Record<string, string>,
  cwd: string,
  retry: boolean,
  liveFilter: () => Promise<(f: string) => boolean> = async () => () => false
) => {
  const input = `${lines.join("\n")}\n`;
  for (let attempt = 0; ; attempt++) {
    const [ts, rust] = await Promise.all([
      runSide(TS_CMD, env, input, cwd),
      runSide([rustBin], env, input, cwd),
    ]);
    const { found, count } = compare(label, lines, ts, rust);
    if (found.length === 0 || !retry || attempt > 0) {
      const live = await liveFilter();
      const kept = found.filter((f) => !live(f));
      if (kept.length < found.length) {
        console.log(
          `  ${found.length - kept.length} live-session replies skipped`
        );
      }
      for (const f of kept) {
        report(f);
      }
      return count;
    }
  }
};

interface Row {
  readonly id: string;
  readonly sizeBytes: number;
  readonly title?: string;
}

/** A word of at least 5 letters from a real session title. */
const titleWord = (rows: readonly Row[]) =>
  rows
    .flatMap((r) => (r.title ?? "").split(NON_LETTERS))
    .find((w) => w.length >= MIN_WORD) ?? "parity";

const listRows = async (env: Record<string, string>) => {
  const ls = await runAsync(
    [rustBin, "--json", "sessions", "ls", "--limit", "0"],
    env
  );
  return JSON.parse(ls.stdout || "[]") as Row[];
};

/** Case ids whose session grew while the suite ran (an agent is writing
 * to it): their replies differ by timing, not by behaviour. */
const liveCases =
  (before: readonly Row[], picked: readonly Row[]) =>
  async (env: Record<string, string>) => {
    const after = new Map(
      (await listRows(env)).map((r) => [r.id, r.sizeBytes] as const)
    );
    const grew = (r: Row | undefined) =>
      r !== undefined && after.get(r.id) !== r.sizeBytes;
    const ids = new Set<string>();
    if (grew(before[0])) {
      ids.add("r-latest");
    }
    picked.forEach((r, i) => {
      if (grew(r)) {
        ids.add(`r-an-${i}`);
        ids.add(`r-an-json-${i}`);
      }
    });
    return (f: string) => [...ids].some((id) => f.includes(`"${id}"`));
  };

const realLines = async (env: Record<string, string>) => {
  const rows = await listRows(env);
  const largest = [...rows]
    .sort((x, y) => y.sizeBytes - x.sizeBytes)
    .slice(0, 2);
  const picked = [...rows.slice(0, 2), ...largest];
  const q = titleWord(rows.slice(0, 20));
  const live = liveCases(rows, picked);
  const lines = [
    call("r-ls3", "sessions_list", { limit: 3 }),
    call("r-ls20", "sessions_list", { limit: 20 }),
    call("r-ls-json", "sessions_list", { limit: 5, format: "json" }),
    ...["claude", "codex", "pi", "opencode"].map((agent) =>
      call(`r-ls-${agent}`, "sessions_list", { limit: 4, agent })
    ),
    call("r-search", "sessions_search", { q, limit: 3 }),
    call("r-search-json", "sessions_search", { q, limit: 2, format: "json" }),
    call("r-latest", "sessions_analyze", { latest: true }),
    ...picked.flatMap((r, i) => [
      call(`r-an-${i}`, "sessions_analyze", { id: r.id }),
      call(`r-an-json-${i}`, "sessions_analyze", { id: r.id, format: "json" }),
    ]),
  ];
  return { lines, live: () => live(env) };
};

export const runMcpStage = async ({
  fixtures,
  real,
}: {
  readonly fixtures: boolean;
  readonly real: boolean;
}) => {
  if (fixtures) {
    const home = buildHome();
    const env = {
      PATH: process.env.PATH ?? "",
      HOME: home,
      TZ: "UTC",
      CLAUDE_CONFIG_DIR: join(home, ".claude"),
      CODEX_HOME: join(home, ".codex"),
      XDG_DATA_HOME: join(home, ".local", "share"),
      PEEKTRACE_DIR: join(home, ".peektrace"),
      PEEKTRACE_NO_TELEMETRY: "1",
      PEEKTRACE_NO_UPDATE_CHECK: "1",
    };
    const lines = [...protocolLines(), ...validationLines(), ...toolLines()];
    const n = await runScript("mcp", lines, env, home, false);
    const remote = await runAsync(
      [...TS_CMD, "--remote", "http://x", "mcp"],
      env
    );
    const rustRemote = await runAsync(
      [rustBin, "mcp", "--remote", "http://x"],
      env
    );
    if (JSON.stringify(remote) !== JSON.stringify(rustRemote)) {
      report(
        `mcp --remote: ts=${JSON.stringify(remote)} rust=${JSON.stringify(rustRemote)}`
      );
    }
    console.log(`suite mcp: ${n} replies, 1 remote case`);
  }
  if (real) {
    const env = {
      ...(process.env as Record<string, string>),
      PEEKTRACE_NO_TELEMETRY: "1",
      PEEKTRACE_NO_UPDATE_CHECK: "1",
    };
    const { lines, live } = await realLines(env);
    const n = await runScript("mcp-real", lines, env, REPO, true, live);
    console.log(`suite mcp-real: ${n} replies`);
  }
};
