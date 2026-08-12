import {
  binName,
  isEnvAssign,
  isFlag,
  isLiteral,
  isPathLike,
  REDIRECT,
  scriptBase,
  splitSegments,
  stripHeredocs,
  tokenize,
  type Word,
} from "./shell-lex";

/**
 * One vocabulary, two entry points: the family a shell command belongs to, and
 * the signature that groups errors into clusters. Both reduce a string to the
 * shape it shares with its neighbours; only the masks differ.
 *
 * Six rules separate this from a plain mask chain, each one a cluster that was
 * wrong without it. A shell error is keyed on its last line, because the first
 * line is the exit code and the informative line is the tail. The exit code
 * itself stays unmasked, or six unrelated causes share one row. A repeated
 * clause collapses before the text is clipped, because a schema rejection
 * repeats one clause per missing field and would otherwise split by field count.
 * A `<tool_use_error>` is cut at its first sentence, because the rest is advice
 * that varies with the payload. Durations, hostnames, the leading binary of a
 * coreutils message and the plural `issue(s)` are masked. Quoted strings are
 * kept: the missing property name is the finding, so it survives into the fact
 * and is folded away only by `clusterKeyOf`, one level up.
 */

/** A command reduced to its binary and its flag-collapsed family. */
export interface NormalizedCommand {
  /** Base binary, e.g. `bun`, `git`, `rg`. */
  readonly bin: string;
  /** Normalised family, e.g. `bun run typecheck`. */
  readonly family: string;
}

/** A run of whitespace, collapsed to one space. */
const WS_RUN = /\s+/g;

/** Chars an error signature keeps. */
const SIG_MAX = 200;

/** Chars a command family key keeps. */
const FAMILY_MAX = 120;

/** Family words kept after the binary. */
const FAMILY_BUDGET = 3;

/** Wrapper hops unwrapped before giving up on a nested command. */
const UNWRAP_GUARD = 4;

/**
 * Segments that set up a shell but do no work, so never name a family. `echo`
 * is one of them: it prints a banner in 2,768 calls, 7% of all bash, and names
 * no work in any of them.
 */
const SETUP_BINS: ReadonlySet<string> = new Set([
  ".",
  "alias",
  "cd",
  "echo",
  "exec",
  "export",
  "popd",
  "pushd",
  "set",
  "shopt",
  "source",
  "trap",
  "true",
  "umask",
  "unset",
]);

/** Wrappers that stand in front of the real command. */
const UNARY_WRAPPERS: ReadonlySet<string> = new Set([
  "bunx",
  "command",
  "env",
  "nohup",
  "npx",
  "pnpx",
  "stdbuf",
  "sudo",
  "time",
]);

/** Two-word wrappers: `<bin> <word>` runs the next word as the real command. */
const BINARY_WRAPPERS: Record<string, ReadonlySet<string>> = {
  bun: new Set(["x"]),
  npm: new Set(["exec", "x"]),
  pnpm: new Set(["dlx", "exec"]),
  yarn: new Set(["dlx"]),
};

/** Binaries whose first path argument is a script worth keeping in the family. */
const SCRIPT_RUNNERS: ReadonlySet<string> = new Set([
  "bash",
  "bun",
  "deno",
  "fish",
  "node",
  "perl",
  "php",
  "poetry",
  "py",
  "python",
  "python3",
  "ruby",
  "sh",
  "ts-node",
  "tsx",
  "uv",
  "uvx",
  "zsh",
]);

/** Words after which the next bare word is the script or task name. */
const RUN_WORDS: ReadonlySet<string> = new Set([
  "dlx",
  "exec",
  "run",
  "run-script",
  "task",
  "test",
  "workspace",
  "x",
]);

/**
 * Coreutils whose message opens `<bin>: <path>: <reason>`. Masking the binary
 * merges `ls:`, `cat:` and `sed:` reporting the same missing path.
 */
const MESSAGE_BINS: ReadonlySet<string> = new Set([
  "awk",
  "cat",
  "cp",
  "find",
  "grep",
  "head",
  "ls",
  "mkdir",
  "mv",
  "rm",
  "sed",
  "sort",
  "stat",
  "tail",
  "wc",
]);

/** Pick the first segment that does real work, falling back to the first one. */
const meaningfulSegment = (segments: readonly string[]) => {
  for (const s of segments) {
    const words = tokenize(s).filter((w) => !isEnvAssign(w.text));
    const first = words[0];
    if (first && !SETUP_BINS.has(binName(first.text))) {
      return s;
    }
  }
  return segments[0] ?? "";
};

/** Drop leading env assignments and wrapper binaries, returning the real words. */
const unwrap = (words: readonly Word[]) => {
  let ws = words;
  while (ws.length > 0 && isEnvAssign(ws[0]?.text ?? "")) {
    ws = ws.slice(1);
  }
  for (let guard = 0; guard < UNWRAP_GUARD && ws.length > 0; guard++) {
    const head = binName(ws[0]?.text ?? "");
    if (UNARY_WRAPPERS.has(head)) {
      ws = ws.slice(1);
      while (
        ws.length > 0 &&
        (isEnvAssign(ws[0]?.text ?? "") || isFlag(ws[0]?.text ?? ""))
      ) {
        ws = ws.slice(1);
      }
      continue;
    }
    const second = ws[1] ? binName(ws[1].text) : "";
    if (BINARY_WRAPPERS[head]?.has(second)) {
      ws = ws.slice(2);
      continue;
    }
    break;
  }
  return ws;
};

/** Walk state while the family words are picked out of one segment. */
interface FamilyScan {
  kept: string[];
  pendingFlagValue: boolean;
  pendingRedirectTarget: boolean;
  sawOperand: boolean;
  sawScript: boolean;
}

/** Fold one word into the family scan. */
const stepFamily = (scan: FamilyScan, word: Word, bin: string): void => {
  const { text } = word;
  const redirect = REDIRECT.exec(text);
  if (redirect) {
    // `2>&1` carries its own target; a bare `>` swallows the next word.
    scan.pendingRedirectTarget = redirect[0].length === text.length;
    scan.pendingFlagValue = false;
    return;
  }
  if (scan.pendingRedirectTarget) {
    scan.pendingRedirectTarget = false;
    return;
  }
  if (isFlag(text)) {
    scan.pendingFlagValue = !text.includes("=") && text !== "--";
    return;
  }
  // Bare `-` and `--` mean stdin or end-of-flags: never part of a family.
  if (text === "-" || text === "--") {
    return;
  }
  const scriptSlot =
    SCRIPT_RUNNERS.has(bin) ||
    (scan.kept.length > 0 && RUN_WORDS.has(scan.kept.at(-1) ?? ""));
  if (isPathLike(text) && !word.quoted) {
    // A script path names the work; any other path is just an argument.
    const base = scriptBase(text);
    if (scriptSlot && !scan.sawScript && base && !isLiteral(base)) {
      scan.kept.push(base);
      scan.sawScript = true;
    }
    scan.sawOperand = true;
    scan.pendingFlagValue = false;
    return;
  }
  if (scan.pendingFlagValue) {
    scan.pendingFlagValue = false;
    return;
  }
  if (word.quoted || isLiteral(text) || isEnvAssign(text)) {
    // A pattern or a literal ends the subcommand run: what follows is operands.
    scan.sawOperand = true;
    return;
  }
  if (scan.sawOperand) {
    return;
  }
  scan.kept.push(text);
  if (scriptSlot) {
    scan.sawScript = true;
  }
};

/** `for x in …` head. 233 shipped families, 1,178 calls, all one shape. */
const FOR_LOOP = /^for\s+[A-Za-z_][A-Za-z0-9_]*\s+in\b/;

/**
 * Reduce a shell command to its base binary and its family. Safe on any input:
 * an unparseable command yields its first word as both.
 */
export const normalizeCommand = (raw: string): NormalizedCommand => {
  const cmd = stripHeredocs(raw ?? "");
  const { segments } = splitSegments(cmd);
  const primary = meaningfulSegment(segments);
  if (FOR_LOOP.test(primary)) {
    return { bin: "for", family: "for-loop" };
  }
  const words = unwrap(tokenize(primary));
  const bin = words.length > 0 ? binName(words[0]?.text ?? "") : "";
  const scan: FamilyScan = {
    kept: [],
    sawScript: false,
    sawOperand: false,
    pendingFlagValue: false,
    pendingRedirectTarget: false,
  };
  for (let i = 1; i < words.length && scan.kept.length < FAMILY_BUDGET; i++) {
    const word = words[i];
    if (word) {
      stepFamily(scan, word, bin);
    }
  }
  const family =
    [bin, ...scan.kept].filter(Boolean).join(" ") ||
    raw.trim().split(WS_RUN)[0] ||
    "";
  return { bin, family: family.slice(0, FAMILY_MAX) };
};

/** A single-quoted, double-quoted or backticked span. */
const QUOTED = /'[^']*'|"[^"]*"|`[^`]*`/g;

/** Exit-code banner a shell result opens with. The number is never masked. */
const EXIT_CODE = /^Exit code (\d+)\b/;

/** The wrapper an agent puts around a refused or invalid tool call. */
const TOOL_USE_ERROR = /<tool_use_error>([\s\S]*?)(?:<\/tool_use_error>|$)/;

/** `2m 0s`, `1.5s`, `300ms`: a wall-clock figure, never a count. */
const DURATION =
  /\b\d+(?:\.\d+)?\s*(?:h|hr|hrs)\b\s*\d*(?:\.\d+)?\s*m?\b|\b\d+(?:\.\d+)?m\s+\d+(?:\.\d+)?s\b|\b\d+(?:\.\d+)?(?:ms|s)\b/g;

/** A dotted host: two or more labels then an alphabetic suffix. */
const HOSTNAME = /\b(?:[a-z0-9](?:[a-z0-9-]*[a-z0-9])?\.){2,}[a-z]{2,24}\b/gi;

const URL = /[a-z][a-z0-9+.-]*:\/\/\S+/gi;
const UUID =
  /\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b/gi;
const PATHY = /(?:[A-Za-z]:)?(?:[\w.@~+-]*[\\/])+[\w.@+-]*/g;
const HEX = /\b[0-9a-f]{7,}\b/gi;
const NUMBER = /\b\d+(?:[.,]\d+)*\b/g;
const PLURAL = /\bissues?\b/gi;

/** A coreutils message opens `<bin>: `. */
const LEADING_BIN = /^([a-z][\w.-]*):\s/;

/** End of the first sentence of a message. */
const SENTENCE_END = /[.!?](?:\s|$)/;

/** Clause separator inside a validator's complaint list. */
const CLAUSE_SPLIT = /,\s+/;

/** Mask the literals that vary between two failures of the same shape. */
const maskLiterals = (s: string) =>
  s
    .replace(URL, "<url>")
    .replace(UUID, "<uuid>")
    .replace(DURATION, "<dur>")
    .replace(PATHY, "<path>")
    .replace(HOSTNAME, "<host>")
    .replace(HEX, "<hash>")
    .replace(NUMBER, "<n>")
    .replace(PLURAL, "issue(s)");

/** Mask the leading `ls:` / `cat:` / `sed:` of a coreutils message. */
const maskLeadingBin = (line: string) => {
  const m = LEADING_BIN.exec(line);
  const name = m?.[1];
  return name && MESSAGE_BINS.has(name)
    ? `<bin>: ${line.slice(m?.[0].length ?? 0)}`
    : line;
};

const nonEmptyLines = (text: string) =>
  text
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);

/** First sentence of a message, or the first line when it ends sooner. */
const firstSentence = (text: string) => {
  const line = nonEmptyLines(text)[0] ?? "";
  const stop = SENTENCE_END.exec(line);
  return stop ? line.slice(0, stop.index + 1) : line;
};

/**
 * Collapse a list of clauses that repeat one shape, keeping the first. A schema
 * rejection prints one clause per missing field, so without this the same defect
 * splits into a row per field count.
 */
const collapseClauses = (s: string) => {
  const parts = s.split(CLAUSE_SPLIT);
  if (parts.length < 2) {
    return s;
  }
  const shapes = parts.map((p) => p.replace(QUOTED, "<str>"));
  const repeat = shapes[1] ?? "";
  const repeats =
    repeat.length > 0 &&
    shapes.slice(1).every((shape) => shape === repeat) &&
    (shapes[0] ?? "").endsWith(repeat);
  return repeats ? (parts[0] ?? s) : s;
};

const collapseWhitespace = (s: string) => s.replace(WS_RUN, " ").trim();

const finishSig = (body: string, prefix: string) => {
  const collapsed = collapseWhitespace(collapseClauses(body));
  const joined = prefix ? `${prefix} ${collapsed}`.trim() : collapsed;
  return joined.slice(0, SIG_MAX);
};

/**
 * Shell key: the exit code verbatim, then the last informative line. The head of
 * a shell result is the banner and whatever the command printed before it died;
 * the line that names the failure is the last one.
 */
const shellSignature = (text: string) => {
  const lines = nonEmptyLines(text);
  const exit = EXIT_CODE.exec(lines[0] ?? "");
  const body = exit ? lines.slice(1) : lines;
  const tail = body.at(-1) ?? "";
  const prefix = exit ? `Exit code ${exit[1]}` : "";
  return finishSig(maskLiterals(maskLeadingBin(tail)), prefix);
};

/** Tool names that run a shell, matched on the name rather than a fixed list. */
const SHELL_TOOL = /(?:^|_)(?:bash|shell|run_command|execute_command)/i;

const isShellResult = (tool: string, text: string) =>
  EXIT_CODE.test(text) || SHELL_TOOL.test(tool);

/**
 * The masked cluster key for one error, or null when the text carries none.
 * Quoted strings survive: the missing property name, the module that could not
 * be found and the variable that was undefined are the whole finding.
 */
export const errorSignature = (args: {
  readonly text: string;
  readonly tool: string;
}): string | null => {
  const text = (args.text ?? "").trim();
  if (!text) {
    return null;
  }
  const wrapped = TOOL_USE_ERROR.exec(text);
  if (wrapped) {
    const sig = finishSig(maskLiterals(firstSentence(wrapped[1] ?? "")), "");
    return sig || null;
  }
  if (isShellResult(args.tool, text)) {
    return shellSignature(text) || null;
  }
  const head = nonEmptyLines(text).slice(0, 2).join(" ");
  return finishSig(maskLiterals(head), "") || null;
};

/**
 * `<intro>: <location>: <complaint>` — a validator addressing one part of a
 * document. The location is an address, not a shape, so the cluster is the
 * intro; which field was wrong belongs in the row's samples.
 */
const VALIDATION_LIST = /^(.{6,80}?):\s+(?:[\w./#[\]-]{1,60}|<path>|<n>):\s+\S/;

/**
 * Fold a signature to its cluster key. This is the one place a quoted literal is
 * masked: the fact keeps the name so a sample line can print it, and the table
 * groups every rejection of the same shape into one row.
 */
export const clusterKeyOf = (sig: string) => {
  const masked = collapseWhitespace(sig.replace(QUOTED, "<str>"));
  if (EXIT_CODE.test(masked)) {
    return masked.slice(0, SIG_MAX);
  }
  const list = VALIDATION_LIST.exec(masked);
  return (list?.[1] ? `${list[1]}:` : masked).slice(0, SIG_MAX);
};

/** The quoted literals a cluster key masked away, in the order they appeared. */
export const detailsOf = (sig: string): readonly string[] =>
  (sig.match(QUOTED) ?? []).map((q) => q.slice(1, -1)).filter(Boolean);
