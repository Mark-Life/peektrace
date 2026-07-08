/**
 * Stable type for the embedded inspector asset manifest.
 *
 * The `.ts` sibling momentarily holds machine-specific generated imports during
 * a binary build, so this declaration pins the public shape independently:
 * `null` from source, a URL-path -> embedded-file-path map inside a compiled
 * binary.
 */
declare const files: Record<string, string> | null;
export default files;
