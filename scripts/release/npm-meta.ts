/** npm package metadata shared by `build-npm.ts` and its tests. */
import { readFileSync } from "node:fs";
import { join } from "node:path";

const HELP_RS = join(import.meta.dir, "../../crates/cli/src/help.rs");
const DESCRIPTION_RE = /pub const DESCRIPTION: &str = "([^"]+)";/;

/** The one-line description the CLI prints in `--help`. */
const readDescription = () => {
  const found = DESCRIPTION_RE.exec(readFileSync(HELP_RS, "utf-8"))?.[1];

  if (!found) {
    throw new Error(`No DESCRIPTION const in ${HELP_RS}`);
  }

  return found;
};

export const DESCRIPTION = readDescription();

export const KEYWORDS = [
  "claude",
  "claude-code",
  "codex",
  "pi",
  "opencode",
  "context-window",
  "inspector",
  "cli",
  "memory",
  "sessions",
  "stats",
] as const;

/** One publishable per-platform binary variant. */
export interface Target {
  readonly arch: "x64" | "arm64";
  /** GitHub Release asset the variant ships, as the installers name it. */
  readonly asset: string;
  readonly platform: "darwin" | "linux" | "win32";
}

/** The platforms shipped as `@peektrace/cli-<platform>-<arch>` packages. */
export const TARGETS = [
  { arch: "arm64", asset: "peektrace-darwin-arm64", platform: "darwin" },
  { arch: "x64", asset: "peektrace-darwin-x64", platform: "darwin" },
  { arch: "x64", asset: "peektrace-linux-x64", platform: "linux" },
  { arch: "arm64", asset: "peektrace-linux-arm64", platform: "linux" },
  { arch: "x64", asset: "peektrace-windows-x64.exe", platform: "win32" },
] as const satisfies readonly Target[];

/** `<platform>-<arch>`, the key `--targets` takes. */
export const targetKey = (t: Pick<Target, "platform" | "arch">) =>
  `${t.platform}-${t.arch}`;

/** npm package name for a per-platform binary variant. */
export const variantName = (t: Pick<Target, "platform" | "arch">) =>
  `@peektrace/cli-${targetKey(t)}`;

/** Binary filename inside a variant (`.exe` only on Windows). */
export const binaryName = (t: Pick<Target, "platform">) =>
  t.platform === "win32" ? "peektrace.exe" : "peektrace";
