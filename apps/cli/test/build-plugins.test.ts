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
import { dirname, join, resolve } from "node:path";
import {
  describeTool,
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
  const main = result.outputs.find((o) => o.kind === "entry-point")?.path;
  return { files, main, text: texts.join("\n"), success: result.success };
};

describe("stubTreeSitterAssets", () => {
  const entry = join(FIXTURES, "entry.js");

  test("drops OpenTUI grammars and queries, keeps worker and other wasm", async () => {
    const { text } = await bundle(entry, [stubTreeSitterAssets()], "ts-on");
    expect(text).not.toContain("wasm-grammar-marker");
    expect(text).not.toContain("(query-marker)");
    expect(text).toContain("worker-marker");
    expect(text).toContain("other-wasm-marker");
    expect(text).toContain(
      "tree-sitter assets are not bundled in the peektrace binary"
    );
  });

  test("without the plugin the assets are bundled", async () => {
    const { text } = await bundle(entry, [], "ts-off");
    expect(text).toContain("wasm-grammar-marker");
  });

  test("matches importers with Windows separators", () => {
    type Resolve = (args: { importer: string; path: string }) => unknown;
    let resolver: Resolve | undefined;
    stubTreeSitterAssets().setup({
      onLoad: () => undefined,
      onResolve: (_options: unknown, fn: Resolve) => {
        resolver = fn;
      },
    } as unknown as Bun.PluginBuilder);
    const resolve = (importer: string) =>
      resolver?.({ importer, path: "./lang.wasm" });
    expect(resolve("C:\\x\\node_modules\\@opentui\\core\\index.js")).toEqual({
      namespace: "opentui-no-tree-sitter",
      path: "./lang.wasm",
    });
    expect(resolve("/x/node_modules/@opentui/core/index.js")).toBeDefined();
    expect(resolve("/x/node_modules/other/index.js")).toBeUndefined();
  });

  test("the stubbed lazy loader throws a clear error", async () => {
    const { main } = await bundle(entry, [stubTreeSitterAssets()], "ts-run");
    const mod = await import(main ?? "");
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

  test("brew is looked up on the given PATH only", async () => {
    const dir = join(scratch, "path-brew");
    const prefix = join(scratch, "brew-prefix");
    await mkdir(join(prefix, "bin"), { recursive: true });
    await writeFile(join(prefix, "bin", "llvm-strip"), "");
    const brew = join(dir, "brew");
    await mkdir(dir, { recursive: true });
    await writeFile(brew, `#!/bin/sh\necho ${prefix}\n`);
    await chmod(brew, 0o755);
    expect(findStripTool("bun-linux-arm64", { PATH: dir })).toBe(
      join(prefix, "bin", "llvm-strip")
    );
    const hostPath = `${dir}:${process.env.PATH ?? ""}`;
    const saved = process.env.PATH;
    process.env.PATH = hostPath;
    try {
      expect(
        findStripTool("bun-linux-arm64", { PATH: join(scratch, "empty-path") })
      ).toBeUndefined();
    } finally {
      process.env.PATH = saved;
    }
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
            output: join(scratch, "c1", "lib.so"),
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

  test("required mode fails the build with a clear plugin error", async () => {
    const lib = await fakeLib("required");
    const entry = await makeEntry("required-entry", lib);
    const error = await bundle(
      entry,
      [
        stripNativeLib({
          output: join(scratch, "c2", "lib.so"),
          required: true,
          tool: undefined,
        }),
      ],
      "required-out"
    ).catch((e: unknown) => e);
    const messages = ((error as AggregateError).errors ?? []).map(
      (e: Error) => e.message
    );
    expect(messages).toContain(
      "no strip tool found (PEEKTRACE_STRIP_REQUIRED=1)"
    );
  });

  test("embeds a stripped copy and leaves the original alone", async () => {
    const dir = join(scratch, "fake-tool");
    await mkdir(dir, { recursive: true });
    const tool = await writeFakeStrip(dir, "fake-strip");
    const lib = await fakeLib("stripped");
    const before = await sha256(lib);
    const entry = await makeEntry("stripped-entry", lib);
    const output = join(scratch, "c3", "nested", "lib.so");
    const out = await bundle(
      entry,
      [stripNativeLib({ output, tool })],
      "stripped-out"
    );
    expect(out.success).toBe(true);
    expect((await readFile(output, "utf8")).endsWith("STRIPPED")).toBe(true);
    const emitted = out.files.find((f) => f.endsWith(".so"));
    expect(
      (await readFile(emitted as string, "utf8")).endsWith("STRIPPED")
    ).toBe(true);
    expect(await sha256(lib)).toBe(before);
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
    "embeds a shrunk copy and leaves node_modules untouched",
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
      const output = join(dir, "out", "libopentui.so");
      const out = await bundle(
        entry,
        [stripNativeLib({ output, tool: realTool })],
        "real-out"
      );
      expect(out.success).toBe(true);
      const stripped = (await stat(output)).size;
      expect(stripped).toBeLessThan(before.size / 2);
      const emitted = out.files.find((f) => f.endsWith(".so"));
      expect((await stat(emitted as string)).size).toBe(stripped);
      const magic = Buffer.from(
        await Bun.file(output).slice(0, 4).arrayBuffer()
      );
      expect(magic.equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]))).toBe(true);
      expect((await stat(lib)).size).toBe(before.size);
      expect(await sha256(lib)).toBe(before.sha);
    }
  );
});

describe("describeTool", () => {
  test("picks the version line, not the first line", async () => {
    const file = join(scratch, "multi-version-tool");
    await writeFile(
      file,
      '#!/bin/sh\necho "fake-llvm, compatible with GNU strip"\necho "Synthetic LLVM version 19.0.0"\n'
    );
    await chmod(file, 0o755);
    expect(describeTool(file)).toBe(`${file} (Synthetic LLVM version 19.0.0)`);
  });

  test("falls back to the first line without a version line", async () => {
    const file = join(scratch, "plain-tool");
    await writeFile(file, '#!/bin/sh\necho "plain-tool 2.1"\n');
    await chmod(file, 0o755);
    expect(describeTool(file)).toBe(`${file} (plain-tool 2.1)`);
  });
});
