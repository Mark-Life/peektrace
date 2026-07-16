/** The running CLI version, resolved once at module load.
 *
 * `PEEKTRACE_VERSION` is injected at compile time by `src/build.ts` (Bun
 * `define`, a global identifier replacement that fires in any module). Running
 * from source it is a bare undeclared global, so the `typeof` guard avoids a
 * `ReferenceError` and falls back to the `0.0.0-dev` sentinel.
 *
 * Kept in its own side-effect-free module so `serve`/`upgrade` can read it
 * without importing `index.ts` (which boots and runs the CLI on load).
 */

declare const PEEKTRACE_VERSION: string | undefined;

/** The build version reported by the CLI and stamped on every wide event. */
export const APP_VERSION =
  (typeof PEEKTRACE_VERSION === "string" ? PEEKTRACE_VERSION : undefined) ??
  "0.0.0-dev";
