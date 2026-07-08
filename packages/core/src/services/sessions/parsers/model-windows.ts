/** Best-effort context-window lookup by model id, for agents whose transcripts
 * don't record the window (Pi). Returns `undefined` when unknown so the caller
 * leaves `nativeContextWindow` unset and `analyze` marks the window inferred.
 *
 * Keyed on substrings of the model id (Pi model ids look like `gpt-5.5`,
 * `claude-opus-4-8`, `qwen/qwen3.6-27b`, `z-ai/glm-5.2`), matched longest-first
 * so more specific families win. This is intentionally a small, low-confidence
 * heuristic — not authoritative context like Codex's `model_context_window`.
 */

/** Ordered [needle, window] pairs; first case-insensitive substring hit wins. */
const WINDOWS: ReadonlyArray<readonly [string, number]> = [
  ["claude-opus", 200_000],
  ["claude-sonnet", 1_000_000],
  ["claude-haiku", 200_000],
  ["claude", 200_000],
  ["gpt-5", 258_400],
  ["gpt-oss", 131_072],
  ["glm-5", 250_000],
  ["glm", 128_000],
  ["qwen3", 131_072],
  ["qwen", 131_072],
  ["gemma", 128_000],
  ["gemini", 1_000_000],
  ["llama", 128_000],
];

/** Look up a model's context window by id substring, or `undefined`. */
export const windowForModel = (
  model: string | undefined
): number | undefined => {
  if (!model) {
    return;
  }
  const needle = model.toLowerCase();
  for (const [key, window] of WINDOWS) {
    if (needle.includes(key)) {
      return window;
    }
  }
  return;
};
