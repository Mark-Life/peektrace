/** Parity stage for the Rune app (`--ui-only`): the app's pure helpers
 * (`crates/ui/examples/ui_helpers`) against the inspector's own functions
 * on fixture and real headers and analyses, the growth chart geometry
 * against the inspector's SVG, and `serve --ui rune`'s static answers.
 *
 * Analyses come from the Rust CLI (`sessions analyze --json`), which the
 * analyze stage already matches with TS. Real analyses stay in WORK and are
 * deleted with it; reports name sessions and fields, never their text. */
import {
  copyFileSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { join } from "node:path";
import type {
  AnalyzedSession,
  SessionHeader,
} from "../../packages/core/src/services/sessions/schema";
import { fixtureEnv, report, run, rustBin, WORK } from "./parity-lib";
import { exampleBin, pipe } from "./parity-tui";
import { growthAnswer } from "./parity-ui-growth";
import { runRuneServeChecks } from "./parity-ui-serve";
import {
  analysisAnswer,
  type FilterCase,
  hitAnswer,
  listAnswer,
  numAnswer,
} from "./parity-ui-ts";

const FIXTURES = join(import.meta.dir, "../../packages/core/test/fixtures");
const NOW = Date.parse("2026-06-01T10:03:00Z");
/** Newest real sessions per agent. */
const REAL_PER_AGENT = 25;
const AGENTS = ["claude", "codex", "pi", "opencode"] as const;

const SWEEP = [
  0,
  0.5,
  1.25,
  1.35,
  2.5,
  12.5,
  999,
  999.5,
  1000,
  1500,
  1050,
  9999.95,
  999_999,
  999_999.5,
  1_000_000,
  1_000_001,
  1_250_000,
  0.0125,
  0.0995,
  0.1,
  0.469_438,
  -1,
  -1500,

  Number.NaN,
  1e21,
  123_456_789.5,
];
const ZONES: [number, number][] = [
  [0.3, 0.4],
  [0.5, 0.4],
  [0.8, 0.4],
  [0.85, 0.8],
  [0.9, 0.8],
  [0.75, 0.75],
];
const LINES = ["one\ntwo", "x".repeat(170), `${"é".repeat(159)}😀tail`, ""];
const HITS: [number, number, number, number][] = [
  [76, 0, 906, 500],
  [76, 1, 906, 0],
  [76, 5, 0, 300],
  [76, 11, 906, 76],
  [76, 11, 906, 982],
  [76, 11, 906, 2000],
  [0, 11, 100, 45],
  [76, 552, 906, 529.3],
];
const CLICKS: [number, number, boolean][] = [
  [-1, 3, false],
  [-1, 3, true],
  [3, 3, true],
  [2, 3, true],
];

/** Filter cases built from the headers themselves, so each facet hits. */
const filterCases = (hs: readonly SessionHeader[]): FilterCase[] => {
  const h = hs[0];
  const base: FilterCase[] = [
    {},
    { query: "refactor" },
    { query: "SESSION" },
    { agent: "codex" },
  ];
  return h
    ? [
        ...base,
        { project: h.cwd || h.project },
        { branch: h.gitBranch ?? "HEAD" },
        { model: h.model ?? "none", query: h.id.slice(0, 4) },
        { source: h.sourceLabel ?? "" },
      ]
    : base;
};

/** Key-order-free, with lone surrogates read as U+FFFD, as a browser
 * shows them and as the app decodes them. */
const canonical = (v: unknown): unknown => {
  if (typeof v === "string") {
    return v.toWellFormed();
  }
  if (Array.isArray(v)) {
    return v.map(canonical);
  }
  if (v !== null && typeof v === "object") {
    return Object.fromEntries(
      Object.keys(v)
        .sort()
        .map((k) => [k, canonical((v as Record<string, unknown>)[k])])
    );
  }
  return v;
};

/** Compare two answers field by field; `show` lets a diff quote values. */
const compare = (
  label: string,
  tsRaw: unknown,
  rustRaw: unknown,
  show: boolean
) => {
  const ts = canonical(tsRaw);
  const rust = canonical(rustRaw);
  if (
    typeof ts !== "object" ||
    ts === null ||
    typeof rust !== "object" ||
    rust === null
  ) {
    if (JSON.stringify(ts) !== JSON.stringify(rust)) {
      report(
        `${label}: ${show ? `ts=${JSON.stringify(ts)} rust=${JSON.stringify(rust)}` : "differs"}`
      );
    }
    return;
  }
  for (const [k, v] of Object.entries(ts)) {
    const r = (rust as Record<string, unknown>)[k];
    if (JSON.stringify(v) === JSON.stringify(r)) {
      continue;
    }
    const tl = Array.isArray(v) ? v : [v];
    const rl = Array.isArray(r) ? r : [r];
    const at = tl.findIndex(
      (x, i) => JSON.stringify(x) !== JSON.stringify(rl[i])
    );
    const where = at < 0 ? `length ${tl.length} vs ${rl.length}` : `at ${at}`;
    report(
      `${label} ${k} ${where}${show ? `: ts=${JSON.stringify(tl[at]).slice(0, 160)} rust=${JSON.stringify(rl[at]).slice(0, 160)}` : ""}`
    );
  }
};

const listHeaders = (env: Record<string, string>, agent?: string) => {
  const args = ["sessions", "ls", "--json", "--limit", "0"];
  const out = run(
    [rustBin, ...args, ...(agent ? ["--agent", agent] : [])],
    env
  );
  return out.code === 0 ? (JSON.parse(out.stdout) as SessionHeader[]) : [];
};

/** Analyses of `headers` written under `dir`; returns `[label, path]`. */
const analyses = (
  env: Record<string, string>,
  headers: readonly SessionHeader[],
  dir: string
) => {
  mkdirSync(dir, { recursive: true });
  return headers.flatMap((h, i) => {
    const out = run([rustBin, "sessions", "analyze", h.path, "--json"], env);
    if (out.code !== 0) {
      return [];
    }
    const path = join(dir, `${i}.json`);
    writeFileSync(path, out.stdout);
    return [[`${h.agent} ${h.id.slice(0, 8)}`, path] as const];
  });
};

interface Counts {
  analyses: number;
  growth: number;
  helpers: number;
  shifted: number;
}

const checkAnalyses = (
  bin: string,
  files: readonly (readonly [string, string])[],
  show: boolean,
  counts: Counts
) => {
  const reqs = files.flatMap(([, path]) => [
    JSON.stringify({ fn: "analysis", path }),
    JSON.stringify({ fn: "growth", path }),
  ]);
  const rust = pipe(bin, reqs);
  if (!rust) {
    return;
  }
  files.forEach(([label, path], i) => {
    const a = JSON.parse(readFileSync(path, "utf8")) as AnalyzedSession;
    compare(
      `ui analysis ${label}`,
      analysisAnswer(a),
      JSON.parse(rust[2 * i] ?? "null"),
      show
    );
    counts.analyses++;
    const g = JSON.parse(rust[2 * i + 1] ?? "null") as Record<
      string,
      unknown
    > | null;
    const tsG = growthAnswer(a);
    if (g?.shifted === true) {
      counts.shifted++;
      return;
    }
    compare(`ui growth ${label}`, tsG, g, show);
    counts.growth++;
  });
};

const fixtureHome = () => {
  const home = join(WORK, "ui-home");
  const projects = join(home, ".claude", "projects");
  run(
    ["cp", "-R", join(FIXTURES, "sessions", "projects"), join(home, ".claude")],
    { PATH: process.env.PATH ?? "" }
  );
  const other = join(projects, "-Users-demo-other");
  mkdirSync(other, { recursive: true });
  for (const f of readdirSync(join(FIXTURES, "sessions-claude"))) {
    copyFileSync(join(FIXTURES, "sessions-claude", f), join(other, f));
  }
  const codex = join(home, ".codex", "sessions", "2026", "05", "20");
  mkdirSync(codex, { recursive: true });
  for (const f of readdirSync(join(FIXTURES, "sessions-codex"))) {
    copyFileSync(
      join(FIXTURES, "sessions-codex", f),
      join(
        codex,
        `rollout-2026-05-20T09-00-00-${f.replace(".jsonl", "")}.jsonl`
      )
    );
  }
  const pi = join(home, ".pi", "agent", "sessions", "--Users-demo-proj--");
  mkdirSync(pi, { recursive: true });
  readdirSync(join(FIXTURES, "sessions-pi")).forEach((f, i) => {
    copyFileSync(
      join(FIXTURES, "sessions-pi", f),
      join(
        pi,
        `2026-05-19T08-00-0${i}-000Z_019f42f4-0000-7000-8000-00000000000${i}.jsonl`
      )
    );
  });
  return home;
};

export const runUiStage = async ({
  fixtures,
  real,
}: {
  fixtures: boolean;
  real: boolean;
}) => {
  console.log("suite ui");
  const bin = exampleBin("peektrace-ui", "ui_helpers");
  if (!bin) {
    return;
  }
  const counts: Counts = { analyses: 0, growth: 0, helpers: 0, shifted: 0 };
  const sets: {
    env: Record<string, string>;
    headers: SessionHeader[];
    label: string;
    show: boolean;
  }[] = [];
  if (fixtures) {
    const env = fixtureEnv(fixtureHome());
    sets.push({
      env,
      headers: listHeaders(env),
      label: "fixtures",
      show: true,
    });
  }
  if (real) {
    const env = {
      PATH: process.env.PATH ?? "",
      HOME: process.env.HOME ?? "",
      TZ: "UTC",
      PEEKTRACE_NO_TELEMETRY: "1",
    };
    const headers = AGENTS.flatMap((a) =>
      listHeaders(env, a).slice(0, REAL_PER_AGENT)
    );
    sets.push({ env, headers, label: "real", show: false });
  }
  const numReq = { fn: "num", xs: SWEEP, zones: ZONES, lines: LINES };
  const hitReq = { fn: "hit", snaps: HITS, clicks: CLICKS };
  const fixed = pipe(bin, [JSON.stringify(numReq), JSON.stringify(hitReq)]);
  if (fixed) {
    compare(
      "ui num",
      numAnswer(SWEEP, ZONES, LINES),
      JSON.parse(fixed[0] ?? "null"),
      true
    );
    compare(
      "ui hit",
      hitAnswer(HITS, CLICKS),
      JSON.parse(fixed[1] ?? "null"),
      true
    );
    counts.helpers +=
      SWEEP.length * 4 +
      ZONES.length +
      LINES.length +
      HITS.length +
      CLICKS.length;
  }
  for (const set of sets) {
    const cases = filterCases(set.headers);
    const listReq = {
      fn: "list",
      headers: set.headers,
      now: NOW,
      filters: cases,
    };
    const out = pipe(bin, [JSON.stringify(listReq)]);
    if (out) {
      compare(
        `ui list ${set.label}`,
        listAnswer(set.headers, NOW, cases),
        JSON.parse(out[0] ?? "null"),
        set.show
      );
      counts.helpers += set.headers.length * (8 + cases.length * 2);
    }
    const files = analyses(set.env, set.headers, join(WORK, `ui-${set.label}`));
    checkAnalyses(bin, files, set.show, counts);
  }
  const statics = await runRuneServeChecks();
  console.log(
    `ui stage: helpers ${counts.helpers} cases, analyses ${counts.analyses}, growth ${counts.growth} sessions (${counts.shifted} interleaved, known), serve --ui rune ${statics} checks`
  );
};
