#!/usr/bin/env bun
/** Compile the peektrace CLI into a single self-contained binary.
 *
 * Steps:
 * 1. Build the inspector UI (`bun run --filter=inspector build`) into
 *    `apps/inspector/dist`.
 * 2. Generate `src/embedded-ui.gen.ts`: one `import … with { type: "file" }`
 *    per built asset plus a default-exported map of URL path -> embedded file
 *    reference. The import attribute is what makes `bun build --compile` bake
 *    each asset into the binary's virtual filesystem.
 * 3. `bun build --compile` `src/index.ts` for the host target (or `BUN_TARGET`),
 *    emitting `dist/<target>/peektrace`.
 * 4. Restore the committed stub of `src/embedded-ui.gen.ts` in a `finally`, so
 *    the working tree is never left with machine-specific generated imports.
 *
 * No native/WASM staging — the UI is the only thing embedded.
 */
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";

const CLI_SRC = import.meta.dirname;
const CLI_ROOT = resolve(CLI_SRC, "..");
const REPO_ROOT = resolve(CLI_SRC, "..", "..", "..");
const INSPECTOR_DIST = join(REPO_ROOT, "apps", "inspector", "dist");
const GEN_PATH = join(CLI_SRC, "embedded-ui.gen.ts");
const ENTRYPOINT = join(CLI_SRC, "index.ts");

/** Committed stub restored after every build (see `embedded-ui.gen.ts`). */
const GEN_STUB = `/**
 * Embedded inspector asset manifest.
 *
 * Committed stub — the default export is \`null\` when running from source, which
 * makes the server fall back to serving the inspector \`dist/\` from disk.
 *
 * The binary build (\`src/build.ts\`) overwrites this file in place with one
 * \`import ... with { type: "file" }\` per built asset plus a default-exported map
 * of URL path (\`/index.html\`, \`/assets/…\`) -> embedded file reference, so that
 * \`bun build --compile\` bakes every asset into the binary's virtual filesystem.
 * The build restores this stub afterwards; the generated (non-null) form is a
 * local build artifact and must never be committed.
 */
const files: Record<string, string> | null = null;

export default files;
`;

/** Bun compile target strings keyed by \`<platform>-<arch>\`. */
const HOST_TARGETS: Record<string, Bun.Build.CompileTarget> = {
  "darwin-arm64": "bun-darwin-arm64",
  "darwin-x64": "bun-darwin-x64",
  "linux-x64": "bun-linux-x64",
  "linux-arm64": "bun-linux-arm64",
  "win32-x64": "bun-windows-x64",
  "win32-arm64": "bun-windows-arm64",
};

/** Resolve the Bun compile target from `BUN_TARGET` or the host platform. */
const resolveTarget = (): Bun.Build.CompileTarget => {
  const fromEnv = process.env.BUN_TARGET;
  if (fromEnv) {
    return fromEnv as Bun.Build.CompileTarget;
  }
  const key = `${process.platform}-${process.arch}`;
  const target = HOST_TARGETS[key];
  if (!target) {
    throw new Error(`Unsupported host platform: ${key}`);
  }
  return target;
};

/** Namespace for the generated stand-in module (see `excludeNativePackage`). */
const STUB_NAMESPACE = "opentui-unused-native";

/** Matches every path in `STUB_NAMESPACE` — the namespace does the filtering. */
const ANY_PATH = /.*/;

/** Architecture segment of a Bun compile target (`bun-linux-x64-musl` -> `x64`). */
const archOf = (target: string) => (target.includes("arm64") ? "arm64" : "x64");

/**
 * The `@opentui/core-*` native package a compile target actually loads.
 *
 * `@opentui/core` picks its native Zig library through per-platform optional
 * dependencies, and `bun build --compile` folds `process.platform`/`process.arch`
 * to the TARGET's values — so a cross-compile resolves the target's import, not
 * the host's.
 */
const nativePackageFor = (target: string) => {
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
const unusedNativePackageFor = (target: string) => {
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
const excludeNativePackage = (specifier: string, target: string) => ({
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

/**
 * Fail fast (before the slow inspector build) when the target's native OpenTUI
 * package is missing — `bun install` only fetches the host's optional deps.
 */
const assertNativeDepsInstalled = (target: Bun.Build.CompileTarget) => {
  const required = nativePackageFor(target);
  if (!required) {
    return;
  }
  const coreDir = resolve(
    Bun.resolveSync("@opentui/core/package.json", CLI_ROOT),
    ".."
  );
  const isMissing = () => {
    try {
      // `resolveSync` follows a dangling symlink, so stat the result too.
      return !existsSync(Bun.resolveSync(required, coreDir));
    } catch {
      return true;
    }
  };
  if (isMissing()) {
    throw new Error(
      `Missing native package for ${target}: ${required}.\n` +
        "Cross-compiling needs every platform's optional deps. Run:\n" +
        "  bun install --frozen-lockfile --cpu='*' --os='*'"
    );
  }
};

/** Build the inspector UI into `apps/inspector/dist`; throws on failure. */
const buildInspector = () => {
  const result = spawnSync("bun", ["run", "--filter=inspector", "build"], {
    cwd: REPO_ROOT,
    stdio: ["ignore", "inherit", "inherit"],
  });
  if (result.status !== 0) {
    throw new Error(`Inspector build failed (exit ${result.status})`);
  }
};

/** Turn a dist-relative file path into its served URL path (`/index.html`). */
const toUrlPath = (rel: string) => `/${rel}`;

/**
 * Generate `src/embedded-ui.gen.ts` from the built inspector `dist/`: an import
 * (with the `type: "file"` attribute) per asset and a default-exported map of
 * URL path -> the imported bunfs reference.
 */
const generateEmbeddedManifest = async () => {
  const files = (
    await Array.fromAsync(new Bun.Glob("**/*").scan({ cwd: INSPECTOR_DIST }))
  )
    .map((f) => f.replaceAll("\\", "/"))
    .sort();
  if (files.length === 0) {
    throw new Error(`No built assets found in ${INSPECTOR_DIST}`);
  }
  const imports = files
    .map((file, i) => {
      const abs = join(INSPECTOR_DIST, file).replaceAll("\\", "/");
      return `import file_${i} from ${JSON.stringify(abs)} with { type: "file" };`;
    })
    .join("\n");
  const entries = files
    .map((file, i) => `  ${JSON.stringify(toUrlPath(file))}: file_${i},`)
    .join("\n");
  const content = `// Auto-generated — maps inspector UI URL paths to embedded file references.
${imports}

export default {
${entries}
} as Record<string, string>;
`;
  await writeFile(GEN_PATH, content, "utf8");
  return files.length;
};

/** Read the CLI package version (CI stamps `package.json` before the build). */
const readVersion = async () => {
  const pkg = await Bun.file(join(CLI_ROOT, "package.json")).json();
  return typeof pkg.version === "string" ? pkg.version : "0.0.0-dev";
};

/** Compile `src/index.ts` into a standalone binary for `target`. */
const compileBinary = async (
  target: Bun.Build.CompileTarget,
  version: string
) => {
  const outfile = join(CLI_ROOT, "dist", target, "peektrace");
  const unused = unusedNativePackageFor(target);
  await Bun.build({
    entrypoints: [ENTRYPOINT],
    minify: true,
    // Bake the real version into the binary; `index.ts` reads `PEEKTRACE_VERSION`
    // (a bare undeclared global when run from source, guarded by `typeof`).
    define: { PEEKTRACE_VERSION: JSON.stringify(version) },
    plugins: unused ? [excludeNativePackage(unused, target)] : [],
    compile: { target, outfile },
  });
  return outfile;
};

const main = async () => {
  const target = resolveTarget();
  const version = await readVersion();
  assertNativeDepsInstalled(target);
  buildInspector();
  try {
    const count = await generateEmbeddedManifest();
    console.log(`Embedded ${count} inspector asset(s).`);
    const outfile = await compileBinary(target, version);
    console.log(`Built ${target} binary: ${outfile} (v${version})`);
  } finally {
    await writeFile(GEN_PATH, GEN_STUB, "utf8");
  }
};

await main();
