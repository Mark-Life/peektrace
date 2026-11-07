/** Raw-argv helpers needed before @effect/cli parses anything. */

/** Global flags that take a separate value argument. */
const VALUE_FLAGS = new Set(["--remote", "--log-level", "--completions"]);

/** First positional (subcommand) in `argv`, skipping global flags and the
 * value of `--remote <url>` and the like. `argv` is the full `process.argv`. */
export const firstPositional = (argv: readonly string[]) => {
  for (let i = 2; i < argv.length; i++) {
    const arg = argv[i] ?? "";
    if (VALUE_FLAGS.has(arg)) {
      i++;
    } else if (!arg.startsWith("-")) {
      return arg;
    }
  }
  return;
};

/** Commands that run on every status-bar refresh and must not write telemetry. */
export const skipsTelemetry = (argv: readonly string[]) =>
  firstPositional(argv) === "statusline";
