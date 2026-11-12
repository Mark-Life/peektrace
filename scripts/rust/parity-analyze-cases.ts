/**
 * Cases of the analyze/export parity stage: every name of each listed
 * session, Claude subagents via `--parent`, and fixed target and flag
 * errors.
 */
import { basename, join } from "node:path";
import { runAsync, rustBin } from "./parity-lib";

const SUBAGENTS_PER_SESSION = 3;
const UUID = /[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/i;

export interface Case {
  readonly args: readonly string[];
  /** File the command writes, compared too. */
  readonly out?: string;
  /** Transcript the case reads, for the live check. */
  readonly owner?: string;
}

export interface Row {
  readonly agent: string;
  readonly id: string;
  readonly path: string;
  readonly sizeBytes: number;
}

/** `id` up to its first `-`, when no other listed id starts with it. */
const shortIdOf = (id: string, ids: readonly string[]) => {
  const short = id.split("-")[0] ?? id;
  return short !== id && ids.filter((x) => x.startsWith(short)).length === 1
    ? short
    : undefined;
};

export const analyze = (t: string, ...rest: string[]) => [
  "sessions",
  "analyze",
  t,
  ...rest,
];
export const exportOf = (t: string, ...rest: string[]) => [
  "sessions",
  "export",
  t,
  ...rest,
];

/** Every mode for a full id; a smaller set for its other names. An id that
 * starts with `-` reads as a flag in both CLIs, so only its path is used. */
export const targetCases = (row: Row, ids: readonly string[]): Case[] => {
  const byPath: Case[] = [
    { args: analyze(row.path, "--json") },
    { args: exportOf(row.path, "--format", "html") },
  ];
  if (row.id.startsWith("-")) {
    return byPath.map((c) => ({ ...c, owner: row.path }));
  }
  const all: Case[] = [
    { args: analyze(row.id, "--json") },
    { args: analyze(row.id) },
    { args: ["--pretty", ...analyze(row.id)] },
    { args: exportOf(row.id, "--format", "json") },
    { args: exportOf(row.id, "--format", "md") },
    { args: exportOf(row.id, "--format", "html") },
    { args: exportOf(row.id, "--no-redact", "--format", "json") },
    { args: exportOf(row.id) },
  ];
  const short = shortIdOf(row.id, ids);
  return [
    ...all,
    ...(short
      ? [
          { args: analyze(short, "--json") },
          { args: exportOf(short, "--format", "md") },
        ]
      : []),
    ...byPath,
  ].map((c) => ({ ...c, owner: row.path }));
};

/** `--parent` cases for the first subagents of a Claude session. */
export const subagentCases = async (row: Row, env: Record<string, string>) => {
  if (row.agent !== "claude") {
    return [];
  }
  const out = await runAsync(
    [rustBin, "sessions", "analyze", row.id, "--json"],
    env
  );
  let subs: { id: string }[] = [];
  try {
    subs = (JSON.parse(out.stdout) as { subagents: { id: string }[] })
      .subagents;
  } catch {
    return [];
  }
  return subs.slice(0, SUBAGENTS_PER_SESSION).flatMap((s) => [
    { args: analyze(s.id, "--parent", row.id, "--json"), owner: row.path },
    {
      args: exportOf(s.id, "--parent", row.id, "--format", "md"),
      owner: row.path,
    },
  ]);
};

/** Target, flag and write errors, run once per fixture suite. */
export const fixedCases = (rows: readonly Row[], work: string): Case[] => {
  const first = rows[0]?.id ?? "x";
  const ids = rows.map((r) => r.id);
  const shared = ids
    .map((id) => id.slice(0, 1))
    .find((p) => ids.filter((x) => x.startsWith(p)).length > 1);
  const ofAgent = (agent: string) => rows.find((r) => r.agent === agent);
  const twice = rows.find(
    (r) => rows.filter((x) => x.id === r.id && x.agent !== r.agent).length > 0
  );
  const uuidPath = rows.find(
    (r) => r.agent === "codex" && UUID.test(basename(r.path))
  );
  return [
    { args: ["sessions", "analyze", "--latest"] },
    { args: ["sessions", "analyze", "--latest", "--json"] },
    { args: ["sessions", "analyze", "--cwd", "/"] },
    { args: ["sessions", "analyze", "--cwd", "/Users/demo/proj"] },
    { args: ["sessions", "analyze", "--cwd", "/nope"] },
    { args: ["sessions", "analyze"] },
    { args: ["sessions", "analyze", "x", "--latest"] },
    { args: ["sessions", "analyze", "--parent", "p"] },
    { args: ["sessions", "analyze", "--cwd"] },
    { args: ["sessions", "export", "--out"] },
    { args: ["sessions", "analyze", "-x"] },
    { args: ["sessions", "analyze", "zzz"] },
    { args: ["sessions", "analyze", "zzz.jsonl"] },
    ...(shared ? [{ args: ["sessions", "analyze", shared] }] : []),
    { args: ["sessions", "analyze", first, "--parent", "zz"] },
    { args: ["sessions", "analyze", "agent-1", "--parent", first] },
    { args: ["sessions", "export", first, "--format", "pdf"] },
    { args: ["sessions", "export", "--latest", "--format", "json"] },
    {
      args: ["sessions", "export", first, "--out", join(work, "x.HTM")],
      out: join(work, "x.HTM"),
    },
    {
      args: [
        "sessions",
        "export",
        first,
        "--out",
        join(work, "x.json"),
        "--no-redact",
      ],
      out: join(work, "x.json"),
    },
    {
      args: [
        "sessions",
        "export",
        first,
        "--out",
        join(work, "missing", "x.md"),
      ],
    },
    { args: ["sessions", "export", first, "--out", work] },
    { args: ["sessions", "export", first, "--out", "-"] },
    { args: ["--json", "sessions", "export", first] },
    { args: ["sessions", "analyze", "rollout"] },
    { args: ["sessions", "analyze", "--cwd", "/tmp/demo", "--json"] },
    { args: ["sessions", "analyze", "--cwd", "/repo"] },
    { args: ["sessions", "export", "--latest", "--format", "md"] },
    ...(twice ? [{ args: ["sessions", "analyze", twice.id, "--json"] }] : []),
    ...(uuidPath ? [{ args: ["sessions", "analyze", uuidPath.path] }] : []),
    ...["codex", "pi"].flatMap((agent) => {
      const row = ofAgent(agent);
      return row
        ? [{ args: ["sessions", "analyze", "x", "--parent", row.id] }]
        : [];
    }),
  ];
};
