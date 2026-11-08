import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { createHash } from "node:crypto";
import {
  chmod,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  stat,
  writeFile,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import {
  findStripTool,
  STRIP_TOOL_ENV,
  stripNativeLib,
  stubTreeSitterAssets,
} from "../src/build-plugins";

const GRAMMAR_ERROR = /tree-sitter assets are not bundled.*lang\.wasm/;
const QUERY_ERROR = /lang\.scm/;
const FIXTURES = resolve(import.meta.dirname, "fixtures", "build-plugins");

let scratch: string;
beforeAll(async () => {
  scratch = await mkdtemp(join(tmpdir(), "peektrace-build-plugins-"));
});
afterAll(async () => {
  await rm(scratch, { force: true, recursive: true });
});

const sha256 = async (file: string) =>
  createHash("sha256")
    .update(await readFile(file))
    .digest("hex");

/** Bundle `entry` and return output file names plus all text content. */
const bundle = async (
  entry: string,
  plugins: Bun.BunPlugin[],
  name: string
) => {
  const outdir = join(scratch, name);
  const result = await Bun.build({
    entrypoints: [entry],
    outdir,
    plugins,
    target: "bun",
  });
  const files = result.outputs.map((o) => o.path);
  const texts = await Promise.all(
    files.map(async (f) =>
      (await stat(f)).size < 1_000_000 ? readFile(f, "utf8") : ""
    )
  );
  return { files, text: texts.join("\n"), success: result.success };
};

describe("stubTreeSitterAssets", () => {
  const entry = join(FIXTURES, "entry.js");

  test("drops OpenTUI grammars and queries, keeps worker and other wasm", async () => {
    const { files, text } = await bundle(
      entry,
      [stubTreeSitterAssets()],
      "ts-on"
    );
    const contents = await Promise.all(
      files.map((f) => readFile(f, "utf8").catch(() => ""))
    );
    const all = contents.join("\n");
    expect(all).not.toContain("wasm-grammar-marker");
    expect(all).not.toContain("(query-marker)");
    expect(all).toContain("worker-marker");
    expect(all).toContain("other-wasm-marker");
    expect(text).toContain(
      "tree-sitter assets are not bundled in the peektrace binary"
    );
  });

  test("without the plugin the assets are bundled", async () => {
    const { files } = await bundle(entry, [], "ts-off");
    const all = (
      await Promise.all(files.map((f) => readFile(f, "utf8").catch(() => "")))
    ).join("\n");
    expect(all).toContain("wasm-grammar-marker");
  });

  test("the stubbed lazy loader throws a clear error", async () => {
    const outdir = join(scratch, "ts-run");
    const result = await Bun.build({
      entrypoints: [entry],
      outdir,
      plugins: [stubTreeSitterAssets()],
      target: "bun",
    });
    const main = result.outputs.find((o) => o.kind === "entry-point");
    const mod = await import(main?.path ?? "");
    await expect(mod.result.loadGrammar()).rejects.toThrow(GRAMMAR_ERROR);
    await expect(mod.result.loadQuery()).rejects.toThrow(QUERY_ERROR);
    expect(await mod.result.loadOther()).toBeDefined();
  });
});

/** Executable shell script that copies its `-o` argument pair's input. */
const writeFakeStrip = async (dir: string, name: string) => {
  const file = join(dir, name);
  await writeFile(
    file,
    '#!/bin/sh\nif [ "$1" = "--version" ]; then echo "fake-strip 1.0"; exit 0; fi\ncp "$4" "$3"\nprintf STRIPPED >> "$3"\n'
  );
  await chmod(file, 0o755);
  return file;
};

describe("findStripTool", () => {
  test("PEEKTRACE_STRIP wins over PATH", () => {
    expect(
      findStripTool("bun-linux-x64", { [STRIP_TOOL_ENV]: "/x/custom" })
    ).toBe("/x/custom");
  });

  test("llvm-strip on PATH is preferred", async () => {
    const dir = join(scratch, "path-llvm");
    await mkdir(dir, { recursive: true });
    const fake = await writeFakeStrip(dir, "llvm-strip");
    expect(findStripTool("bun-linux-arm64", { PATH: dir })).toBe(fake);
  });

  test("no tool for a foreign arch without llvm-strip", () => {
    expect(
      findStripTool("bun-linux-arm64", { PATH: join(scratch, "empty-path") })
    ).toBeUndefined();
  });
});

describe("stripNativeLib", () => {
  const makeEntry = async (name: string, lib: string) => {
    const dir = join(scratch, name);
    await mkdir(dir, { recursive: true });
    const entry = join(dir, "entry.js");
    await writeFile(
      entry,
      `export const lib = await import(${JSON.stringify(lib)}, { with: { type: "file" } });\n`
    );
    return entry;
  };

  const fakeLib = async (name: string) => {
    const dir = join(scratch, `${name}-pkg`);
    await mkdir(dir, { recursive: true });
    await writeFile(
      join(dir, "package.json"),
      '{"name":"@synthetic/native","version":"1.2.3"}'
    );
    const lib = join(dir, "libopentui.so");
    await writeFile(lib, "original-bytes");
    return lib;
  };

  test("a missing tool warns and keeps the original", async () => {
    const lib = await fakeLib("missing");
    const entry = await makeEntry("missing-entry", lib);
    const warn = console.warn;
    const warnings: string[] = [];
    console.warn = (m: string) => warnings.push(m);
    try {
      const out = await bundle(
        entry,
        [
          stripNativeLib({
            cacheDir: join(scratch, "c1"),
            tool: join(scratch, "nope"),
          }),
        ],
        "missing-out"
      );
      expect(out.success).toBe(true);
    } finally {
      console.warn = warn;
    }
    expect(warnings.join("\n")).toContain("unstripped");
  });

  test("required mode fails the build", async () => {
    const lib = await fakeLib("required");
    const entry = await makeEntry("required-entry", lib);
    await expect(
      bundle(
        entry,
        [
          stripNativeLib({
            cacheDir: join(scratch, "c2"),
            required: true,
            tool: undefined,
          }),
        ],
        "required-out"
      )
    ).rejects.toThrow();
  });

  test("strips to the cache, reuses it, and leaves the original alone", async () => {
    const dir = join(scratch, "fake-tool");
    await mkdir(dir, { recursive: true });
    const tool = await writeFakeStrip(dir, "fake-strip");
    const lib = await fakeLib("cached");
    const before = await sha256(lib);
    const entry = await makeEntry("cached-entry", lib);
    const cacheDir = join(scratch, "c3");
    const out = await bundle(
      entry,
      [stripNativeLib({ cacheDir, tool })],
      "cached-out"
    );
    expect(out.success).toBe(true);
    const [cached] = await Array.fromAsync(
      new Bun.Glob("*.so").scan({ absolute: true, cwd: cacheDir })
    );
    expect(basename(cached ?? "")).toStartWith("_synthetic_native-1.2.3-14-");
    expect(
      (await readFile(cached as string, "utf8")).endsWith("STRIPPED")
    ).toBe(true);
    expect(await sha256(lib)).toBe(before);
    const mtime = (await stat(cached as string)).mtimeMs;
    await bundle(entry, [stripNativeLib({ cacheDir, tool })], "cached-out2");
    expect((await stat(cached as string)).mtimeMs).toBe(mtime);
  });
});

const arch = process.arch === "arm64" ? "arm64" : "x64";
const realLib = (() => {
  if (process.platform !== "linux") {
    return;
  }
  try {
    const coreDir = dirname(
      Bun.resolveSync("@opentui/core/package.json", import.meta.dirname)
    );
    return join(
      dirname(
        Bun.resolveSync(`@opentui/core-linux-${arch}/package.json`, coreDir)
      ),
      "libopentui.so"
    );
  } catch {
    return;
  }
})();
const realTool = findStripTool(`bun-linux-${arch}`);

describe("stripNativeLib on the real OpenTUI library", () => {
  test.skipIf(!(realLib && realTool))(
    "shrinks a copy and leaves node_modules untouched",
    async () => {
      const lib = realLib as string;
      const before = { sha: await sha256(lib), size: (await stat(lib)).size };
      const dir = join(scratch, "real");
      await mkdir(dir, { recursive: true });
      const entry = join(dir, "entry.js");
      await writeFile(
        entry,
        `export const lib = await import(${JSON.stringify(lib)}, { with: { type: "file" } });\n`
      );
      const cacheDir = join(dir, "cache");
      const out = await bundle(
        entry,
        [stripNativeLib({ cacheDir, tool: realTool })],
        "real-out"
      );
      expect(out.success).toBe(true);
      const [stripped] = await Array.fromAsync(
        new Bun.Glob("*.so").scan({ cwd: cacheDir, absolute: true })
      );
      expect(stripped).toBeDefined();
      expect((await stat(stripped as string)).size).toBeLessThan(
        before.size / 2
      );
      const magic = Buffer.from(
        await Bun.file(stripped as string)
          .slice(0, 4)
          .arrayBuffer()
      );
      expect(magic.equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]))).toBe(true);
      expect((await stat(lib)).size).toBe(before.size);
      expect(await sha256(lib)).toBe(before.sha);
    }
  );
});
