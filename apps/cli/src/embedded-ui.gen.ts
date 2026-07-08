/**
 * Embedded inspector asset manifest.
 *
 * Committed stub — the default export is `null` when running from source, which
 * makes the server fall back to serving the inspector `dist/` from disk.
 *
 * The binary build (`src/build.ts`) overwrites this file in place with one
 * `import ... with { type: "file" }` per built asset plus a default-exported map
 * of URL path (`/index.html`, `/assets/…`) -> embedded file reference, so that
 * `bun build --compile` bakes every asset into the binary's virtual filesystem.
 * The build restores this stub afterwards; the generated (non-null) form is a
 * local build artifact and must never be committed.
 */
const files: Record<string, string> | null = null;

export default files;
