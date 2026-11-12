/** Parity stage for `peektrace tui`: flag errors byte for byte, the syntax
 * tokenizer token by token (fixture snippets, and real tool bodies kept in
 * WORK only), and the TUI's pure number, text and layout helpers.
 *
 * Screens are not compared here: the Rust TUI fixes the TS layout bugs, so
 * its frames differ on purpose (`crates/tui/tests/snapshots` holds them).
 * Non-TTY runs and `--port abc` are known differences. */
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { decodeBody } from "../../apps/cli/src/tui/screens/history-decode";
import {
  agoOf,
  columnsOf,
  layoutOf,
  pairCell,
  wrap,
} from "../../apps/cli/src/tui/screens/stats-format";
import {
  clip,
  fmt,
  fmtBytes,
  fmtK,
  fmtPct,
  fmtStarted,
  sanitize,
} from "../../apps/cli/src/tui/theme";
import { windowSlice } from "../../apps/cli/src/tui/use-list";
import {
  type CodeBlockLanguage,
  isHighlightable,
  plainLines,
  tokenizeLines,
} from "../../packages/core/src/highlight";
import { HIGHLIGHT_CASES } from "./parity-highlight-cases";
import {
  compareRun,
  fixtureEnv,
  option,
  report,
  run,
  rustBin,
  TS_CMD,
  WORK,
} from "./parity-lib";

const PORT_CASES: readonly (readonly string[])[] = [
  ["tui", "--port", "0", "--no-open"],
  ["tui", "--port", "70000", "--no-open"],
  ["tui", "--port", "-1", "--no-open"],
  ["tui", "--host", "0.0.0.0", "--port", "99999"],
];
/** Real sessions whose tool bodies feed the tokenizer comparison. */
const REAL_SESSIONS = 25;
/** Bodies above this many chars are skipped (both sides go plain anyway
 * past the limits; this keeps the stage fast). */
const REAL_BODY_MAX = 200_000;
/** Chars of a failed example's stderr quoted in the report. */
const STDERR_HEAD = 400;
const ESC = String.fromCharCode(27);
const BEL = String.fromCharCode(7);

/** Cargo keeps the caller's env, so RUSTFLAGS, CARGO_TARGET_DIR and the
 * toolchain settings match the build under test. */
const cargoEnv = process.env as Record<string, string>;

/** Where cargo puts release examples, honouring CARGO_TARGET_DIR and
 * `build.target-dir`. */
const releaseExamples = () => {
  const meta = run(
    ["cargo", "metadata", "--format-version", "1", "--no-deps"],
    cargoEnv
  );
  if (meta.code !== 0) {
    report(`tui: cargo metadata failed: ${meta.stderr.slice(0, STDERR_HEAD)}`);
    return null;
  }
  const { target_directory } = JSON.parse(meta.stdout) as {
    readonly target_directory: string;
  };
  return join(target_directory, "release", "examples");
};

/** The example binary next to `--bin`, as built with it; else a locked
 * release build of it. */
const exampleBin = (pkg: string, name: string) => {
  if (option("--bin") !== undefined) {
    const bin = join(dirname(rustBin), "examples", name);
    if (!existsSync(bin)) {
      report(`tui: missing ${bin}: build the examples beside --bin`);
      return null;
    }
    return bin;
  }
  const built = run(
    [
      "cargo",
      "build",
      "--locked",
      "--release",
      "-q",
      "-p",
      pkg,
      "--example",
      name,
    ],
    cargoEnv
  );
  if (built.code !== 0) {
    report(`tui: cannot build example ${name}: ${built.stderr.slice(0, 200)}`);
    return null;
  }
  const dir = releaseExamples();
  return dir === null ? null : join(dir, name);
};

/** One output line per input line, or `null` (reported) when the example
 * fails. */
const pipe = (bin: string, lines: readonly string[]) => {
  const out = Bun.spawnSync([bin], {
    stdin: new TextEncoder().encode(`${lines.join("\n")}\n`),
  });
  if (out.exitCode !== 0) {
    report(
      `tui: example ${bin.split("/").at(-1)} exited ${out.exitCode}: ${out.stderr.toString().slice(0, STDERR_HEAD)}`
    );
    return null;
  }
  return out.stdout.toString().trimEnd().split("\n");
};

/** `JSON.parse`, or `undefined` for a missing or malformed line. */
const parseLine = (line: string | undefined): unknown => {
  if (line === undefined) {
    return;
  }
  try {
    return JSON.parse(line);
  } catch {
    return;
  }
};

type Lang = CodeBlockLanguage;

/** `[[class|null, text], ...]` per line, same-class neighbours merged. */
const tsTokens = (code: string, lang: Lang) => {
  const lines = isHighlightable(code, lang)
    ? tokenizeLines(code, lang)
    : plainLines(code);
  return JSON.stringify(
    lines.map((l) => {
      const out: [string | null, string][] = [];
      for (const t of l.tokens) {
        const c = t.className ?? null;
        const last = out.at(-1);
        if (last && last[0] === c) {
          last[1] += t.value;
        } else {
          out.push([c, t.value]);
        }
      }
      return out;
    })
  );
};

/** Compare one batch; `show` decides whether a diff may quote the code. */
const compareTokens = (
  bin: string,
  label: string,
  cases: readonly { code: string; lang: Lang; name: string }[],
  show: boolean
) => {
  const rust = pipe(
    bin,
    cases.map((c) => JSON.stringify({ code: c.code, lang: c.lang }))
  );
  if (!rust) {
    return 0;
  }
  cases.forEach((c, i) => {
    const ts = tsTokens(c.code, c.lang);
    if (ts === rust[i]) {
      return;
    }
    const tl = JSON.parse(ts) as unknown[];
    const parsed = parseLine(rust[i]);
    const rl = Array.isArray(parsed) ? parsed : [];
    const at = tl.findIndex(
      (l, k) => JSON.stringify(l) !== JSON.stringify(rl[k])
    );
    const where = `line ${at + 1} of ${tl.length}`;
    report(
      show
        ? `${label} ${c.name} (${c.lang}): ${where} ts=${JSON.stringify(tl[at])} rust=${JSON.stringify(rl[at])}`
        : `${label} ${c.name} (${c.lang}): ${where}`
    );
  });
  return cases.length;
};

/** Tool and prompt bodies of the newest real sessions, decoded as the
 * history view decodes them. Written to WORK only. */
const realBodies = () => {
  const env = { PATH: process.env.PATH ?? "", HOME: process.env.HOME ?? "" };
  const ls = run(
    [
      rustBin,
      "sessions",
      "ls",
      "--agent",
      "claude",
      "--json",
      "--limit",
      String(REAL_SESSIONS),
    ],
    env
  );
  const rows = JSON.parse(ls.stdout || "[]") as { id: string }[];
  const out: { code: string; lang: Lang; name: string }[] = [];
  for (const [n, row] of rows.entries()) {
    const a = run([rustBin, "sessions", "analyze", row.id, "--json"], env);
    if (a.code !== 0) {
      continue;
    }
    const events =
      (JSON.parse(a.stdout) as { events?: unknown[] }).events ?? [];
    for (const [k, e] of events.entries()) {
      const d = decodeBody(e as Parameters<typeof decodeBody>[0]);
      if (d.lang === "plain" || d.content.length > REAL_BODY_MAX) {
        continue;
      }
      out.push({ code: sanitize(d.content), lang: d.lang, name: `s${n}e${k}` });
    }
  }
  mkdirSync(join(WORK, "tui"), { recursive: true });
  writeFileSync(
    join(WORK, "tui/real-bodies.jsonl"),
    out.map((c) => JSON.stringify(c)).join("\n")
  );
  return out;
};

const HOSTILE = [
  `${ESC}[31mred${ESC}[0m`,
  `a${ESC}]8;;http://x${BEL}link${ESC}]8;;${ESC}\\b`,
  `x${ESC}Ky\r\nz\rw\tv\u0000\u007f`,
  "plain text",
];

/** TS and Rust answers for the pure helpers, as JSON lines. */
const viewCases = () => {
  const cases: [Record<string, unknown>, unknown][] = [];
  const add = (req: Record<string, unknown>, ts: unknown) => {
    cases.push([req, ts]);
  };
  for (const n of [
    0, 0.4, 0.5, 2.5, 999, 1000, 12_345, 999_950, 1_234_567, -1500, 1e9,
  ]) {
    add({ fn: "fmt", n }, fmt(n));
    add({ fn: "fmtK", n }, fmtK(n));
  }
  for (const n of [0, 0.0005, 0.0734, 0.0999, 0.1, 0.22, 1, 1.5]) {
    add({ fn: "fmtPct", n }, fmtPct(n));
  }
  for (const n of [0, 512, 1023, 1024, 1638, 1_048_575, 3_145_728]) {
    add({ fn: "fmtBytes", n }, fmtBytes(n));
  }
  for (const iso of [
    "2026-07-09T08:18:00Z",
    "2026-13-01T00:00:00Z",
    "yesterday at noon",
    "",
  ]) {
    add({ fn: "fmtStarted", iso }, fmtStarted(iso));
  }
  for (const s of HOSTILE) {
    add({ fn: "sanitize", s }, sanitize(s));
  }
  for (const [s, max] of [
    ["hello", 5],
    ["hello!", 5],
    ["abc", 0],
    ["abcdef", 1],
  ] as const) {
    add({ fn: "clip", s, max }, clip(s, max));
  }
  for (const height of [20, 24, 30, 36, 44, 48, 60, 80]) {
    add({ fn: "layoutOf", height }, layoutOf(height));
  }
  for (const content of [78, 95, 96, 98, 118, 158, 198]) {
    for (const measured of [true, false]) {
      const c = columnsOf(content, measured);
      add(
        { fn: "columnsOf", content, measured },
        {
          wide: c.wide,
          failWhat: c.fail.what,
          failWhere: c.fail.where,
          timeJob: c.time.job,
          timeShare: c.time.share,
          timeMedian: c.time.median,
        }
      );
    }
  }
  for (const [index, count, visible] of [
    [0, 3, 10],
    [0, 20, 5],
    [10, 20, 5],
    [19, 20, 5],
    [7, 8, 4],
  ] as const) {
    add(
      { fn: "windowSlice", index, count, visible },
      windowSlice(index, count, visible)
    );
  }
  for (const [runs, sec, width] of [
    [0, 0, 11],
    [15, 6060, 11],
    [123_456, 400_000, 11],
    [3, 30.5, 10],
  ] as const) {
    add({ fn: "pairCell", runs, sec, width }, pairCell(runs, sec, width));
  }
  for (const [at, now] of [
    [0, 30_000],
    [0, 120_000],
    [0, 7_200_000],
    [0, 3 * 86_400_000],
  ] as const) {
    add({ fn: "agoOf", at, now }, agoOf(at, now));
  }
  for (const [s, width] of [
    ["- one two three four five six seven eight", 16],
    ["", 20],
    ["word", 2],
  ] as const) {
    add({ fn: "wrap", s, width }, wrap(s, width));
  }
  return cases;
};

const compareViews = (bin: string) => {
  const cases = viewCases();
  const rust = pipe(
    bin,
    cases.map(([req]) => JSON.stringify(req))
  );
  if (!rust) {
    return 0;
  }
  cases.forEach(([req, ts], i) => {
    const want = JSON.stringify(ts);
    const got = parseLine(rust[i]);
    if (got === undefined || JSON.stringify(got) !== want) {
      report(
        `tui view ${JSON.stringify(req)}: ts=${want} rust=${rust[i] ?? "<missing>"}`
      );
    }
  });
  return cases.length;
};

export const runTuiStage = ({
  fixtures,
  real,
}: {
  readonly fixtures: boolean;
  readonly real: boolean;
}) => {
  let flags = 0;
  let tokens = 0;
  let views = 0;
  let realTokens = 0;
  if (fixtures) {
    const home = join(WORK, "tui-home");
    mkdirSync(home, { recursive: true });
    const env = fixtureEnv(home);
    for (const args of PORT_CASES) {
      const label = `tui ${args.join(" ")}`;
      compareRun(
        label,
        run([...TS_CMD, ...args], env),
        run([rustBin, ...args], env)
      );
      flags += 1;
    }
  }
  const hl = exampleBin("peektrace-core", "highlight");
  const vw = exampleBin("peektrace-tui", "views");
  console.log(`tui stage: examples ${hl ?? "-"}, ${vw ?? "-"}`);
  if (fixtures && hl !== null) {
    tokens = compareTokens(hl, "highlight", HIGHLIGHT_CASES, true);
  }
  if (fixtures && vw !== null) {
    views = compareViews(vw);
  }
  if (real && hl !== null) {
    realTokens = compareTokens(hl, "highlight real", realBodies(), false);
  }
  console.log(
    `tui stage: ${flags} flag cases, ${tokens} tokenizer snippets, ${views} view cases, ${realTokens} real bodies`
  );
};
