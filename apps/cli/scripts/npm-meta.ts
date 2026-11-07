/** Pure npm package metadata shared by `build-npm.ts` and its tests. */

export { DESCRIPTION } from "../src/description";

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
  readonly bunTarget: Bun.Build.CompileTarget;
  readonly platform: "darwin" | "linux" | "win32";
}

/** The matrix of platforms shipped as `@peektrace/cli-<platform>-<arch>` packages. */
export const TARGETS = [
  { platform: "darwin", arch: "arm64", bunTarget: "bun-darwin-arm64" },
  { platform: "darwin", arch: "x64", bunTarget: "bun-darwin-x64" },
  { platform: "linux", arch: "x64", bunTarget: "bun-linux-x64" },
  { platform: "linux", arch: "arm64", bunTarget: "bun-linux-arm64" },
  { platform: "win32", arch: "x64", bunTarget: "bun-windows-x64" },
] as const satisfies readonly Target[];

/** npm package name for a per-platform binary variant. */
export const variantName = (t: Pick<Target, "platform" | "arch">) =>
  `@peektrace/cli-${t.platform}-${t.arch}`;
