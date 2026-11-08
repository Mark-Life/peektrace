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

/** Flag name without an `=value` suffix. */
const flagName = (arg: string) => arg.split("=")[0] ?? arg;

const isHelpFlag = (arg: string) => {
  const name = flagName(arg);
  return name === "--help" || name === "-h";
};

/** True when `--help` / `-h` comes before any subcommand (the owned root page).
 * `--completions` wins over help, as in the lib. */
export const isRootHelp = (argv: readonly string[]) => {
  const args = argv.slice(2);
  if (args.some((arg) => flagName(arg) === "--completions")) {
    return false;
  }
  const first = firstPositional(argv);
  const helpAt = args.findIndex(isHelpFlag);
  if (helpAt < 0) {
    return false;
  }
  return first === undefined || args.indexOf(first) > helpAt;
};
