/** Build-time Bun plugins and helpers for `build.ts`.
 *
 * Kept apart from `build.ts` so tests can import them without running its
 * `main()`.
 */
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync } from "node:fs";
import { mkdir, readFile, realpath, rename, rm, stat } from "node:fs/promises";
import { basename, dirname, join } from "node:path";

/** Namespace for the generated stand-in module (see `excludeNativePackage`). */
const STUB_NAMESPACE = "opentui-unused-native";

/** Matches every path in `STUB_NAMESPACE` — the namespace does the filtering. */
const ANY_PATH = /.*/;

/** Architecture segment of a Bun compile target (`bun-linux-x64-musl` -> `x64`). */
export const archOf = (target: string) =>
  target.includes("arm64") ? "arm64" : "x64";

/**
 * The `@opentui/core-*` native package a compile target actually loads.
 *
 * `@opentui/core` picks its native Zig library through per-platform optional
 * dependencies, and `bun build --compile` folds `process.platform`/`process.arch`
 * to the TARGET's values — so a cross-compile resolves the target's import, not
 * the host's.
 */
export const nativePackageFor = (target: string) => {
  const arch = archOf(target);
  if (target.includes("darwin")) {
    return `@opentui/core-darwin-${arch}`;
  }
  if (target.includes("windows")) {
    return `@opentui/core-win32-${arch}`;
  }
  if (target.includes("linux")) {
    const libc = target.includes("musl") ? "-musl" : "";
    return `@opentui/core-linux-${arch}${libc}`;
  }
  return;
};

/**
 * The sibling libc variant a Linux target never loads.
 *
 * OpenTUI chooses between glibc and musl with a runtime `process.env.OPENTUI_LIBC`
 * check, which the bundler cannot fold away — so without this both 13 MB `.so`
 * files land in a Linux binary even though only one is reachable.
 */
export const unusedNativePackageFor = (target: string) => {
  if (!target.includes("linux")) {
    return;
  }
  const arch = archOf(target);
  return target.includes("musl")
    ? `@opentui/core-linux-${arch}`
    : `@opentui/core-linux-${arch}-musl`;
};

/**
 * Replace `specifier` with a module that throws, so its native library is never
 * embedded. Only reachable if `OPENTUI_LIBC` contradicts the build target, which
 * a clear error explains better than a failed `dlopen` would.
 */
export const excludeNativePackage = (specifier: string, target: string) => ({
  name: "opentui-unused-native",
  setup: (build: Bun.PluginBuilder) => {
    build.onResolve({ filter: new RegExp(`^${specifier}$`) }, () => ({
      namespace: STUB_NAMESPACE,
      path: specifier,
    }));
    build.onLoad({ filter: ANY_PATH, namespace: STUB_NAMESPACE }, () => ({
      contents: `throw new Error(${JSON.stringify(
        `${specifier} is not bundled: this binary was built for ${target}.`
      )});`,
      loader: "js" as const,
    }));
  },
});

/** Hex chars of the tool-path hash in cache file names. */
const TOOL_HASH_LENGTH = 8;

/** Matches the native OpenTUI library import (`./libopentui.so`). */
const NATIVE_LIB = /libopentui\.so$/;

/** Env var naming the strip executable; wins over auto-detection. */
export const STRIP_TOOL_ENV = "PEEKTRACE_STRIP";

/** When `1`, a failed or impossible strip fails the build instead of warning. */
export const STRIP_REQUIRED_ENV = "PEEKTRACE_STRIP_REQUIRED";

const runsOnPath = (tool: string, searchPath = process.env.PATH ?? "") =>
  Bun.which(tool, { PATH: searchPath }) ?? undefined;

const homebrewLlvmStrip = () => {
  const brew = runsOnPath("brew");
  if (!brew) {
    return;
  }
  const result = spawnSync(brew, ["--prefix", "llvm"], { encoding: "utf8" });
  if (result.status !== 0) {
    return;
  }
  const candidate = join(result.stdout.trim(), "bin", "llvm-strip");
  return existsSync(candidate) ? candidate : undefined;
};

/**
 * Find a `strip` that can handle the target's ELF files, or `undefined`.
 *
 * Order: `PEEKTRACE_STRIP`, `llvm-strip` on PATH (works for any arch), Homebrew
 * llvm, then GNU `strip` only when the host is Linux of the target's arch (GNU
 * strip cannot read foreign ELF; Apple `strip` cannot read ELF at all).
 */
export const findStripTool = (
  target: string,
  env: Record<string, string | undefined> = process.env
) => {
  const explicit = env[STRIP_TOOL_ENV];
  if (explicit) {
    return explicit;
  }
  const llvm = runsOnPath("llvm-strip", env.PATH ?? "") ?? homebrewLlvmStrip();
  if (llvm) {
    return llvm;
  }
  const hostMatches =
    process.platform === "linux" &&
    (process.arch === "arm64" ? "arm64" : "x64") === archOf(target);
  return hostMatches ? runsOnPath("strip", env.PATH ?? "") : undefined;
};

/** First line of `<tool> --version`, for the build log. */
export const describeTool = (tool: string) => {
  const result = spawnSync(tool, ["--version"], { encoding: "utf8" });
  const line = result.stdout?.split("\n")[0]?.trim();
  return line ? `${tool} (${line})` : tool;
};

interface StripOptions {
  /** Where stripped copies are cached; created on demand. */
  cacheDir: string;
  /** Fail instead of warn when the strip cannot run. */
  required?: boolean;
  /** Strip executable from `findStripTool`; `undefined` skips stripping. */
  tool: string | undefined;
}

const fail = (options: StripOptions, message: string, original: string) => {
  if (options.required) {
    throw new Error(`${message} (${STRIP_REQUIRED_ENV}=1)`);
  }
  console.warn(`warning: ${message}; embedding the unstripped library.`);
  return original;
};

/**
 * Strip a copy of `file` into the cache and return its path.
 *
 * The package manager hardlinks `node_modules` files across installs, so the
 * original must never be modified: strip writes a temp copy, then renames it,
 * so an interrupted run cannot leave a half-written cache hit.
 */
const stripToCache = async (
  file: string,
  options: StripOptions & { tool: string }
) => {
  const real = await realpath(file);
  const { size } = await stat(real);
  const pkg = await readFile(join(dirname(real), "package.json"), "utf8")
    .then((text) => JSON.parse(text) as { name?: string; version?: string })
    .catch(() => ({}) as { name?: string; version?: string });
  const label =
    `${pkg.name ?? basename(dirname(real))}-${pkg.version ?? "0"}`.replaceAll(
      /[^\w.-]/g,
      "_"
    );
  const hash = createHash("sha256")
    .update(options.tool)
    .digest("hex")
    .slice(0, TOOL_HASH_LENGTH);
  const cached = join(options.cacheDir, `${label}-${size}-${hash}.so`);
  if (existsSync(cached)) {
    return cached;
  }
  await mkdir(options.cacheDir, { recursive: true });
  const temp = `${cached}.${process.pid}.tmp`;
  const result = spawnSync(options.tool, ["--strip-debug", "-o", temp, real], {
    encoding: "utf8",
  });
  if (result.error || result.status !== 0) {
    await rm(temp, { force: true });
    throw new Error(
      `${options.tool} failed on ${label}: ${result.error?.message ?? result.stderr.trim()}`
    );
  }
  await rename(temp, cached);
  return cached;
};

/**
 * Embed a debug-stripped copy of the native OpenTUI library.
 *
 * The Linux `@opentui/core-linux-*` libraries ship with DWARF debug info
 * (about 10 MB of a 14 MB file). `--strip-debug` keeps `.dynsym`, which
 * `dlopen` needs. The macOS and Windows libraries have no debug info.
 */
export const stripNativeLib = (options: StripOptions) => ({
  name: "strip-native-lib",
  setup: (build: Bun.PluginBuilder) => {
    build.onResolve({ filter: NATIVE_LIB }, async (args) => {
      const file = args.path.startsWith(".")
        ? join(dirname(args.importer), args.path)
        : args.path;
      const { tool } = options;
      if (!tool) {
        return { path: fail(options, "no strip tool found", file) };
      }
      try {
        const path = await stripToCache(file, { ...options, tool });
        return { path };
      } catch (error) {
        return {
          path: fail(
            options,
            error instanceof Error ? error.message : String(error),
            file
          ),
        };
      }
    });
  },
});

/** Namespace for stubbed tree-sitter assets. */
const NO_TREE_SITTER_NAMESPACE = "opentui-no-tree-sitter";

/** Grammar and query assets OpenTUI loads lazily for tree-sitter. */
const TREE_SITTER_ASSET = /\.(wasm|scm)$/;

/**
 * Drop OpenTUI's tree-sitter grammars (`.wasm`) and queries (`.scm`) from the
 * bundle, about 10 MB. They are only loaded by OpenTUI's `<code>` and
 * `<markdown>` renderables; peektrace highlights with `@workspace/core/highlight`
 * instead. Remove this plugin before adopting those renderables.
 *
 * `parser.worker` stays real: OpenTUI imports it at top level, so stubbing it
 * breaks startup. Other packages' `.wasm` files are untouched.
 */
export const stubTreeSitterAssets = () => ({
  name: "stub-tree-sitter-assets",
  setup: (build: Bun.PluginBuilder) => {
    build.onResolve({ filter: TREE_SITTER_ASSET }, (args) => {
      if (!args.importer.includes("/@opentui/")) {
        return;
      }
      return { namespace: NO_TREE_SITTER_NAMESPACE, path: args.path };
    });
    build.onLoad(
      { filter: ANY_PATH, namespace: NO_TREE_SITTER_NAMESPACE },
      (args) => ({
        contents: `throw new Error(${JSON.stringify(
          `tree-sitter assets are not bundled in the peektrace binary: ${args.path}`
        )});`,
        loader: "js" as const,
      })
    );
  },
});
