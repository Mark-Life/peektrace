import { describe, expect, test } from "bun:test";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { BunFileSystem } from "@effect/platform-bun";
import { Effect, Layer } from "effect";
import {
  type AgentId,
  AgentRegistry,
  type AgentRegistryShape,
  type AgentRoots,
} from "../../src/services/agents";
import {
  SessionsService,
  SessionsServiceLive,
} from "../../src/services/sessions/service";

const FIXTURE_DIR = join(import.meta.dir, "../fixtures/sessions");
const PROJECTS_ROOT = join(FIXTURE_DIR, "projects");
const SLUG = "-Users-demo-proj";
const SESSION_ID = "11111111-1111-4111-8111-111111111111";

const encodeSlug = (p: string) => p.replace(/[/.]/g, "-");

const stubRoots = (id: AgentId): AgentRoots => ({
  id,
  home: join(FIXTURE_DIR, "home"),
  layout: id === "claude" ? "claude-projects" : "none",
  projectsRoot: PROJECTS_ROOT,
  supported: id === "claude",
});

const agentStub: AgentRegistryShape = {
  allowedRoots: [PROJECTS_ROOT],
  encodeSlug,
  gitRoot: (cwd) => Effect.succeed(cwd),
  listProjectSlugs: () => Effect.succeed([SLUG]),
  listSessionFiles: (id) =>
    Effect.succeed(
      id === "claude"
        ? [
            {
              path: join(PROJECTS_ROOT, SLUG, `${SESSION_ID}.jsonl`),
              id: SESSION_ID,
              slug: SLUG,
            },
          ]
        : []
    ),
  loadTranscript: ({ ref }) =>
    Effect.sync(() => {
      const text = readFileSync(ref.path, "utf8");
      return {
        text,
        sizeBytes: Buffer.byteLength(text, "utf8"),
        mtimeMs: 0,
      };
    }),
  memoryDir: ({ slug }) => Effect.succeed(join(PROJECTS_ROOT, slug, "memory")),
  projectsRoot: () => Effect.succeed(PROJECTS_ROOT),
  roots: stubRoots,
  sessionsGlob: () => Effect.succeed(join(PROJECTS_ROOT, "**", "*.jsonl")),
};

const AgentRegistryStub = Layer.succeed(AgentRegistry, agentStub);

const layer = SessionsServiceLive.pipe(
  Layer.provide(AgentRegistryStub),
  Layer.provide(BunFileSystem.layer)
);

const run = <A, E>(program: Effect.Effect<A, E, SessionsService>) =>
  Effect.runPromise(
    program.pipe(Effect.provide(layer)) as Effect.Effect<A, E, never>
  );

const golden = JSON.parse(
  readFileSync(join(FIXTURE_DIR, "golden.json"), "utf8")
) as {
  peakContextTokens: number;
  budget: Array<{ key: string; tokens: number }>;
};

describe("SessionsService.list (lazy headers)", () => {
  test("returns one header per transcript with correct fields", () =>
    run(
      Effect.gen(function* () {
        const svc = yield* SessionsService;
        const headers = yield* svc.list();
        expect(headers).toHaveLength(1);
        const h = headers[0];
        expect(h.id).toBe(SESSION_ID);
        expect(h.project).toBe(SLUG);
        expect(h.agent).toBe("claude");
        expect(h.cwd).toBe("/Users/demo/proj");
        expect(h.gitBranch).toBe("main");
        expect(h.model).toBe("claude-opus-4");
        expect(h.title).toBe("Refactor auth module");
        expect(h.messageCount).toBe(10);
        expect(h.sizeBytes).toBeGreaterThan(0);
        expect(h.startedAt).toBe("2026-06-01T10:00:00.000Z");
      })
    ));
});

describe("SessionsService.parse (subagent folding)", () => {
  test("folds the subagent transcript into the parent", () =>
    run(
      Effect.gen(function* () {
        const svc = yield* SessionsService;
        const parsed = yield* svc.parse({ id: SESSION_ID });
        expect(parsed.subagents).toHaveLength(1);
        const sub = parsed.subagents[0];
        expect(sub.id).toBe("agent-1");
        expect(sub.agentType).toBe("investigator");
        expect(sub.turns).toBe(2);
        expect(sub.peakContextTokens).toBe(700);
        // Main turns exclude the sidechain line (req-sub @ 5010 ctx).
        expect(parsed.turns).toHaveLength(3);
        expect(Math.max(...parsed.turns.map((t) => t.contextTokens))).toBe(
          1150
        );
      })
    ));

  test("redaction is on by default and masks the planted secret", () =>
    run(
      Effect.gen(function* () {
        const svc = yield* SessionsService;
        const parsed = yield* svc.parse({ id: SESSION_ID });
        const toolResult = parsed.events.find((e) => e.kind === "tool-result");
        expect(toolResult?.body).toContain("[REDACTED:anthropic-key]");
        expect(toolResult?.body).not.toContain("sk-ant-api03");
      })
    ));

  test("redact:false leaves the transcript verbatim", () =>
    run(
      Effect.gen(function* () {
        const svc = yield* SessionsService;
        const parsed = yield* svc.parse({ id: SESSION_ID, redact: false });
        const toolResult = parsed.events.find((e) => e.kind === "tool-result");
        expect(toolResult?.body).toContain("sk-ant-api03");
      })
    ));
});

describe("SessionsService.analyze", () => {
  test("reproduces golden peak + budget", () =>
    run(
      Effect.gen(function* () {
        const svc = yield* SessionsService;
        const a = yield* svc.analyze({ id: SESSION_ID });
        expect(a.peakContextTokens).toBe(golden.peakContextTokens);
        expect(a.budget.map((b) => ({ key: b.key, tokens: b.tokens }))).toEqual(
          golden.budget
        );
      })
    ));

  test("resolves a short id prefix to the full session", () =>
    run(
      Effect.gen(function* () {
        const svc = yield* SessionsService;
        const a = yield* svc.analyze({ id: SESSION_ID.slice(0, 8) });
        expect(a.sessionId).toBe(SESSION_ID);
      })
    ));

  test("missing session fails with SessionNotFoundError", () =>
    run(
      Effect.gen(function* () {
        const svc = yield* SessionsService;
        const result = yield* Effect.either(
          svc.analyze({ id: "does-not-exist" })
        );
        expect(result._tag).toBe("Left");
        if (result._tag === "Left") {
          expect(result.left._tag).toBe("SessionNotFoundError");
        }
      })
    ));
});
