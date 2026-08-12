/** In-process round trip for the `stats.*` procedures.
 *
 * A purpose-built Claude transcript in a temp root carries one row of each shape
 * the tables rank: a command zsh aborted on an unquoted separator, and a
 * typecheck that printed `error TS####` through a pipe and still exited 0. The
 * clock is pinned, so two reports over the same corpus are compared byte for
 * byte — the second one also reads its facts back from the shard cache, which is
 * what makes the comparison worth running.
 */
import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import {
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { BunFileSystem } from "@effect/platform-bun";
import {
  AgentRegistry,
  type AgentRegistryShape,
  type AgentRoots,
} from "@workspace/core";
import { makeHandlersLayer, makeInProcessClient } from "@workspace/rpc";
import { Clock, Effect, Layer } from "effect";

const SLUG = "-Users-demo-stats";
const SID = "22222222-2222-4222-8222-222222222222";

/** A secret in a command body: it must not reach any string in the report. */
const SECRET = "sk-ant-api03-AAAABBBBCCCCDDDDEEEEFFFFGGGGHHHHIIIIJJJJ1234";

const ABORTED = "bun test 2>&1 | tail -5; echo ===; ls packages";
const SILENT = "bun run typecheck | tail -3";

const transcript = [
  {
    type: "user",
    cwd: "/Users/demo/stats",
    gitBranch: "main",
    version: "1.2.3",
    timestamp: "2026-06-01T10:00:00.000Z",
    message: { content: "run the checks" },
  },
  {
    type: "assistant",
    requestId: "req-1",
    timestamp: "2026-06-01T10:00:05.000Z",
    message: {
      model: "claude-opus-4",
      usage: {
        input_tokens: 100,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 200,
        output_tokens: 50,
      },
      content: [
        {
          type: "tool_use",
          id: "tool-1",
          name: "Bash",
          input: { command: ABORTED },
        },
      ],
    },
  },
  {
    type: "user",
    timestamp: "2026-06-01T10:00:09.000Z",
    message: {
      content: [
        {
          type: "tool_result",
          tool_use_id: "tool-1",
          is_error: true,
          content: "Exit code 1\n(eval):1: = not found",
        },
      ],
    },
  },
  {
    type: "assistant",
    requestId: "req-2",
    timestamp: "2026-06-01T10:00:12.000Z",
    message: {
      model: "claude-opus-4",
      usage: {
        input_tokens: 60,
        cache_read_input_tokens: 1000,
        cache_creation_input_tokens: 0,
        output_tokens: 40,
      },
      content: [
        {
          type: "tool_use",
          id: "tool-2",
          name: "Bash",
          input: { command: `EXA_API_KEY=${SECRET} ${SILENT}` },
        },
      ],
    },
  },
  {
    type: "user",
    timestamp: "2026-06-01T10:00:20.000Z",
    message: {
      content: [
        {
          type: "tool_result",
          tool_use_id: "tool-2",
          content: "src/auth.ts(12,3): error TS2339: Property 'x' is missing.",
        },
      ],
    },
  },
]
  .map((line) => JSON.stringify(line))
  .join("\n");

let root = "";

/** Stub AgentRegistry pointing Claude resolution at the temp root. */
const makeAgents = (base: string): AgentRegistryShape => {
  const roots = (id: AgentRoots["id"]): AgentRoots => ({
    id,
    home: join(base, "home"),
    layout: id === "claude" ? "claude-projects" : "none",
    projectsRoot: base,
    supported: id === "claude",
  });
  return {
    encodeSlug: (p) => p.replace(/[/.]/g, "-"),
    allowedRoots: [base, tmpdir()],
    roots,
    gitRoot: (cwd) => Effect.succeed(cwd),
    projectsRoot: () => Effect.succeed(base),
    sessionsGlob: () => Effect.succeed(join(base, "**", "*.jsonl")),
    memoryDir: ({ slug }) => Effect.succeed(join(base, slug, "memory")),
    listProjectSlugs: () => Effect.succeed([SLUG]),
    listSessionFiles: (id) =>
      Effect.succeed(
        id === "claude"
          ? [{ path: join(base, SLUG, `${SID}.jsonl`), id: SID, slug: SLUG }]
          : []
      ),
    loadTranscript: ({ ref }) =>
      Effect.sync(() => {
        const text = readFileSync(ref.path, "utf8");
        return {
          text,
          sizeBytes: Buffer.byteLength(text, "utf8"),
          mtimeMs: 1,
        };
      }),
  };
};

const FIXED_MS = Date.UTC(2026, 6, 1, 12, 0, 0);
const NANOS_PER_MS = 1_000_000n;

/** A clock frozen at one instant, so `generatedAt` and the window cannot drift. */
const fixedClock: Clock.Clock = {
  [Clock.ClockTypeId]: Clock.ClockTypeId,
  currentTimeMillis: Effect.succeed(FIXED_MS),
  currentTimeNanos: Effect.succeed(BigInt(FIXED_MS) * NANOS_PER_MS),
  unsafeCurrentTimeMillis: () => FIXED_MS,
  unsafeCurrentTimeNanos: () => BigInt(FIXED_MS) * NANOS_PER_MS,
  sleep: () => Effect.void,
};

/** Build the in-process client under the pinned clock and run `use`. */
const withClient = <A, E>(
  use: (
    client: Effect.Effect.Success<ReturnType<typeof makeInProcessClient>>
  ) => Effect.Effect<A, E, never>
): Promise<A> =>
  Effect.runPromise(
    Effect.gen(function* () {
      const client = yield* makeInProcessClient();
      return yield* use(client);
    }).pipe(
      Effect.scoped,
      Effect.provide(
        makeHandlersLayer({
          agents: Layer.succeed(AgentRegistry, makeAgents(root)),
          fileSystem: BunFileSystem.layer,
        })
      ),
      Effect.withClock(fixedClock)
    ) as Effect.Effect<A, E, never>
  );

/** A window wide enough to hold the fixture, whatever the pinned date. */
const WIDE = 3650;

const prevPtDir = process.env.PEEKTRACE_DIR;

beforeAll(() => {
  root = mkdtempSync(join(tmpdir(), "peektrace-stats-"));
  mkdirSync(join(root, SLUG), { recursive: true });
  writeFileSync(join(root, SLUG, `${SID}.jsonl`), `${transcript}\n`);
  process.env.PEEKTRACE_DIR = join(root, "pt");
  mkdirSync(process.env.PEEKTRACE_DIR, { recursive: true });
});
afterAll(() => {
  if (prevPtDir === undefined) {
    delete process.env.PEEKTRACE_DIR;
  } else {
    process.env.PEEKTRACE_DIR = prevPtDir;
  }
  rmSync(root, { recursive: true, force: true });
});

describe("stats.report", () => {
  test("returns the versioned envelope over the fixture corpus", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const report = yield* client.stats.report({ days: WIDE });
        expect(report.schemaVersion).toBe("peektrace-stats/v1");
        expect(report.generatedAt).toBe(FIXED_MS);
        expect(report.window.days).toBe(WIDE);
        expect(report.corpus.transcripts).toBe(1);
        expect(report.corpus.bashCalls).toBe(2);
        expect(report.caveats.length).toBeGreaterThan(0);
      })
    ));

  test("ranks the aborted separator and the silent typecheck", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const report = yield* client.stats.report({ days: WIDE });
        const rule = report.fails.find(
          (row) => row.detector === "zsh-equals-abort"
        );
        expect(rule?.rows).toBe(1);
        expect(rule?.sessions).toBe(1);
        expect(rule?.note).toContain('echo "==="');

        const silent = report.fails.find((row) => row.key === "scan:tsc");
        expect(silent?.rows).toBe(1);
        expect(silent?.flagged).toBe(0);
        expect(silent?.detector).toBe("piped-exit-code");
      })
    ));

  test("every ranking is capped and every truncation is stated", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const report = yield* client.stats.report({ days: WIDE, limit: 1 });
        expect(report.fails.length).toBe(1);
        expect(report.caveats.some((note) => note.includes("not shown"))).toBe(
          true
        );
      })
    ));

  test("no command secret survives into the report", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const report = yield* client.stats.report({ days: WIDE });
        expect(JSON.stringify(report)).not.toContain(SECRET);
      })
    ));

  test("two runs over one corpus print byte-identical JSON", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const first = yield* client.stats.report({ days: WIDE });
        const second = yield* client.stats.report({ days: WIDE });
        expect(JSON.stringify(second, null, 2)).toBe(
          JSON.stringify(first, null, 2)
        );
      })
    ));
});

describe("stats.drill", () => {
  test("expands the rows behind a fail key", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const drill = yield* client.stats.drill({
          days: WIDE,
          table: "fail",
          key: "detect:zsh-equals-abort",
        });
        expect(drill.schemaVersion).toBe("peektrace-stats/v1");
        expect(drill.total).toBe(1);
        expect(drill.hits[0]?.sid).toBe(SID);
        expect(drill.hits[0]?.failed).toBe(true);
      })
    ));

  test("a page states where it starts and keeps the true total", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const args = {
          days: WIDE,
          key: "detect:zsh-equals-abort",
          table: "fail",
        } as const;
        const first = yield* client.stats.drill({ ...args, pageSize: 1 });
        expect(first.offset).toBe(0);
        expect(first.hits).toHaveLength(1);
        const past = yield* client.stats.drill({
          ...args,
          offset: 1,
          pageSize: 1,
        });
        expect(past.offset).toBe(1);
        expect(past.hits).toHaveLength(0);
        expect(past.total).toBe(first.total);
      })
    ));

  test("an unknown key is a typed wire failure, not an empty table", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const result = yield* Effect.either(
          client.stats.drill({ days: WIDE, table: "fail", key: "nope" })
        );
        expect(result._tag).toBe("Left");
        if (result._tag === "Left") {
          expect(result.left._tag).toBe("StatsRowNotFoundError");
        }
      })
    ));
});

describe("stats.markers", () => {
  test("marks the same calls the tables rank, keyed on the tool-use id", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const marked = yield* client.stats.markers({ sid: SID });
        expect(marked.schemaVersion).toBe("peektrace-stats/v1");
        expect(marked.sid).toBe(SID);
        const ids = marked.markers.map((m) => [m.toolUseId, m.detector]);
        expect(ids).toContainEqual(["tool-1", "zsh-equals-abort"]);
        expect(ids).toContainEqual(["tool-2", "piped-exit-code"]);
      })
    ));

  test("an unknown session marks nothing", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const marked = yield* client.stats.markers({ sid: "no-such-session" });
        expect(marked.markers).toHaveLength(0);
      })
    ));
});

describe("stats.refresh", () => {
  test("re-extracts every transcript under --force", () =>
    withClient((client) =>
      Effect.gen(function* () {
        const summary = yield* client.stats.refresh({ force: true });
        expect(summary.transcripts).toBe(1);
        expect(summary.extracted).toBe(1);
        expect(summary.cached).toBe(0);
        expect(summary.facts).toBeGreaterThan(0);
        const warm = yield* client.stats.refresh({});
        expect(warm.cached).toBe(1);
        expect(warm.extracted).toBe(0);
      })
    ));

  test("reading one session's markers leaves the rest of the cache alone", () =>
    withClient((client) =>
      Effect.gen(function* () {
        yield* client.stats.refresh({});
        yield* client.stats.markers({ sid: "no-such-session" });
        const after = yield* client.stats.refresh({});
        expect(after.cached).toBe(1);
        expect(after.extracted).toBe(0);
      })
    ));
});
