/** Read errors the sessions service surfaces instead of hiding, and the
 * header message count. Synthetic fixtures only, in a fresh temp dir. */
import { afterAll, describe, expect, test } from "bun:test";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { BunFileSystem } from "@effect/platform-bun";
import { Effect, Layer } from "effect";
import {
  type AgentId,
  AgentRegistry,
  type AgentRegistryShape,
  type AgentRoots,
  TranscriptReadError,
} from "../../src/services/agents";
import { buildHeader } from "../../src/services/sessions/header";
import { UNREADABLE_ROOT_ATTR } from "../../src/services/sessions/listing";
import { parseJsonl, scanJsonl } from "../../src/services/sessions/parse";
import {
  SessionsService,
  SessionsServiceLive,
} from "../../src/services/sessions/service";

const ROOT = mkdtempSync(join(tmpdir(), "pt-swallowed-"));
const PROJECTS = join(ROOT, "projects");
const SLUG = "-demo-proj";
const GOOD = "22222222-2222-4222-8222-222222222222";
const MISSING = "33333333-3333-4333-8333-333333333333";
const EMPTY = "44444444-4444-4444-8444-444444444444";

const line = (o: unknown) => JSON.stringify(o);
const TRANSCRIPT = [
  line({
    type: "user",
    cwd: "/demo",
    timestamp: "2026-01-01T00:00:00.000Z",
    message: { content: "hi" },
  }),
  line({
    type: "assistant",
    requestId: "r1",
    timestamp: "2026-01-01T00:00:01.000Z",
    message: {
      model: "claude-x",
      usage: { input_tokens: 10, output_tokens: 5 },
      content: [{ type: "text", text: "a" }],
    },
  }),
  line({ type: "attachment", timestamp: "2026-01-01T00:00:02.000Z" }),
  line({ type: "file-history-snapshot" }),
  line({ type: "ai-title", aiTitle: "Demo" }),
].join("\n");

const projectDir = join(PROJECTS, SLUG);
const subDir = join(projectDir, GOOD, "subagents");
mkdirSync(subDir, { recursive: true });
writeFileSync(join(projectDir, `${GOOD}.jsonl`), TRANSCRIPT);
writeFileSync(join(projectDir, `${EMPTY}.jsonl`), "");
const lockedSub = join(subDir, "agent-locked.jsonl");
writeFileSync(lockedSub, TRANSCRIPT);
chmodSync(lockedSub, 0o000);

afterAll(() => {
  chmodSync(lockedSub, 0o600);
  rmSync(ROOT, { recursive: true, force: true });
});

const roots = (id: AgentId): AgentRoots => ({
  id,
  home: ROOT,
  layout: id === "claude" ? "claude-projects" : "none",
  projectsRoot: PROJECTS,
  supported: id === "claude",
});

const stub = (over: Partial<AgentRegistryShape> = {}): AgentRegistryShape => ({
  allowedRoots: [PROJECTS],
  encodeSlug: (p) => p.replace(/[/.]/g, "-"),
  gitRoot: (cwd) => Effect.succeed(cwd),
  listProjectSlugs: () => Effect.succeed([SLUG]),
  listSessionFiles: (id) =>
    Effect.succeed(
      id === "claude"
        ? [GOOD, MISSING, EMPTY].map((sid) => ({
            path: join(projectDir, `${sid}.jsonl`),
            id: sid,
            slug: SLUG,
          }))
        : []
    ),
  // Fails for MISSING by construction, so the read error does not depend on
  // file permissions (which root ignores).
  loadTranscript: ({ ref }) =>
    Effect.try({
      try: () => {
        if (ref.id === MISSING) {
          throw new Error("ENOENT");
        }
        const text = readFileSync(ref.path, "utf8");
        return { text, sizeBytes: Buffer.byteLength(text), mtimeMs: 0 };
      },
      catch: (e) =>
        new TranscriptReadError({ path: ref.path, reason: String(e) }),
    }),
  memoryDir: () => Effect.succeed(join(projectDir, "memory")),
  projectsRoot: () => Effect.succeed(PROJECTS),
  roots,
  sessionsGlob: () => Effect.succeed(join(PROJECTS, "**", "*.jsonl")),
  ...over,
});

const run = <A, E>(
  agents: AgentRegistryShape,
  program: Effect.Effect<A, E, SessionsService>
) =>
  Effect.runPromise(
    program.pipe(
      Effect.provide(
        SessionsServiceLive.pipe(
          Layer.provide(Layer.succeed(AgentRegistry, agents)),
          Layer.provide(BunFileSystem.layer)
        )
      )
    ) as Effect.Effect<A, E, never>
  );

const isRoot = process.getuid?.() === 0;

describe("read errors are surfaced", () => {
  test("list: an unreadable transcript is reported, an empty one skipped", async () => {
    const { headers, unreadable } = await run(
      stub(),
      Effect.flatMap(SessionsService, (s) => s.list())
    );
    expect(headers.map((h) => h.id)).toEqual([GOOD]);
    expect(unreadable).toEqual([
      {
        agent: "claude",
        project: SLUG,
        path: join(projectDir, `${MISSING}.jsonl`),
      },
    ]);
  });

  test("list: the unreadable count lands on the root span", async () => {
    const attrs = await run(
      stub(),
      Effect.gen(function* () {
        const s = yield* SessionsService;
        yield* Effect.withSpan(s.list(), "inner");
        const span = yield* Effect.currentSpan;
        return span.attributes;
      }).pipe(
        Effect.withSpan("root", { attributes: { "peektrace.root": true } }),
        Effect.withSpan("outer")
      )
    );
    expect(attrs.get(UNREADABLE_ROOT_ATTR)).toBe(1);
  });

  test("list: an unreadable row with no slug keeps an empty project", async () => {
    const { unreadable } = await run(
      stub({
        listSessionFiles: (id) =>
          Effect.succeed(
            id === "claude"
              ? [
                  {
                    path: join(projectDir, `${MISSING}.jsonl`),
                    id: MISSING,
                    slug: "",
                  },
                ]
              : []
          ),
      }),
      Effect.flatMap(SessionsService, (s) => s.list())
    );
    expect(unreadable.map((u) => u.project)).toEqual([""]);
  });

  test.skipIf(isRoot)(
    "parseSubagent: an unreadable subagent is flagged",
    async () => {
      const parsed = await run(
        stub(),
        Effect.flatMap(SessionsService, (s) => s.parse({ id: GOOD }))
      );
      const locked = parsed.subagents.find((a) => a.id === "agent-locked");
      expect(locked).toMatchObject({
        turns: 0,
        peakContextTokens: 0,
        unreadable: true,
      });
    }
  );

  test("foldClaudeSubagents: a resolve failure keeps the session", async () => {
    const parsed = await run(
      stub({
        listProjectSlugs: () => Effect.fail(new Error("eacces") as never),
      }),
      Effect.flatMap(SessionsService, (s) => s.parse({ id: GOOD }))
    );
    expect(parsed.sessionId).toBe(GOOD);
    expect(parsed.subagents).toHaveLength(0);
  });
});

describe("scanJsonl", () => {
  const text = [
    line({ type: "user" }),
    "{not json",
    line({ type: "assistant" }),
    '{"type":"us',
  ].join("\n");

  test("counts malformed lines except a partial last line", () => {
    const { rows, malformed } = scanJsonl(text);
    expect(rows).toHaveLength(2);
    expect(malformed).toBe(1);
  });

  test("parseJsonl keeps the same rows", () => {
    expect(parseJsonl(text)).toEqual(scanJsonl(text).rows);
  });

  test("a clean transcript has no malformed lines", () => {
    expect(scanJsonl(TRANSCRIPT).malformed).toBe(0);
  });
});

const header = (text: string) =>
  buildHeader({
    text,
    id: GOOD,
    slug: SLUG,
    path: "/x.jsonl",
    sizeBytes: 1,
    mtimeMs: 0,
  });

describe("header messageCount", () => {
  test("counts user and assistant messages, not lines", () => {
    const h = header(TRANSCRIPT);
    expect(h.messageCount).toBe(2);
    expect(h.malformedLines).toBeUndefined();
  });

  test("assistant lines that share a requestId count once", () => {
    const block = (requestId: string, text: string) =>
      line({
        type: "assistant",
        requestId,
        message: { content: [{ type: "text", text }] },
      });
    const h = header(
      [
        line({ type: "user", message: { content: "q" } }),
        block("r1", "a"),
        block("r1", "b"),
        block("r2", "c"),
      ].join("\n")
    );
    expect(h.messageCount).toBe(3);
  });

  test("user lines that only carry tool output do not count", () => {
    const toolResult = (id: string) =>
      line({
        type: "user",
        message: {
          content: [{ type: "tool_result", tool_use_id: id, content: "ok" }],
        },
      });
    const ask = (requestId: string) =>
      line({
        type: "assistant",
        requestId,
        message: { content: [{ type: "tool_use", id: requestId }] },
      });
    const h = header(
      [
        line({ type: "user", message: { content: "q" } }),
        ask("r1"),
        toolResult("r1"),
        ask("r2"),
        toolResult("r2"),
        ask("r3"),
      ].join("\n")
    );
    expect(h.messageCount).toBe(4);
  });

  test("malformed mid-file lines are counted on the header", () => {
    const h = header([TRANSCRIPT, "{bad", line({ type: "user" })].join("\n"));
    expect(h.messageCount).toBe(3);
    expect(h.malformedLines).toBe(1);
  });
});
