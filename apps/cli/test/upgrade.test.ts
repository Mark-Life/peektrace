/** `peektrace upgrade` — release resolution, checksum gate, and atomic replace.
 *
 * Drives the real command + IO against a local fake release server (Bun.serve on
 * an ephemeral port) that mimics the GitHub API + release-download shapes, wired
 * in via `PEEKTRACE_GITHUB_API` / `PEEKTRACE_BASE_URL`. No real network. Pure
 * helpers are exercised directly against fixture strings.
 */
import {
  afterAll,
  afterEach,
  beforeAll,
  describe,
  expect,
  test,
} from "bun:test";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Command } from "@effect/cli";
import { BunContext } from "@effect/platform-bun";
import { Effect } from "effect";
import { makeUpgrade } from "../src/commands/upgrade";
import { performUpgrade, sha256Hex } from "../src/upgrade/install";
import {
  compareVersions,
  detectAsset,
  parseChecksum,
  parseLatestCliTag,
  resolveReleaseConfig,
} from "../src/upgrade/release";
import { startupUpdateNotice } from "../src/upgrade/update-check";

const LATEST_TAG = "cli-v9.9.9";
const PAYLOAD = new Uint8Array([0x70, 0x65, 0x65, 0x6b, 0x9, 0x9, 0x9]);
const CORRECT_HEX = sha256Hex(PAYLOAD);
const WRONG_HEX = "0".repeat(64);

// The asset for the current host — the server routes on it and the command
// detects the same value at runtime.
const detection = detectAsset(process.platform, process.arch);
const HOST_ASSET =
  detection._tag === "supported" ? detection.asset : "peektrace-unknown";

/** When true, the server serves a deliberately wrong checksum for the asset. */
let serveWrongChecksum = false;

/** Count of requests the fake server has handled (reset per test as needed). */
let requestCount = 0;

let server: ReturnType<typeof Bun.serve>;
const savedEnv = {
  api: process.env.PEEKTRACE_GITHUB_API,
  base: process.env.PEEKTRACE_BASE_URL,
  version: process.env.PEEKTRACE_VERSION,
};

beforeAll(() => {
  server = Bun.serve({
    port: 0,
    fetch(req) {
      requestCount += 1;
      const { pathname } = new URL(req.url);
      if (pathname === "/releases") {
        // Newest-first, with a desktop-v* entry ahead to prove cli-v* filtering.
        return Response.json([
          { tag_name: "desktop-v3.0.0" },
          { tag_name: LATEST_TAG },
          { tag_name: "cli-v0.0.1" },
        ]);
      }
      if (pathname.endsWith("/SHA256SUMS")) {
        const hex = serveWrongChecksum ? WRONG_HEX : CORRECT_HEX;
        return new Response(`${hex}  ${HOST_ASSET}\n`);
      }
      if (pathname.endsWith(`/${HOST_ASSET}`)) {
        return new Response(PAYLOAD);
      }
      return new Response("not found", { status: 404 });
    },
  });
  const origin = `http://127.0.0.1:${server.port}`;
  process.env.PEEKTRACE_GITHUB_API = origin;
  process.env.PEEKTRACE_BASE_URL = origin;
  process.env.PEEKTRACE_VERSION = "";
});

afterAll(() => {
  server.stop(true);
  process.env.PEEKTRACE_GITHUB_API = savedEnv.api;
  process.env.PEEKTRACE_BASE_URL = savedEnv.base;
  process.env.PEEKTRACE_VERSION = savedEnv.version;
});

/** Run `fn` with `console.log` collected, returning the captured lines. */
const captureLog = async (fn: () => Promise<void>): Promise<string> => {
  const lines: string[] = [];
  const original = console.log;
  console.log = (...args: unknown[]) => {
    lines.push(args.join(" "));
  };
  try {
    await fn();
  } finally {
    console.log = original;
  }
  return lines.join("\n");
};

describe("upgrade --check", () => {
  test("reports an available update and writes nothing", async () => {
    const marker = join(tmpdir(), "peektrace-check-execpath");
    const before = Bun.file(marker).exists();

    const run = Command.run(makeUpgrade(), {
      name: "peektrace",
      version: "0.0.1",
    });
    const out = await captureLog(() =>
      Effect.runPromise(
        run(["bun", "peektrace", "--check"]).pipe(
          Effect.provide(BunContext.layer)
        )
      )
    );

    expect(out).toContain(`A newer version (${LATEST_TAG}) is available`);
    // No file was created as a side effect of a read-only check.
    expect(await Bun.file(marker).exists()).toBe(await before);
  });
});

describe("upgrade install", () => {
  test("checksum mismatch aborts and leaves the target unchanged", async () => {
    serveWrongChecksum = true;
    const dir = mkdtempSync(join(tmpdir(), "peektrace-upgrade-"));
    const target = join(dir, "peektrace");
    const original = new Uint8Array([1, 2, 3, 4]);
    writeFileSync(target, original);

    const error = await Effect.runPromise(
      performUpgrade({
        config: resolveReleaseConfig(),
        tag: LATEST_TAG,
        asset: HOST_ASSET,
        targetPath: target,
      }).pipe(Effect.flip)
    );

    expect(error._tag).toBe("CliUserError");
    expect(error.message).toContain("Checksum mismatch");
    // Pre-existing binary is byte-for-byte untouched.
    expect(new Uint8Array(readFileSync(target))).toEqual(original);
    serveWrongChecksum = false;
  });

  test("a verified upgrade replaces the target with the downloaded bytes", async () => {
    serveWrongChecksum = false;
    const dir = mkdtempSync(join(tmpdir(), "peektrace-upgrade-"));
    const target = join(dir, "peektrace");
    writeFileSync(target, new Uint8Array([9, 9, 9]));

    await Effect.runPromise(
      performUpgrade({
        config: resolveReleaseConfig(),
        tag: LATEST_TAG,
        asset: HOST_ASSET,
        targetPath: target,
      })
    );

    expect(new Uint8Array(readFileSync(target))).toEqual(PAYLOAD);
  });
});

describe("startupUpdateNotice (criterion e)", () => {
  const savedDir = process.env.PEEKTRACE_DIR;
  const savedNoCheck = process.env.PEEKTRACE_NO_UPDATE_CHECK;

  /** Point the cache at a fresh tmp dir and clear the gate for each case. */
  const freshCacheDir = (): string => {
    const dir = mkdtempSync(join(tmpdir(), "peektrace-updatecheck-"));
    process.env.PEEKTRACE_DIR = dir;
    process.env.PEEKTRACE_NO_UPDATE_CHECK = "";
    requestCount = 0;
    return dir;
  };

  afterEach(() => {
    process.env.PEEKTRACE_DIR = savedDir;
    process.env.PEEKTRACE_NO_UPDATE_CHECK = savedNoCheck;
  });

  const runNotice = (version: string): Promise<string> =>
    captureLog(() => Effect.runPromise(startupUpdateNotice(version)));

  test("prints the hint and writes the cache when a newer tag exists", async () => {
    const dir = freshCacheDir();
    const out = await runNotice("0.0.1");

    expect(out).toContain(`A newer version (${LATEST_TAG}) is available`);
    // The passive check hit the API and refreshed the on-disk cache.
    expect(requestCount).toBeGreaterThan(0);
    const cache = JSON.parse(
      readFileSync(join(dir, "update-check.json"), "utf8")
    ) as { latestTag: string; checkedAt: number };
    expect(cache.latestTag).toBe(LATEST_TAG);
    expect(typeof cache.checkedAt).toBe("number");
  });

  test("prints nothing when the current version is up to date", async () => {
    freshCacheDir();
    const out = await runNotice("9.9.9");

    expect(out).toBe("");
  });

  test("makes zero network calls when PEEKTRACE_NO_UPDATE_CHECK is set", async () => {
    freshCacheDir();
    process.env.PEEKTRACE_NO_UPDATE_CHECK = "1";

    const out = await runNotice("0.0.1");

    expect(requestCount).toBe(0);
    expect(out).toBe("");
  });

  test("resolves silently when the API is unreachable", async () => {
    freshCacheDir();
    const savedApi = process.env.PEEKTRACE_GITHUB_API;
    // A closed port: fetch is refused, and the notice must swallow it.
    process.env.PEEKTRACE_GITHUB_API = "http://127.0.0.1:1";
    try {
      const out = await runNotice("0.0.1");
      expect(out).toBe("");
    } finally {
      process.env.PEEKTRACE_GITHUB_API = savedApi;
    }
  });

  test("uses a fresh cache (<24h) and skips the network entirely", async () => {
    const dir = freshCacheDir();
    // Seed a fresh cache advertising a newer tag; no request should be made.
    writeFileSync(
      join(dir, "update-check.json"),
      JSON.stringify({ checkedAt: Date.now(), latestTag: "cli-v5.0.0" })
    );

    const out = await runNotice("0.0.1");

    expect(requestCount).toBe(0);
    expect(out).toContain("A newer version (cli-v5.0.0) is available");
  });
});

describe("pure helpers", () => {
  test("parseLatestCliTag picks the newest cli-v*, skipping other prefixes", () => {
    const tag = parseLatestCliTag([
      { tag_name: "desktop-v3.0.0" },
      { tag_name: "cli-v2.1.0" },
      { tag_name: "cli-v1.0.0" },
    ]);
    expect(tag).toBe("cli-v2.1.0");
    expect(parseLatestCliTag([{ tag_name: "desktop-v1.0.0" }])).toBeNull();
    expect(parseLatestCliTag("nope")).toBeNull();
  });

  test("parseChecksum extracts the hex for a matching asset only", () => {
    const sums =
      "abc123  peektrace-linux-x64\ndef456  peektrace-darwin-arm64\n";
    expect(parseChecksum(sums, "peektrace-darwin-arm64")).toBe("def456");
    expect(parseChecksum(sums, "peektrace-windows-x64.exe")).toBeNull();
  });

  test("detectAsset maps known platform/arch pairs and flags the rest", () => {
    expect(detectAsset("darwin", "arm64")).toMatchObject({
      _tag: "supported",
      asset: "peektrace-darwin-arm64",
    });
    expect(detectAsset("linux", "x64")).toMatchObject({
      asset: "peektrace-linux-x64",
    });
    expect(detectAsset("win32", "x64")).toMatchObject({
      asset: "peektrace-windows-x64.exe",
      os: "windows",
    });
    expect(detectAsset("freebsd", "x64")._tag).toBe("unsupported");
  });

  test("compareVersions orders dotted versions, ignoring prefixes and suffixes", () => {
    expect(compareVersions("cli-v9.9.9", "0.0.1")).toBe(1);
    expect(compareVersions("1.2.3", "1.2.3")).toBe(0);
    expect(compareVersions("v1.0.0", "v1.2.0")).toBe(-1);
    expect(compareVersions("0.0.0-dev", "cli-v0.0.1")).toBe(-1);
  });
});
