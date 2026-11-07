import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { BunFileSystem } from "@effect/platform-bun";
import { Effect, Layer } from "effect";
import { AgentRegistry, AgentRegistryLive } from "../src/services/agents";
import { writeGoodDb } from "./fixtures/opencode-db";

const CODEX_ID = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const PI_ID = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
const OC_ID = "ses_demo1";

let base = "";
let ocData = "";
const keys = [
  "PEEKTRACE_DIR",
  "PEEKTRACE_CODEX_SESSIONS",
  "PEEKTRACE_PI_SESSIONS",
  "PEEKTRACE_OPENCODE_DATA",
] as const;
const prev = Object.fromEntries(keys.map((k) => [k, process.env[k]]));

beforeAll(() => {
  base = mkdtempSync(join(tmpdir(), "peektrace-walkers-"));
  const codex = join(base, "codex", "2026", "01", "02");
  mkdirSync(codex, { recursive: true });
  writeFileSync(
    join(codex, `rollout-2026-01-02T00-00-00-${CODEX_ID}.jsonl`),
    '{"type":"session_meta"}\n'
  );
  writeFileSync(join(codex, "rollout-demo.jsonl"), "{}\n");
  writeFileSync(join(codex, "notes.txt"), "ignored");
  const pi = join(base, "pi", "--demo-project--");
  mkdirSync(pi, { recursive: true });
  writeFileSync(
    join(pi, `2026-01-02T00-00-00-000Z_${PI_ID}.jsonl`),
    '{"type":"session"}\n'
  );
  writeFileSync(join(pi, "plain.jsonl"), "{}\n");
  ocData = join(base, "opencode");
  mkdirSync(ocData, { recursive: true });
  writeGoodDb(join(ocData, "opencode.db"), OC_ID, "/demo/project");
  mkdirSync(join(base, "pt"), { recursive: true });
  process.env.PEEKTRACE_DIR = join(base, "pt");
  process.env.PEEKTRACE_CODEX_SESSIONS = join(base, "codex");
  process.env.PEEKTRACE_PI_SESSIONS = join(base, "pi");
  process.env.PEEKTRACE_OPENCODE_DATA = ocData;
});

afterAll(() => {
  for (const k of keys) {
    if (prev[k] === undefined) {
      delete process.env[k];
    } else {
      process.env[k] = prev[k];
    }
  }
  rmSync(base, { recursive: true, force: true });
});

const layer = AgentRegistryLive.pipe(Layer.provide(BunFileSystem.layer));
const run = <A, E>(program: Effect.Effect<A, E, AgentRegistry>) =>
  Effect.runPromise(
    program.pipe(Effect.provide(layer)) as Effect.Effect<A, E, never>
  );

const sortedIds = (refs: readonly { readonly id: string }[]) =>
  refs.map((r) => r.id).sort();

describe("AgentRegistry walkers", () => {
  test("codex walks the date tree, keeps only rollout files", () =>
    run(
      Effect.gen(function* () {
        const reg = yield* AgentRegistry;
        const refs = yield* reg.listSessionFiles("codex");
        // A rollout name without a uuid falls back to its stem.
        expect(sortedIds(refs)).toEqual([CODEX_ID, "rollout-demo"].sort());
        expect(refs.every((r) => r.slug === "")).toBe(true);
      })
    ));

  test("pi lists slug dirs, id from after the underscore", () =>
    run(
      Effect.gen(function* () {
        const reg = yield* AgentRegistry;
        const refs = yield* reg.listSessionFiles("pi");
        expect(refs.every((r) => r.slug === "--demo-project--")).toBe(true);
        // A name without `_` falls back to its stem.
        expect(sortedIds(refs)).toEqual([PI_ID, "plain"].sort());
      })
    ));

  test("loadTranscript reads text + stamp; missing file fails typed", () =>
    run(
      Effect.gen(function* () {
        const reg = yield* AgentRegistry;
        const refs = yield* reg.listSessionFiles("codex");
        const ref = refs.find((r) => r.id === CODEX_ID);
        if (!ref) {
          throw new Error("no ref");
        }
        const payload = yield* reg.loadTranscript({ agent: "codex", ref });
        expect(payload.text).toBe('{"type":"session_meta"}\n');
        expect(payload.sizeBytes).toBe(payload.text.length);
        expect(payload.mtimeMs).toBeGreaterThan(0);
        const known = yield* reg.loadTranscript({
          agent: "codex",
          ref,
          stamp: { sizeBytes: 1, mtimeMs: 2 },
        });
        expect([known.sizeBytes, known.mtimeMs]).toEqual([1, 2]);
        const err = yield* reg
          .loadTranscript({
            agent: "codex",
            ref: { ...ref, path: join(base, "missing.jsonl") },
          })
          .pipe(Effect.flip);
        expect(err._tag).toBe("TranscriptReadError");
      })
    ));

  test("gitRoot falls back to cwd outside a repo", () =>
    run(
      Effect.gen(function* () {
        const reg = yield* AgentRegistry;
        expect(yield* reg.gitRoot(base)).toBe(base);
      })
    ));
});

describe("AgentRegistry opencode", () => {
  test("lists sessions as <dataDir>#<id> refs slugged by directory", () =>
    run(
      Effect.gen(function* () {
        const reg = yield* AgentRegistry;
        const refs = yield* reg.listSessionFiles("opencode");
        expect(refs).toEqual([
          { id: OC_ID, slug: "/demo/project", path: `${ocData}#${OC_ID}` },
        ]);
      })
    ));

  test("loadTranscript serializes the session; no-hash path uses the default root", () =>
    run(
      Effect.gen(function* () {
        const reg = yield* AgentRegistry;
        const [ref] = yield* reg.listSessionFiles("opencode");
        if (!ref) {
          throw new Error("no ref");
        }
        const payload = yield* reg.loadTranscript({ agent: "opencode", ref });
        expect(payload.text.length).toBeGreaterThan(0);
        expect(payload.sizeBytes).toBe(Buffer.byteLength(payload.text, "utf8"));
        const fallback = yield* reg.loadTranscript({
          agent: "opencode",
          ref: { path: "no-hash", slug: "", id: OC_ID },
        });
        expect(fallback.text).toBe(payload.text);
        const unknown = yield* reg.loadTranscript({
          agent: "opencode",
          ref: { path: `${ocData}#ses_missing`, slug: "", id: "ses_missing" },
        });
        expect([unknown.text, unknown.sizeBytes]).toEqual(["", 0]);
      })
    ));
});
