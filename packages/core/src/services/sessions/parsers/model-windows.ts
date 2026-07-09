/** Best-effort context-window lookup by model id, for agents whose transcripts
 * don't record the window (Pi). Returns `undefined` when unknown so the caller
 * leaves `nativeContextWindow` unset and `analyze` marks the window inferred.
 *
 * Keyed on substrings of the model id (Pi model ids look like `gpt-5.5`,
 * `claude-opus-4-8`, `qwen/qwen3.6-27b`, `z-ai/glm-5.2`), matched longest-first
 * so more specific families win. This is intentionally a small, low-confidence
 * heuristic — not authoritative context like Codex's `model_context_window`.
 */

const K128 = 128_000;
const K131 = 131_072;
const K200 = 200_000;
const K250 = 250_000;
const K258 = 258_400;
const M1 = 1_000_000;

/** Ordered [needle, window] pairs; first case-insensitive substring hit wins. */
const WINDOWS: ReadonlyArray<readonly [string, number]> = [
  ["claude-opus", K200],
  ["claude-sonnet", M1],
  ["claude-haiku", K200],
  ["claude", K200],
  ["gpt-5", K258],
  ["gpt-oss", K131],
  ["glm-5", K250],
  ["glm", K128],
  ["qwen3", K131],
  ["qwen", K131],
  ["gemma", K128],
  ["gemini", M1],
  ["llama", K128],
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
