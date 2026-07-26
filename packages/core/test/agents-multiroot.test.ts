import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { BunFileSystem } from "@effect/platform-bun";
import { Effect, Layer } from "effect";
import { AgentRegistry, AgentRegistryLive } from "../src/services/agents";

const SLUG_A = "-Users-demo-personal";
const SLUG_B = "-Users-demo-work";
const ID_A = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const ID_B = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";

let base = "";
let defaultProjects = "";
let workHome = "";
let workProjects = "";
let ptDir = "";

const prevEnv = {
  dir: process.env.PEEKTRACE_DIR,
  claude: process.env.PEEKTRACE_CLAUDE_PROJECTS,
};

/** Drop one transcript file into `<root>/<slug>/<id>.jsonl`. */
const seed = (root: string, slug: string, id: string) => {
  const dir = join(root, slug);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, `${id}.jsonl`), '{"type":"user"}\n');
};

beforeAll(() => {
  base = mkdtempSync(join(tmpdir(), "peektrace-multiroot-"));
  defaultProjects = join(base, "default", "projects");
  workHome = join(base, "work");
  workProjects = join(workHome, "projects");
  ptDir = join(base, "pt");
  mkdirSync(ptDir, { recursive: true });
  seed(defaultProjects, SLUG_A, ID_A);
  seed(workProjects, SLUG_B, ID_B);
  writeFileSync(
    join(ptDir, "settings.json"),
    JSON.stringify({ roots: { claude: [{ path: workHome, label: "work" }] } })
  );
  process.env.PEEKTRACE_DIR = ptDir;
  process.env.PEEKTRACE_CLAUDE_PROJECTS = defaultProjects;
});

afterAll(() => {
  if (prevEnv.dir === undefined) {
    delete process.env.PEEKTRACE_DIR;
  } else {
    process.env.PEEKTRACE_DIR = prevEnv.dir;
  }
  if (prevEnv.claude === undefined) {
    delete process.env.PEEKTRACE_CLAUDE_PROJECTS;
  } else {
    process.env.PEEKTRACE_CLAUDE_PROJECTS = prevEnv.claude;
  }
  rmSync(base, { recursive: true, force: true });
});

const layer = AgentRegistryLive.pipe(Layer.provide(BunFileSystem.layer));
const run = <A, E>(program: Effect.Effect<A, E, AgentRegistry>) =>
  Effect.runPromise(
    program.pipe(Effect.provide(layer)) as Effect.Effect<A, E, never>
  );

describe("AgentRegistry multi-root (config file)", () => {
  test("unions sessions across the default + configured roots, stamping source", () =>
    run(
      Effect.gen(function* () {
        const reg = yield* AgentRegistry;
        const refs = yield* reg.listSessionFiles("claude");
        const byId = new Map(refs.map((r) => [r.id, r]));

        expect(refs).toHaveLength(2);
        expect(byId.get(ID_A)?.source).toEqual({
          id: "claude",
          label: "default",
        });
        expect(byId.get(ID_B)?.source).toEqual({
          id: "claude:work",
          label: "work",
        });
        // The work transcript resolves under its own root.
        expect(byId.get(ID_B)?.path).toBe(
          join(workProjects, SLUG_B, `${ID_B}.jsonl`)
        );
      })
    ));

  test("declares both source roots (default + work) for FS containment", () =>
    run(
      Effect.gen(function* () {
        const reg = yield* AgentRegistry;
        expect(reg.allowedRoots).toContain(defaultProjects);
        expect(reg.allowedRoots).toContain(workProjects);
        expect(reg.allowedRoots).toContain(workHome);
      })
    ));

  test("unions project slugs across roots", () =>
    run(
      Effect.gen(function* () {
        const reg = yield* AgentRegistry;
        const slugs = yield* reg.listProjectSlugs("claude");
        expect([...slugs].sort()).toEqual([SLUG_A, SLUG_B].sort());
      })
    ));
});
