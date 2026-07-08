#!/usr/bin/env bun
/** Compile the peephole CLI into a single self-contained binary.
 *
 * Steps:
 * 1. Build the inspector UI (`bun run --filter=inspector build`) into
 *    `apps/inspector/dist`.
 * 2. Generate `src/embedded-ui.gen.ts`: one `import … with { type: "file" }`
 *    per built asset plus a default-exported map of URL path -> embedded file
 *    reference. The import attribute is what makes `bun build --compile` bake
 *    each asset into the binary's virtual filesystem.
 * 3. `bun build --compile` `src/index.ts` for the host target (or `BUN_TARGET`),
 *    emitting `dist/<target>/peephole`.
 * 4. Restore the committed stub of `src/embedded-ui.gen.ts` in a `finally`, so
 *    the working tree is never left with machine-specific generated imports.
 *
 * No native/WASM staging — the UI is the only thing embedded.
 */
import { spawnSync } from "node:child_process";
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

/** Compile `src/index.ts` into a standalone binary for `target`. */
const compileBinary = async (target: Bun.Build.CompileTarget) => {
  const outfile = join(CLI_ROOT, "dist", target, "peephole");
  await Bun.build({
    entrypoints: [ENTRYPOINT],
    minify: true,
    compile: { target, outfile },
  });
  return outfile;
};

const main = async () => {
  const target = resolveTarget();
  buildInspector();
  try {
    const count = await generateEmbeddedManifest();
    console.log(`Embedded ${count} inspector asset(s).`);
    const outfile = await compileBinary(target);
    console.log(`Built ${target} binary: ${outfile}`);
  } finally {
    await writeFile(GEN_PATH, GEN_STUB, "utf8");
  }
};

await main();
