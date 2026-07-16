import { describe, expect, test } from "bun:test";
import { BunFileSystem } from "@effect/platform-bun";
import { Effect, Layer } from "effect";
import {
  AGENT_IDS,
  AgentRegistry,
  AgentRegistryLive,
} from "../src/services/agents";

const layer = AgentRegistryLive.pipe(Layer.provide(BunFileSystem.layer));

const run = <A, E>(program: Effect.Effect<A, E, AgentRegistry>) =>
  Effect.runPromise(
    program.pipe(Effect.provide(layer)) as Effect.Effect<A, E, never>
  );

describe("AgentRegistry slug encoding", () => {
  test("round-trips known cwd paths", () =>
    run(
      Effect.gen(function* () {
        const reg = yield* AgentRegistry;
        expect(reg.encodeSlug("/Users/x/Code/y.z")).toBe("-Users-x-Code-y-z");
        expect(reg.encodeSlug("/Users/andrey/Code/personal/peektrace")).toBe(
          "-Users-andrey-Code-personal-peektrace"
        );
        expect(reg.encodeSlug("/a.b.c/d")).toBe("-a-b-c-d");
      })
    ));

  test("memoryDir composes projects root + slug + memory", () =>
    run(
      Effect.gen(function* () {
        const reg = yield* AgentRegistry;
        const dir = yield* reg.memoryDir({
          agent: "claude",
          slug: "-Users-x-y",
        });
        expect(dir.endsWith("/.claude/projects/-Users-x-y/memory")).toBe(true);
      })
    ));

  test("sessionsGlob targets every jsonl under projects", () =>
    run(
      Effect.gen(function* () {
        const reg = yield* AgentRegistry;
        const glob = yield* reg.sessionsGlob("claude");
        expect(glob.endsWith("/.claude/projects/**/*.jsonl")).toBe(true);
      })
    ));
});

describe("AgentRegistry agent gating", () => {
  test("Claude-layout resolvers fail for non-Claude agents", () =>
    run(
      Effect.gen(function* () {
        const reg = yield* AgentRegistry;
        // listProjectSlugs / memoryDir are gated on the Claude project layout;
        // Codex (date tree) has no per-project dirs even though it is supported.
        const result = yield* Effect.either(reg.listProjectSlugs("codex"));
        expect(result._tag).toBe("Left");
        if (result._tag === "Left") {
          expect(result.left._tag).toBe("AgentUnsupportedError");
          expect(result.left.agent).toBe("codex");
        }
      })
    ));

  test("OpenCode is a supported SQLite-backed agent with a resolvable root", () =>
    run(
      Effect.gen(function* () {
        const reg = yield* AgentRegistry;
        const root = yield* reg.projectsRoot("opencode");
        expect(root.length).toBeGreaterThan(0);
        expect(reg.roots("opencode").layout).toBe("opencode-sqlite");
      })
    ));

  test("declares roots for every agent in the matrix", () =>
    run(
      Effect.gen(function* () {
        const reg = yield* AgentRegistry;
        for (const id of AGENT_IDS) {
          const roots = reg.roots(id);
          expect(roots.id).toBe(id);
          expect(roots.projectsRoot.length).toBeGreaterThan(0);
        }
        expect(reg.roots("claude").supported).toBe(true);
        // Codex + Pi sessions are now parseable.
        expect(reg.roots("codex").supported).toBe(true);
        expect(reg.roots("pi").supported).toBe(true);
        expect(reg.roots("opencode").supported).toBe(true);
        expect(
          reg.roots("pi").projectsRoot.endsWith("/.pi/agent/sessions")
        ).toBe(true);
        expect(reg.roots("codex").layout).toBe("codex-datetree");
      })
    ));
});
