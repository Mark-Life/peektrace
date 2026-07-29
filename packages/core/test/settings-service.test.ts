import {
  afterAll,
  afterEach,
  beforeAll,
  describe,
  expect,
  test,
} from "bun:test";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { BunFileSystem } from "@effect/platform-bun";
import { Effect, Exit, Layer } from "effect";
import {
  SettingsService,
  SettingsServiceLive,
} from "../src/services/settings-service";

let base = "";
const prevDir = process.env.PEEKTRACE_DIR;

beforeAll(() => {
  base = mkdtempSync(join(tmpdir(), "peektrace-settings-"));
  process.env.PEEKTRACE_DIR = base;
});

afterEach(() => {
  rmSync(join(base, "settings.json"), { force: true });
});

afterAll(() => {
  if (prevDir === undefined) {
    delete process.env.PEEKTRACE_DIR;
  } else {
    process.env.PEEKTRACE_DIR = prevDir;
  }
  rmSync(base, { recursive: true, force: true });
});

const layer = SettingsServiceLive.pipe(Layer.provide(BunFileSystem.layer));
const run = <A, E>(program: Effect.Effect<A, E, SettingsService>) =>
  Effect.runPromise(
    program.pipe(Effect.provide(layer)) as Effect.Effect<A, E, never>
  );
const runExit = <A, E>(program: Effect.Effect<A, E, SettingsService>) =>
  Effect.runPromiseExit(
    program.pipe(Effect.provide(layer)) as Effect.Effect<A, E, never>
  );

describe("SettingsService", () => {
  test("get returns empty settings + mtime 0 when the file is absent", () =>
    run(
      Effect.gen(function* () {
        const svc = yield* SettingsService;
        const result = yield* svc.get;
        expect(result.settings).toEqual({});
        expect(result.mtimeMs).toBe(0);
        expect(result.path).toBe(join(base, "settings.json"));
      })
    ));

  test("update writes the file and get reads it back", () =>
    run(
      Effect.gen(function* () {
        const svc = yield* SettingsService;
        const written = yield* svc.update({
          settings: {
            roots: { claude: [{ path: "/work/.claude", label: "work" }] },
          },
        });
        expect(written.mtimeMs).toBeGreaterThan(0);
        // File exists and is valid pretty JSON.
        const raw = readFileSync(join(base, "settings.json"), "utf8");
        expect(JSON.parse(raw)).toEqual({
          roots: { claude: [{ path: "/work/.claude", label: "work" }] },
        });

        const reread = yield* svc.get;
        expect(reread.settings.roots?.claude).toEqual([
          { path: "/work/.claude", label: "work" },
        ]);
        expect(reread.mtimeMs).toBe(written.mtimeMs);
      })
    ));

  test("update with a stale expectedMtime fails FileChangedError", () =>
    run(
      Effect.gen(function* () {
        const svc = yield* SettingsService;
        yield* svc.update({ settings: { roots: { codex: [{ path: "/a" }] } } });
        const exit = yield* Effect.exit(
          svc.update({
            settings: { roots: { codex: [{ path: "/b" }] } },
            expectedMtime: 12_345, // not the real mtime
          })
        );
        expect(Exit.isFailure(exit)).toBe(true);
        if (Exit.isFailure(exit) && exit.cause._tag === "Fail") {
          expect(exit.cause.error._tag).toBe("FileChangedError");
        }
        // The stale write did not clobber the file.
        expect(
          JSON.parse(readFileSync(join(base, "settings.json"), "utf8"))
        ).toEqual({ roots: { codex: [{ path: "/a" }] } });
      })
    ));

  test("update with expectedMtime 0 succeeds only when the file is absent", () =>
    runExit(
      Effect.gen(function* () {
        const svc = yield* SettingsService;
        const first = yield* svc.update({
          settings: {},
          expectedMtime: 0,
        });
        expect(first.path.endsWith("settings.json")).toBe(true);
        expect(existsSync(first.path)).toBe(true);
      })
    ).then((exit) => {
      expect(Exit.isSuccess(exit)).toBe(true);
    }));
});
